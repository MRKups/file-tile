//! Per-view motion state and symmetric forward/reverse primitives for folder refresh transitions.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use iced::widget::{Stack, column, container, pin, row, space, text};
use iced::{Background, Border, Color, Element, Fill, Padding};

use crate::application::contracts::directory::{DirectoryEntry, DirectoryListing};
use crate::application::files::listing_diff::{ArrivalKind, DeletedGhost, ExitKind, ListingDiff};
use crate::{
    Message,
    boxes::file_list::ROW_HEIGHT,
    window::appearance as a,
    window::icons::{self, Icon},
};

/// Shared physical transition span for appearing (0 -> 1), disappearing (1 -> 0), and renaming items.
pub(crate) const MOTION_DURATION: Duration = Duration::from_millis(500);

const ARRIVAL_FLASH_IN: Duration = Duration::from_millis(90);
const ARRIVAL_FADEOUT: Duration = Duration::from_millis(1500);
pub(crate) const ARRIVAL_DURATION: Duration = Duration::from_millis(1590);

const EXIT_FLASH_IN: Duration = Duration::from_millis(70);

/// Rolling upper bound across staggered consecutive reloads; evicts only the oldest motion.
const MAX_CONCURRENT_MOTIONS: usize = 4096;
/// Cap simultaneously height-animating slots so viewport virtualization stays bounded on massive bursts.
const MAX_LAYOUT_MOTION_SLOTS: usize = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tone {
    Accent,
    Rename,
    Exit,
}

/// Unified presence curve evaluated forward (0 -> 1) on appear and in reverse (1 -> 0) on disappear.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Presence {
    pub space: f32,
    pub alpha: f32,
    pub offset: f32,
}

impl Presence {
    fn from_phase(u: f32) -> Self {
        let u = u.clamp(0.0, 1.0);
        let space = 0.5 * (1.0 - (std::f32::consts::PI * u).cos());
        let alpha = space.powf(0.65);
        let offset = 1.0 - space;
        Self {
            space,
            alpha,
            offset,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ActiveArrival {
    kind: ArrivalKind,
    animate_height: bool,
    started_at: Instant,
}

#[derive(Debug, Clone)]
struct ActiveDeletion {
    id: u64,
    ghost: DeletedGhost,
    started_at: Instant,
}

#[derive(Debug, Clone)]
pub(crate) struct ViewMotion {
    arrivals: HashMap<PathBuf, ActiveArrival>,
    deletions: Vec<ActiveDeletion>,
    has_entering_height: bool,
    next_ghost_id: u64,
    now: Instant,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ArrivalVisual {
    pub tone: Tone,
    pub strength: f32,
    pub height_factor: f32,
    pub icon_alpha: f32,
    pub label_alpha: f32,
    pub row_offset: f32,
    pub label_offset: f32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Slot<'a> {
    Live {
        index: usize,
        entry: &'a DirectoryEntry,
        height_factor: f32,
    },
    Ghost {
        id: u64,
        entry: &'a DirectoryEntry,
        tone: Tone,
        presence: Presence,
        tint: f32,
    },
}

impl Slot<'_> {
    pub(crate) fn height(self, row_height: f32) -> f32 {
        match self {
            Self::Live { height_factor, .. } => (row_height * height_factor).max(0.5),
            Self::Ghost { presence, .. } => (row_height * presence.space).max(0.5),
        }
    }
}

impl ViewMotion {
    fn new(now: Instant) -> Self {
        Self {
            arrivals: HashMap::new(),
            deletions: Vec::new(),
            has_entering_height: false,
            next_ghost_id: 1,
            now,
        }
    }

    pub(crate) fn apply_diff(
        slot: &mut Option<Self>,
        diff: ListingDiff,
        current: &DirectoryListing,
        show_hidden: bool,
        prior_scroll_offset: f32,
        now: Instant,
    ) {
        match diff {
            ListingDiff::Unchanged => {
                if let Some(motion) = slot {
                    motion.prune_invisible_arrivals(current, show_hidden);
                    if !motion.tick(now) {
                        *slot = None;
                    }
                }
            }
            ListingDiff::Bulk => {
                *slot = None;
            }
            ListingDiff::Animated {
                arrivals,
                deletions,
            } => {
                let motion = slot.get_or_insert_with(|| Self::new(now));
                motion.now = now;
                motion.prune_invisible_arrivals(current, show_hidden);
                motion.merge_animated(arrivals, deletions, prior_scroll_offset, now);
                motion.evict_excess();
                if !motion.tick(now) {
                    *slot = None;
                }
            }
        }
    }

    fn merge_animated(
        &mut self,
        arrivals: HashMap<PathBuf, ArrivalKind>,
        mut deletions: Vec<DeletedGhost>,
        prior_scroll_offset: f32,
        now: Instant,
    ) {
        for deleted in &deletions {
            self.arrivals.remove(&deleted.entry.path);
            for active in &mut self.deletions {
                if active.ghost.after_path.as_deref() == Some(deleted.entry.path.as_path()) {
                    active.ghost.after_path.clone_from(&deleted.after_path);
                }
            }
        }

        let mut active_height_slots = self
            .arrivals
            .values()
            .filter(|item| {
                item.animate_height
                    && now.saturating_duration_since(item.started_at) < MOTION_DURATION
            })
            .count();

        let mut sorted_arrivals: Vec<(PathBuf, ArrivalKind)> = arrivals.into_iter().collect();
        sorted_arrivals.sort_by(|(a, _), (b, _)| a.cmp(b));
        for (path, kind) in sorted_arrivals {
            self.deletions
                .retain(|active| active.ghost.entry.path != path);
            self.arrivals.entry(path).or_insert_with(|| {
                let needs_slot_expansion = matches!(
                    kind,
                    ArrivalKind::Created | ArrivalKind::Renamed { relocated: true }
                );
                let animate_height =
                    needs_slot_expansion && active_height_slots < MAX_LAYOUT_MOTION_SLOTS;
                if animate_height {
                    active_height_slots += 1;
                }
                ActiveArrival {
                    kind,
                    animate_height,
                    started_at: now,
                }
            });
        }

        if deletions.is_empty() {
            return;
        }
        if deletions.len() > MAX_LAYOUT_MOTION_SLOTS {
            let approx_row = (prior_scroll_offset.max(0.0) / ROW_HEIGHT).floor() as usize;
            let start =
                deletions.partition_point(|ghost| ghost.prior_index < approx_row.saturating_sub(4));
            let end = (start + MAX_LAYOUT_MOTION_SLOTS).min(deletions.len());
            let start = end.saturating_sub(MAX_LAYOUT_MOTION_SLOTS);
            deletions = deletions.drain(start..end).collect();
        }

        let mut next_deletions = Vec::with_capacity(deletions.len() + self.deletions.len());
        for ghost in deletions {
            let id = self.next_ghost_id;
            self.next_ghost_id = self.next_ghost_id.saturating_add(1);
            next_deletions.push(ActiveDeletion {
                id,
                ghost,
                started_at: now,
            });
        }
        for existing in self.deletions.drain(..) {
            if !next_deletions
                .iter()
                .any(|item| item.ghost.entry.path == existing.ghost.entry.path)
            {
                next_deletions.push(existing);
            }
        }
        next_deletions.truncate(MAX_LAYOUT_MOTION_SLOTS);
        self.deletions = next_deletions;
    }

    fn evict_excess(&mut self) {
        while self.arrivals.len() + self.deletions.len() > MAX_CONCURRENT_MOTIONS {
            let oldest_arrival = self
                .arrivals
                .iter()
                .min_by_key(|(_, item)| item.started_at)
                .map(|(path, item)| (path.clone(), item.started_at));
            let oldest_deletion = self
                .deletions
                .iter()
                .enumerate()
                .min_by_key(|(_, item)| item.started_at)
                .map(|(idx, item)| (idx, item.started_at));
            match (oldest_arrival, oldest_deletion) {
                (Some((path, arr_t)), Some((_, del_t))) if arr_t <= del_t => {
                    self.arrivals.remove(&path);
                }
                (_, Some((idx, _))) => {
                    self.deletions.remove(idx);
                }
                (Some((path, _)), None) => {
                    self.arrivals.remove(&path);
                }
                (None, None) => break,
            }
        }
    }

    fn prune_invisible_arrivals(&mut self, current: &DirectoryListing, show_hidden: bool) {
        self.arrivals.retain(|path, _| {
            current
                .entry(path)
                .is_some_and(|entry| show_hidden || !entry.hidden)
        });
    }

    pub(crate) fn tick(&mut self, now: Instant) -> bool {
        self.now = now;
        self.arrivals.retain(|_, arrival| {
            now.saturating_duration_since(arrival.started_at) < ARRIVAL_DURATION
        });
        self.deletions.retain(|deletion| {
            now.saturating_duration_since(deletion.started_at) < MOTION_DURATION
        });
        self.has_entering_height = self.arrivals.values().any(|arrival| {
            arrival.animate_height
                && now.saturating_duration_since(arrival.started_at) < MOTION_DURATION
        });
        !self.arrivals.is_empty() || !self.deletions.is_empty()
    }

    pub(crate) fn arrival(&self, path: &Path) -> Option<ArrivalVisual> {
        let arrival = self.arrivals.get(path)?;
        let elapsed = self.now.saturating_duration_since(arrival.started_at);
        if elapsed >= ARRIVAL_DURATION {
            return None;
        }
        let strength = flash_then_fade(elapsed, ARRIVAL_FLASH_IN, ARRIVAL_FADEOUT);
        let raw = if elapsed >= MOTION_DURATION {
            1.0
        } else {
            (elapsed.as_secs_f32() / MOTION_DURATION.as_secs_f32()).clamp(0.0, 1.0)
        };
        let presence = Presence::from_phase(raw);
        match arrival.kind {
            ArrivalKind::Created => {
                let height_factor = if arrival.animate_height {
                    presence.space
                } else {
                    1.0
                };
                Some(ArrivalVisual {
                    tone: Tone::Accent,
                    strength,
                    height_factor,
                    icon_alpha: presence.alpha,
                    label_alpha: presence.alpha,
                    row_offset: presence.offset,
                    label_offset: 0.0,
                })
            }
            ArrivalKind::Renamed { relocated } => {
                let height_factor = if relocated && arrival.animate_height {
                    presence.space
                } else {
                    1.0
                };
                let icon_alpha = if relocated { presence.alpha } else { 1.0 };
                Some(ArrivalVisual {
                    tone: Tone::Rename,
                    strength,
                    height_factor,
                    icon_alpha,
                    label_alpha: presence.alpha,
                    row_offset: 0.0,
                    label_offset: presence.offset,
                })
            }
        }
    }

    pub(crate) fn has_deletions(&self) -> bool {
        !self.deletions.is_empty()
    }

    pub(crate) fn has_layout_motion(&self) -> bool {
        !self.deletions.is_empty() || self.has_entering_height
    }

    pub(crate) fn project_slots<'a>(
        &'a self,
        listing: &'a DirectoryListing,
        order: Option<&'a [usize]>,
        show_hidden: bool,
    ) -> Vec<Slot<'a>> {
        let mut leading = Vec::new();
        let mut after_map: HashMap<&'a Path, Vec<Slot<'a>>> = HashMap::new();
        let exit_fade = MOTION_DURATION.saturating_sub(EXIT_FLASH_IN);

        for active in &self.deletions {
            let elapsed = self.now.saturating_duration_since(active.started_at);
            if elapsed >= MOTION_DURATION {
                continue;
            }
            let raw = (elapsed.as_secs_f32() / MOTION_DURATION.as_secs_f32()).clamp(0.0, 1.0);
            let tone = match active.ghost.kind {
                ExitKind::Deleted => Tone::Exit,
                ExitKind::Relocated => Tone::Rename,
            };
            let slot = Slot::Ghost {
                id: active.id,
                entry: &active.ghost.entry,
                tone,
                presence: Presence::from_phase(1.0 - raw),
                tint: flash_then_fade(elapsed, EXIT_FLASH_IN, exit_fade),
            };
            match active.ghost.after_path.as_deref() {
                None => leading.push(slot),
                Some(anchor) => after_map.entry(anchor).or_default().push(slot),
            }
        }

        let live_count = listing.display_len(show_hidden);
        let mut slots = Vec::with_capacity(live_count + self.deletions.len());
        slots.append(&mut leading);

        for index in 0..live_count {
            let entry = if let Some(order) = order {
                order
                    .get(index)
                    .and_then(|target| listing.entries().get(*target))
            } else {
                listing.display_entry(index, show_hidden)
            };
            if let Some(entry) = entry {
                let height_factor = self
                    .arrival(&entry.path)
                    .map_or(1.0, |visual| visual.height_factor);
                slots.push(Slot::Live {
                    index,
                    entry,
                    height_factor,
                });
                if let Some(mut attached) = after_map.remove(entry.path.as_path()) {
                    slots.append(&mut attached);
                }
            }
        }

        if !after_map.is_empty() {
            let mut trailing: Vec<Slot<'a>> = after_map.into_values().flatten().collect();
            trailing.sort_by_key(|slot| match slot {
                Slot::Ghost { id, .. } => *id,
                Slot::Live { .. } => 0,
            });
            slots.append(&mut trailing);
        }

        slots
    }
}

fn tone_color(tone: Tone, palette: a::Palette) -> Color {
    match tone {
        Tone::Accent => palette.accent,
        Tone::Exit => palette.error,
        Tone::Rename => {
            let bg = palette.contrast_background;
            let lum = 0.2126 * bg.r + 0.7152 * bg.g + 0.0722 * bg.b;
            if lum < 0.35 {
                Color::from_rgb8(245, 184, 46)
            } else {
                Color::from_rgb8(201, 130, 10)
            }
        }
    }
}

fn flash_then_fade(elapsed: Duration, flash_in: Duration, fade_out: Duration) -> f32 {
    if elapsed < flash_in && !flash_in.is_zero() {
        let u = (elapsed.as_secs_f32() / flash_in.as_secs_f32()).clamp(0.0, 1.0);
        1.0 - (1.0 - u).powi(3)
    } else if fade_out.is_zero() {
        0.0
    } else {
        let fade_elapsed = elapsed.saturating_sub(flash_in);
        let t = (fade_elapsed.as_secs_f32() / fade_out.as_secs_f32()).clamp(0.0, 1.0);
        0.5 * (1.0 + (std::f32::consts::PI * t).cos())
    }
}

pub(crate) fn row_background(
    base: Option<Background>,
    arrival: Option<ArrivalVisual>,
    palette: a::Palette,
) -> Option<Background> {
    let Some(visual) = arrival else {
        return base;
    };
    let tint_alpha = (0.22 * visual.strength).clamp(0.0, 1.0);
    if tint_alpha <= 0.001 {
        return base;
    }
    let base_color = match base {
        Some(Background::Color(color)) => Some(color),
        Some(other) => return Some(other),
        None => None,
    };
    Some(Background::Color(overlay_tint(
        base_color,
        tone_color(visual.tone, palette),
        tint_alpha,
    )))
}

pub(crate) fn decorate_row<'a>(
    content: Element<'a, Message>,
    arrival: Option<ArrivalVisual>,
    effective_height: f32,
    palette: a::Palette,
) -> Element<'a, Message> {
    let Some(visual) = arrival else {
        return content;
    };
    let bar_alpha = (0.95 * visual.strength).clamp(0.0, 1.0);
    with_side_bar(
        content,
        tone_color(visual.tone, palette),
        bar_alpha,
        effective_height,
    )
}

fn with_side_bar<'a>(
    content: Element<'a, Message>,
    color: Color,
    bar_alpha: f32,
    effective_height: f32,
) -> Element<'a, Message> {
    if bar_alpha <= 0.001 || effective_height < 3.0 {
        return content;
    }
    let bar_height = (effective_height - 14.0).max(2.0).min(effective_height);
    let bar_y = ((effective_height - bar_height) * 0.5).max(0.0);
    let bar = container(space())
        .width(2.5)
        .height(bar_height)
        .style(move |_| container::Style {
            background: Some(color.scale_alpha(bar_alpha).into()),
            border: Border {
                radius: 1.25.into(),
                ..Border::default()
            },
            ..container::Style::default()
        });
    Stack::new()
        .push(content)
        .push(pin(bar).x(2.0).y(bar_y))
        .width(Fill)
        .height(effective_height)
        .into()
}

pub(crate) fn ghost_row(
    entry: &DirectoryEntry,
    tone: Tone,
    presence: Presence,
    tint: f32,
    palette: a::Palette,
) -> Element<'static, Message> {
    let ty = palette.typography;
    let full_height = ty.row(ROW_HEIGHT);
    let effective_height = (full_height * presence.space).max(0.5);
    let alpha = presence.alpha.clamp(0.0, 1.0);
    let (row_slide_x, label_slide_x) = if tone == Tone::Rename {
        (0.0, presence.offset.max(0.0) * 14.0)
    } else {
        (presence.offset.max(0.0) * 14.0, 0.0)
    };
    let tint_strength = tint.clamp(0.0, 1.0);
    let color = tone_color(tone, palette);
    let icon = Icon::for_entry(entry);
    let icon_size = ty.icon(18.0);
    let name = entry.name.to_string_lossy().into_owned();

    let inner = row![
        container(icons::view(
            icon,
            icon_size,
            Some(palette.icon(icon)),
            alpha
        ))
        .padding(Padding {
            bottom: icon_size / 12.0,
            ..Padding::ZERO
        }),
        container(
            ty.text(name)
                .color(palette.text.scale_alpha(alpha))
                .wrapping(text::Wrapping::None),
        )
        .width(Fill)
        .clip(true)
        .padding(Padding {
            left: label_slide_x,
            ..Padding::ZERO
        })
    ]
    .spacing(10)
    .align_y(iced::Center);

    let body = container(inner)
        .width(Fill)
        .padding(Padding {
            top: 0.0,
            right: 8.0,
            bottom: 0.0,
            left: 8.0 + row_slide_x,
        })
        .height(effective_height)
        .align_y(iced::Center)
        .clip(true)
        .style(move |_| container::Style {
            background: Some(color.scale_alpha(0.16 * tint_strength * alpha).into()),
            border: Border {
                radius: 5.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        });
    with_side_bar(
        body.into(),
        color,
        (0.85 * tint_strength * alpha).clamp(0.0, 1.0),
        effective_height,
    )
}

pub(crate) fn tile_style(
    mut base: container::Style,
    arrival: Option<ArrivalVisual>,
    palette: a::Palette,
) -> container::Style {
    let Some(visual) = arrival else {
        return base;
    };
    let strength = visual.strength.clamp(0.0, 1.0);
    if strength <= 0.001 {
        return base;
    }
    base.background = row_background(base.background, arrival, palette);
    base.border = Border {
        color: tone_color(visual.tone, palette).scale_alpha(0.72 * strength),
        width: 1.0,
        radius: 5.0.into(),
    };
    base
}

pub(crate) fn ghost_tile(
    entry: &DirectoryEntry,
    tone: Tone,
    presence: Presence,
    tint: f32,
    width: f32,
    tile_height: f32,
    palette: a::Palette,
) -> Element<'static, Message> {
    let ty = palette.typography;
    let alpha = presence.alpha.clamp(0.0, 1.0);
    let (sink_y, label_slide_x) = if tone == Tone::Rename {
        (0.0, presence.offset.max(0.0) * 16.0)
    } else {
        (presence.offset.max(0.0) * 16.0, 0.0)
    };
    let tint_strength = tint.clamp(0.0, 1.0);
    let color = tone_color(tone, palette);
    let icon = Icon::for_entry(entry);
    let name = entry.name.to_string_lossy();

    let contents = column![
        icons::view(icon, 32.0, Some(palette.icon(icon)), alpha),
        crate::boxes::file_grid::tile_label(
            ty,
            name.as_ref(),
            width,
            palette.text.scale_alpha(alpha),
            label_slide_x,
        )
    ]
    .spacing(8)
    .align_x(iced::Center);

    container(contents)
        .padding(Padding {
            top: sink_y,
            right: 5.0,
            bottom: 0.0,
            left: 5.0,
        })
        .width(width)
        .height((tile_height - 6.0).max(1.0))
        .align_y(iced::Center)
        .clip(true)
        .style(move |_| container::Style {
            background: Some(color.scale_alpha(0.16 * tint_strength * alpha).into()),
            border: Border {
                color: color.scale_alpha(0.55 * tint_strength * alpha),
                width: 1.0,
                radius: 5.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn overlay_tint(base: Option<Color>, tint: Color, tint_alpha: f32) -> Color {
    let Some(base) = base else {
        return tint.scale_alpha(tint_alpha);
    };
    let inv = 1.0 - tint_alpha;
    let out_a = (tint_alpha + base.a * inv).clamp(0.0, 1.0);
    if out_a <= f32::EPSILON {
        return Color::TRANSPARENT;
    }
    Color {
        r: (tint.r * tint_alpha + base.r * base.a * inv) / out_a,
        g: (tint.g * tint_alpha + base.g * base.a * inv) / out_a,
        b: (tint.b * tint_alpha + base.b * base.a * inv) / out_a,
        a: out_a,
    }
}
