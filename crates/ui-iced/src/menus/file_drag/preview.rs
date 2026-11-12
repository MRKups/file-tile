//! Lifted intra-app drag preview (1-3 content-fitted item cards, 4+ collapsed group card) and drop targets.

use std::path::Path;

use iced::widget::{Stack, column, container, pin, row, space, text};
use iced::{Border, Color, Element, Fill, Padding, Shadow, Vector};

use crate::application::LoadState;
use crate::application::contracts::directory::{DirectoryEntry, DirectoryListing};
use crate::{
    Desktop, Message,
    boxes::file_list,
    runtime::thumbnails,
    window::{
        appearance as a,
        icons::{self, Icon},
    },
};

use super::{DropTarget, InternalDrag};

const MAX_INDIVIDUAL_ITEMS: usize = 3;

#[derive(Clone, Copy)]
struct OperationCue {
    badge: Option<(Icon, Color)>,
    active_target: bool,
}

#[derive(Clone, Copy)]
struct PreviewContext<'a> {
    desktop: &'a Desktop,
    drag: &'a InternalDrag,
    listing: Option<&'a DirectoryListing>,
    cue: OperationCue,
    lift: f32,
    palette: a::Palette,
}

pub(crate) fn view(desktop: &Desktop, palette: a::Palette) -> Element<'_, Message> {
    let internal = desktop
        .file_drag
        .internal
        .as_ref()
        .filter(|drag| drag.active);
    let target = internal
        .and_then(|drag| drag.target.as_ref())
        .or(desktop.file_drag.external_target.as_ref());
    if internal.is_none() && target.is_none() {
        return space().into();
    }
    let mut layers = Stack::new();
    if let Some(target) = target {
        layers = layers.push(target_highlight(
            desktop,
            palette,
            target,
            internal.is_some(),
        ));
    }
    if let Some(drag) = internal {
        layers = layers.push(drag_object(desktop, drag, palette));
    }
    container(layers.width(Fill).height(Fill))
        .width(Fill)
        .height(Fill)
        .clip(true)
        .into()
}

fn operation_cue(desktop: &Desktop, drag: &InternalDrag, palette: a::Palette) -> OperationCue {
    let active_target = drag.target.is_some();
    let badge = match &drag.target {
        Some(DropTarget::Launcher { .. }) => Some((Icon::Link, palette.tool_active_icon)),
        Some(DropTarget::SidebarPin { .. }) => Some((Icon::Pin, palette.tool_active_icon)),
        _ if desktop.modifiers.alt() => Some((Icon::Copy, palette.accent)),
        _ => None,
    };
    OperationCue {
        badge,
        active_target,
    }
}

fn drag_object<'a>(
    desktop: &'a Desktop,
    drag: &'a InternalDrag,
    palette: a::Palette,
) -> Element<'a, Message> {
    let cue = operation_cue(desktop, drag, palette);
    let lift = drag.lift_progress();
    let listing = desktop
        .application
        .load(drag.source_view)
        .and_then(LoadState::listing);

    if drag.paths.len() > MAX_INDIVIDUAL_ITEMS {
        return collapsed_group(drag, listing, cue, lift, palette);
    }

    let ctx = PreviewContext {
        desktop,
        drag,
        listing,
        cue,
        lift,
        palette,
    };
    let ty = palette.typography;
    let bar_h = ty.row(file_list::ROW_HEIGHT);
    let gap = 3.0 + (1.0 - lift) * 5.0;
    let slot = drag
        .layout
        .pressed_slot
        .min(drag.paths.len().saturating_sub(1));

    let mut cards = column![].spacing(gap);
    for (idx, path) in drag.paths.iter().enumerate() {
        cards = cards.push(compact_item_card(ctx, path, idx == slot));
    }

    let anchor_x = drag.layout.grab_offset.x.clamp(ty.row(14.0), ty.row(36.0));
    let ratio_y =
        (drag.layout.grab_offset.y / drag.layout.item_size.height.max(1.0)).clamp(0.2, 0.8);
    let anchor_y = slot as f32 * (bar_h + gap) + bar_h * ratio_y + 2.0 * lift;
    pin(cards)
        .x(drag.pointer.x - anchor_x)
        .y(drag.pointer.y - anchor_y)
        .into()
}

fn compact_item_card<'a>(
    ctx: PreviewContext<'a>,
    path: &'a Path,
    show_cue: bool,
) -> Element<'a, Message> {
    let palette = ctx.palette;
    let ty = palette.typography;
    let entry = ctx.listing.and_then(|list| list.entry(path)).or_else(|| {
        ctx.desktop
            .column_previews
            .entry(ctx.drag.source_view, path)
    });
    let is_symlink = entry.is_some_and(|item| item.symlink);
    let badge = if show_cue { ctx.cue.badge } else { None };
    let name = display_name(entry, path);

    let mut name_budget = 234.0;
    if is_symlink {
        name_budget -= 28.0;
    }
    if badge.is_some() {
        name_budget -= 21.0;
    }
    let name_box = container(
        ty.text(name)
            .color(palette.text)
            .wrapping(text::Wrapping::None),
    )
    .max_width(ty.row(name_budget))
    .clip(true);

    let mut content = row![item_artwork(ctx, entry, path), name_box]
        .spacing(8)
        .align_y(iced::Center);

    if is_symlink {
        content = content.push(ty.caption("link").color(palette.muted));
    }
    if let Some((badge_icon, badge_color)) = badge {
        content = content.push(icons::view(
            badge_icon,
            ty.icon(13.0),
            Some(badge_color),
            0.95,
        ));
    }

    container(content)
        .height(ty.row(file_list::ROW_HEIGHT))
        .max_width(ty.row(280.0))
        .padding([0, 10])
        .align_y(iced::Center)
        .clip(true)
        .style(move |_| lifted_panel_style(palette, ctx.cue.active_target, ctx.lift, 6.0))
        .into()
}

fn item_artwork(
    ctx: PreviewContext<'_>,
    entry: Option<&DirectoryEntry>,
    path: &Path,
) -> Element<'static, Message> {
    let palette = ctx.palette;
    let ty = palette.typography;
    let revision = ctx
        .desktop
        .views
        .get(&ctx.drag.source_view)
        .map_or(0, |state| state.listing_revision);
    if let Some(thumb) = thumbnails::image_with_opacity(
        Some(&ctx.desktop.thumbnails.values),
        ctx.drag.source_view,
        revision,
        path,
        ty.icon(18.0),
        1.0,
    ) {
        return thumb;
    }
    let icon = entry.map_or_else(|| icon_for_path(path), Icon::for_entry);
    let icon_size = ty.icon(16.0);
    container(icons::view(icon, icon_size, Some(palette.icon(icon)), 1.0))
        .padding(Padding {
            bottom: icon_size / 12.0,
            ..Padding::ZERO
        })
        .into()
}

fn collapsed_group<'a>(
    drag: &'a InternalDrag,
    listing: Option<&'a DirectoryListing>,
    cue: OperationCue,
    lift: f32,
    palette: a::Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    let rep_icons = representative_icons(drag, listing);
    let icon_count = rep_icons.len();
    let label = format!("{} items", drag.paths.len());
    let icons_w = icon_count as f32 * ty.icon(16.0) + icon_count.saturating_sub(1) as f32 * 4.0;
    let label_w = ty.row(label.len() as f32 * 6.8 + 4.0);
    let badge_w = if cue.badge.is_some() {
        8.0 + ty.icon(13.0)
    } else {
        0.0
    };
    let card_w = (20.0 + icons_w + 8.0 + label_w + badge_w).max(ty.row(96.0));
    let card_h = ty.row(38.0);
    let spread = 5.0 * lift;

    let mut icons_row = row![].spacing(4).align_y(iced::Center);
    for icon in rep_icons {
        icons_row = icons_row.push(icons::view(
            icon,
            ty.icon(16.0),
            Some(palette.icon(icon)),
            1.0,
        ));
    }

    let mut front_content = row![
        icons_row,
        ty.text(label)
            .color(palette.text)
            .wrapping(text::Wrapping::None)
            .width(Fill)
    ]
    .spacing(8)
    .align_y(iced::Center);

    if let Some((badge_icon, badge_color)) = cue.badge {
        front_content = front_content.push(icons::view(
            badge_icon,
            ty.icon(13.0),
            Some(badge_color),
            0.95,
        ));
    }

    let back_two = backing_card(card_w, card_h, palette, 0.52 * lift);
    let back_one = backing_card(card_w, card_h, palette, 0.76 * lift);
    let front = container(front_content)
        .width(card_w)
        .height(card_h)
        .padding([0, 10])
        .align_y(iced::Center)
        .clip(true)
        .style(move |_| lifted_panel_style(palette, cue.active_target, lift, 6.0));

    let stack = Stack::new()
        .push(pin(back_two).x(spread).y(spread))
        .push(pin(back_one).x(spread * 0.5).y(spread * 0.5))
        .push(pin(front).x(0.0).y(0.0))
        .width(card_w + spread + 2.0)
        .height(card_h + spread + 2.0);

    let ratio_x =
        (drag.layout.grab_offset.x / drag.layout.item_size.width.max(1.0)).clamp(0.15, 0.85);
    let anchor_x = card_w * ratio_x;
    let anchor_y = card_h * 0.5 + 2.0 * lift;
    pin(stack)
        .x(drag.pointer.x - anchor_x)
        .y(drag.pointer.y - anchor_y)
        .into()
}

fn backing_card(
    width: f32,
    height: f32,
    palette: a::Palette,
    alpha: f32,
) -> Element<'static, Message> {
    container(space())
        .width(width)
        .height(height)
        .style(move |_| container::Style {
            background: Some(
                Color {
                    a: 0.94 * alpha,
                    ..palette.contrast_background
                }
                .into(),
            ),
            border: Border {
                color: palette.selected_border.scale_alpha(0.75 * alpha),
                width: 1.0,
                radius: 6.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn representative_icons(drag: &InternalDrag, listing: Option<&DirectoryListing>) -> Vec<Icon> {
    let mut icons = Vec::with_capacity(3);
    for path in &drag.paths {
        let icon = listing
            .and_then(|list| list.entry(path))
            .map_or_else(|| icon_for_path(path), Icon::for_entry);
        if !icons.contains(&icon) {
            icons.push(icon);
            if icons.len() == 3 {
                break;
            }
        }
    }
    if icons.is_empty() {
        icons.push(Icon::Document);
    }
    icons
}

fn lifted_panel_style(
    palette: a::Palette,
    active_target: bool,
    lift: f32,
    radius: f32,
) -> container::Style {
    let bg = blend_selected_surface(palette.contrast_background, palette.row_selected, 0.95);
    container::Style {
        background: Some(bg.into()),
        text_color: Some(palette.text),
        border: Border {
            color: if active_target {
                palette.tool_active_icon
            } else {
                palette.selected_border
            },
            width: 1.0,
            radius: radius.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.18 * lift),
            offset: Vector::new(0.0, 5.0 * lift),
            blur_radius: 14.0 * lift,
        },
        ..container::Style::default()
    }
}

fn blend_selected_surface(base: Color, tint: Color, alpha: f32) -> Color {
    let t = (tint.a * 0.55).clamp(0.0, 0.35);
    Color {
        r: base.r * (1.0 - t) + tint.r * t,
        g: base.g * (1.0 - t) + tint.g * t,
        b: base.b * (1.0 - t) + tint.b * t,
        a: alpha,
    }
}

fn display_name(entry: Option<&DirectoryEntry>, path: &Path) -> String {
    entry.map_or_else(
        || {
            path.file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .into_owned()
        },
        |item| item.name.to_string_lossy().into_owned(),
    )
}

fn icon_for_path(path: &Path) -> Icon {
    let is_image = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            ["png", "jpg", "jpeg", "gif", "webp", "svg", "heic"]
                .iter()
                .any(|img| ext.eq_ignore_ascii_case(img))
        });
    if is_image {
        Icon::Image
    } else {
        Icon::Document
    }
}

fn target_highlight<'a>(
    desktop: &'a Desktop,
    palette: a::Palette,
    target: &'a DropTarget,
    is_internal: bool,
) -> Element<'a, Message> {
    match target {
        DropTarget::SidebarPin { marker, .. } => {
            crate::boxes::sidebar::insertion_marker(*marker, palette)
        }
        DropTarget::SidebarRow { rect, label, .. } => {
            sidebar_row_highlight(desktop, palette, *rect, label, is_internal)
        }
        DropTarget::FolderBox { .. } | DropTarget::Launcher { .. } => {
            let rect = target.rect();
            let label = target_label(desktop, target, is_internal);
            let indicator = container(
                palette
                    .typography
                    .small(label)
                    .wrapping(text::Wrapping::None),
            )
            .padding([6, 12])
            .max_width((rect.width - 24.0).max(48.0))
            .clip(true)
            .style(move |_| container::Style {
                background: Some(
                    Color {
                        a: 0.96,
                        ..palette.contrast_background
                    }
                    .into(),
                ),
                text_color: Some(palette.text),
                border: Border {
                    color: palette.tool_active_icon,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..container::Style::default()
            });
            let highlight = container(indicator)
                .width(rect.width)
                .height(rect.height)
                .center_x(rect.width)
                .center_y(rect.height)
                .style(move |_| container::Style {
                    background: Some(
                        Color {
                            a: 0.14,
                            ..palette.tool_active_icon
                        }
                        .into(),
                    ),
                    border: Border {
                        color: palette.tool_active_icon,
                        width: 2.0,
                        radius: 10.0.into(),
                    },
                    ..container::Style::default()
                });
            pin(highlight).x(rect.x).y(rect.y).into()
        }
    }
}

fn sidebar_row_highlight<'a>(
    desktop: &'a Desktop,
    palette: a::Palette,
    rect: iced::Rectangle,
    label: &'a str,
    is_internal: bool,
) -> Element<'a, Message> {
    let verb = transfer_verb(desktop, is_internal);
    let caption = format!("{verb} {label}");
    let row_fill = container(
        palette
            .typography
            .caption(caption)
            .color(palette.text)
            .wrapping(text::Wrapping::None),
    )
    .padding([0, 10])
    .width(rect.width)
    .height(rect.height)
    .align_y(iced::Center)
    .clip(true)
    .style(move |_| container::Style {
        background: Some(
            Color {
                a: 0.92,
                ..palette.contrast_background
            }
            .into(),
        ),
        border: Border {
            color: palette.tool_active_icon,
            width: 1.5,
            radius: 4.0.into(),
        },
        ..container::Style::default()
    });
    pin(row_fill).x(rect.x).y(rect.y).into()
}

fn transfer_verb(desktop: &Desktop, is_internal: bool) -> &'static str {
    if !is_internal {
        "Drop into"
    } else if desktop.modifiers.alt() {
        "Copy to"
    } else {
        "Move to"
    }
}

fn target_label(desktop: &Desktop, target: &DropTarget, is_internal: bool) -> String {
    match target {
        DropTarget::Launcher { .. } => "Set shortcut".into(),
        DropTarget::SidebarPin { .. } => "Pin to sidebar".into(),
        DropTarget::SidebarRow { label, .. } => {
            format!("{} {label}", transfer_verb(desktop, is_internal))
        }
        DropTarget::FolderBox { view, .. } => {
            let folder = desktop
                .application
                .workspace()
                .view(*view)
                .and_then(|item| item.navigation.current().file_name())
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .unwrap_or("/");
            let verb = transfer_verb(desktop, is_internal);
            format!("{verb} {folder}")
        }
    }
}
