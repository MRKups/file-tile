use std::ops::Range;
use std::path::Path;

use iced::advanced::widget::{
    self, Id, Operation,
    operation::{Outcome, Scrollable},
};
use iced::widget::{container, keyed_column, space};
use iced::{Element, Fill};
use iced::{Rectangle, Task, Vector};

use crate::application::LoadState;
use crate::domain::ViewId;

use crate::{
    Message,
    boxes::{entry_motion, file_entry},
    window::appearance as a,
};

pub(crate) mod retained_row;

pub const ROW_HEIGHT: f32 = 34.0;

#[derive(Debug, Clone, Copy, Default)]
pub struct ScrollWindow {
    pub offset: f32,
    pub height: f32,
}

pub fn scroll_id(view: ViewId) -> String {
    format!("files-{}", view.0)
}

pub(crate) struct ScrollSnapshot {
    id: Id,
    window: Option<ScrollWindow>,
}
impl ScrollSnapshot {
    pub fn new(view: ViewId) -> Self {
        Self {
            id: scroll_id(view).into(),
            window: None,
        }
    }
}
impl Operation<ScrollWindow> for ScrollSnapshot {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<ScrollWindow>)) {
        operate(self);
    }
    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        _content: Rectangle,
        translation: Vector,
        _state: &mut dyn Scrollable,
    ) {
        if id == Some(&self.id) {
            self.window = Some(ScrollWindow {
                offset: translation.y,
                height: bounds.height,
            });
        }
    }
    fn finish(&self) -> Outcome<ScrollWindow> {
        self.window.map_or(Outcome::None, Outcome::Some)
    }
}

pub(crate) fn observe_scroll(view: ViewId, generation: u64) -> Task<Message> {
    widget::operate(ScrollSnapshot::new(view)).map(move |window| Message::ListingScrollObserved {
        view,
        generation,
        window,
    })
}

#[derive(Clone, Copy)]
pub struct Props<'a> {
    pub order: Option<&'a [usize]>,
    pub order_revision: u64,
    pub usage: Option<
        &'a Result<
            crate::application::files::folder_usage::Estimate,
            crate::application::files::folder_usage::UsageError,
        >,
    >,
    pub folder_usages: Option<&'a crate::application::files::folder_usage::UsageBook>,
    pub palette: a::Palette,
    pub view: ViewId,
    pub load: Option<&'a LoadState>,
    pub selected: Option<&'a Path>,
    pub selection: Option<&'a std::collections::HashSet<std::path::PathBuf>>,
    pub thumbnails: Option<&'a crate::runtime::thumbnails::Values>,
    pub hovered: Option<&'a Path>,
    pub interactive: bool,
    pub window: ScrollWindow,
    pub revision: u64,
    pub show_hidden: bool,
    pub scope: &'a crate::domain::Scope,
    pub metadata: Option<&'a crate::runtime::metadata::Values>,
    pub settings: crate::domain::BoxSettings,
    pub width: f32,
    pub clipped_left: f32,
    pub clipped_width: f32,
    pub metadata_paths: Option<&'a [std::path::PathBuf]>,
    pub motion: Option<&'a entry_motion::ViewMotion>,
    pub dragging: Option<&'a std::collections::HashSet<std::path::PathBuf>>,
    pub column_preview: Option<&'a crate::boxes::file_columns::ViewPreview>,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum RowKey {
    Before,
    Entry(u64, u64, bool, usize),
    Ghost(u64),
    Grid(u64, u64, bool, usize, usize, usize),
    GridMotion(u64, u64, bool, usize, usize, usize),
    After,
    Empty,
}

impl<'a> Props<'a> {
    pub(crate) fn entry(
        self,
        listing: &'a crate::application::contracts::directory::DirectoryListing,
        index: usize,
    ) -> Option<&'a crate::application::contracts::directory::DirectoryEntry> {
        if let Some(order) = self.order {
            listing.entries().get(*order.get(index)?)
        } else {
            listing.display_entry(index, self.show_hidden)
        }
    }
}

pub fn view(props: Props<'_>) -> Element<'_, Message> {
    let scroller = a::scroll_area(content(props))
        .style(props.palette.scrollable())
        .direction(iced::widget::scrollable::Direction::Vertical(a::scrollbar()))
        .id(scroll_id(props.view))
        .width(Fill)
        .height(Fill)
        .on_scroll(move |viewport| {
            Message::FileViewportScrolled(
                props.view,
                viewport.absolute_offset(),
                viewport.bounds().height,
            )
        })
        .into();
    crate::menus::folder_menu::target(props.view, props.interactive, scroller)
}

pub fn content(props: Props<'_>) -> Element<'_, Message> {
    if props.settings.file_view == crate::domain::FileView::Grid {
        return crate::boxes::file_grid::content(props);
    }
    let Some(listing) = props.load.and_then(LoadState::listing) else {
        return empty(
            "No contents loaded.",
            props.view,
            props.interactive,
            props.palette,
        );
    };
    let count = listing.display_len(props.show_hidden);
    let has_deletions = props.motion.is_some_and(|motion| motion.has_deletions());
    if count == 0 && !has_deletions {
        return empty_listing(listing, props);
    }
    let row_height = props.palette.typography.row(ROW_HEIGHT);
    if let Some(motion) = props.motion.filter(|motion| motion.has_layout_motion()) {
        return content_with_motion(props, listing, motion, row_height);
    }
    let range = range_with_height(count, props.window, row_height);
    let demand = metadata_demand(
        props,
        listing,
        unbuffered_range(count, props.window, row_height),
    );
    let mut children = Vec::with_capacity(range.len() + 2);
    children.push((
        RowKey::Before,
        space().height(range.start as f32 * row_height).into(),
    ));
    for index in range.clone() {
        if let Some(entry) = props.entry(listing, index) {
            children.push((
                RowKey::Entry(
                    props.revision,
                    props.order_revision,
                    props.show_hidden,
                    index,
                ),
                file_entry::view(entry, props, !entry_allowed(props, listing, entry)),
            ));
        }
    }
    children.push((
        RowKey::After,
        space()
            .height((count - range.end) as f32 * row_height)
            .into(),
    ));
    keyed_rows(props, children, demand)
}

fn content_with_motion<'a>(
    props: Props<'a>,
    listing: &'a crate::application::contracts::directory::DirectoryListing,
    motion: &'a entry_motion::ViewMotion,
    row_height: f32,
) -> Element<'a, Message> {
    let slots = motion.project_slots(listing, props.order, props.show_hidden);
    if slots.is_empty() {
        return empty_listing(listing, props);
    }
    let unbuffered = unbuffered_slot_range(&slots, props.window, row_height);
    let range = unbuffered.start.saturating_sub(4).min(slots.len())
        ..unbuffered.end.saturating_add(4).min(slots.len());
    let live_indices = slots[unbuffered].iter().filter_map(|slot| match slot {
        entry_motion::Slot::Live { index, .. } => Some(*index),
        entry_motion::Slot::Ghost { .. } => None,
    });
    let demand = metadata_demand(props, listing, live_indices);
    let before_height: f32 = slots[..range.start]
        .iter()
        .map(|slot| slot.height(row_height))
        .sum();
    let after_height: f32 = slots[range.end..]
        .iter()
        .map(|slot| slot.height(row_height))
        .sum();
    let mut children = Vec::with_capacity(range.len() + 2);
    children.push((RowKey::Before, space().height(before_height).into()));
    for slot in &slots[range] {
        match *slot {
            entry_motion::Slot::Live { index, entry, .. } => {
                children.push((
                    RowKey::Entry(
                        props.revision,
                        props.order_revision,
                        props.show_hidden,
                        index,
                    ),
                    file_entry::view(entry, props, !entry_allowed(props, listing, entry)),
                ));
            }
            entry_motion::Slot::Ghost {
                id,
                entry,
                tone,
                presence,
                tint,
            } => {
                children.push((
                    RowKey::Ghost(id),
                    entry_motion::ghost_row(entry, tone, presence, tint, props.palette),
                ));
            }
        }
    }
    children.push((RowKey::After, space().height(after_height).into()));
    keyed_rows(props, children, demand)
}

pub(crate) fn empty_listing(
    listing: &crate::application::contracts::directory::DirectoryListing,
    props: Props<'_>,
) -> Element<'static, Message> {
    empty(
        if listing.issues.is_empty() {
            "This folder is empty."
        } else {
            "No usable entries. Listing is incomplete."
        },
        props.view,
        props.interactive,
        props.palette,
    )
}

pub(crate) fn keyed_rows<'a>(
    props: Props<'a>,
    children: Vec<(RowKey, Element<'a, Message>)>,
    demand: Option<retained_row::Demand>,
) -> Element<'a, Message> {
    keyed_column(children.into_iter().map(|(key, child)| {
        (
            key,
            retained_row::with_demand(
                props.view,
                key,
                props.interactive,
                child,
                if key == RowKey::Before {
                    demand.clone()
                } else {
                    None
                },
            ),
        )
    }))
    .padding([0, 9])
    .width(Fill)
    .into()
}

pub(crate) fn metadata_demand(
    props: Props<'_>,
    listing: &crate::application::contracts::directory::DirectoryListing,
    range: impl Iterator<Item = usize>,
) -> Option<retained_row::Demand> {
    let limit = crate::application::files::metadata::CACHE_LIMIT - 1;
    let mut paths: Vec<_> = if props.clipped_width > 0.0 {
        range
            .take(limit)
            .filter_map(|index| props.entry(listing, index))
            .map(|entry| entry.path.clone())
            .collect()
    } else {
        Vec::new()
    };
    if let Some(preview) = props.column_preview {
        for path in preview.visible_demand_paths(
            props.window.height,
            props.palette.typography.row(ROW_HEIGHT),
        ) {
            if paths.len() >= limit {
                break;
            }
            if !paths.iter().any(|candidate| candidate == path) {
                paths.push(path.to_path_buf());
            }
        }
    }
    if let Some(path) = props.selected
        && !paths.iter().any(|candidate| candidate == path)
    {
        paths.push(path.to_path_buf());
    }
    (!paths.is_empty()).then_some(retained_row::Demand {
        revision: props.revision,
        paths,
        settings: props.settings,
    })
}

pub(crate) fn entry_allowed(
    props: Props<'_>,
    listing: &crate::application::contracts::directory::DirectoryListing,
    entry: &crate::application::contracts::directory::DirectoryEntry,
) -> bool {
    entry.kind != crate::application::contracts::directory::EntryKind::Directory
        || props.scope.kind() == crate::domain::ScopeKind::Free
        || listing
            .entry_target(entry)
            .is_some_and(|path| props.scope.allows(path))
}

pub(crate) fn empty(
    message: &'static str,
    view: ViewId,
    interactive: bool,
    palette: a::Palette,
) -> Element<'static, Message> {
    keyed_column([(
        RowKey::Empty,
        retained_row::view(
            view,
            RowKey::Empty,
            interactive,
            container(palette.typography.small(message).color(palette.muted))
                .padding([20, 17])
                .width(Fill)
                .into(),
        ),
    )])
    .width(Fill)
    .into()
}

pub(crate) fn range_with_height(
    count: usize,
    window: ScrollWindow,
    row_height: f32,
) -> Range<usize> {
    if window.height <= 0.0 {
        return 0..0;
    }
    let range = unbuffered_range(count, window, row_height);
    range.start.saturating_sub(4).min(count)..range.end.saturating_add(4).min(count)
}
pub(crate) fn unbuffered_range(
    count: usize,
    window: ScrollWindow,
    row_height: f32,
) -> Range<usize> {
    if window.height <= 0.0 {
        return 0..0;
    }
    (window.offset.max(0.0) / row_height).floor() as usize
        ..((window.offset.max(0.0) + window.height) / row_height)
            .ceil()
            .min(count as f32) as usize
}

fn unbuffered_slot_range(
    slots: &[entry_motion::Slot<'_>],
    window: ScrollWindow,
    row_height: f32,
) -> Range<usize> {
    if window.height <= 0.0 || slots.is_empty() {
        return 0..0;
    }
    let total_height: f32 = slots.iter().map(|slot| slot.height(row_height)).sum();
    let top = window
        .offset
        .clamp(0.0, (total_height - window.height).max(0.0));
    let bottom = top + window.height;
    let mut cursor = 0.0f32;
    let mut start = None;
    let mut end = slots.len();
    for (idx, slot) in slots.iter().enumerate() {
        let next = cursor + slot.height(row_height);
        if start.is_none() && next > top {
            start = Some(idx);
        }
        if cursor >= bottom {
            end = idx;
            break;
        }
        cursor = next;
    }
    let first = start.unwrap_or(slots.len());
    first.min(end)..end
}
