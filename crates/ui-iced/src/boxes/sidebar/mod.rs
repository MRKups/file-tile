//! Per-box two-section sidebar (`Pinned Folders` + auto-detected `Locations`).

pub(crate) mod drag;

use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::widget::{button, column, container, rule, scrollable, space, stack, text, tooltip};
use iced::{Border, Center, Color, Element, Fill, Padding, Point, Rectangle, keyboard};

use crate::application::{Action, Navigate};
use crate::domain::{BoxId, BoxSettings, FolderBox, Mode};
use crate::platform::volumes::{DetectedLocation, LocationKind};
use crate::{
    Desktop, Message,
    boxes::navigation_header,
    canvas::geometry,
    menus::context_menu,
    widgets::focus_control,
    window::{
        appearance as a,
        icons::{self, Icon},
    },
};

pub const ROW_HEIGHT: f32 = 30.0;
pub const SECTION_HEADER_HEIGHT: f32 = 24.0;
pub const EMPTY_SLOT_HEIGHT: f32 = 32.0;
pub const SECTION_DIVIDER_HEIGHT: f32 = 13.0;
pub const END_INSET: f32 = 1.0;
const CORNER_RADIUS: f32 = 9.0;

pub fn left_rail_width(item: &FolderBox, settings: BoxSettings, box_width: f32) -> f32 {
    if item.has_visible_tabs() || settings.sidebar {
        (3.0 * geometry::PITCH - geometry::GAP).min(box_width * 0.65)
    } else {
        0.0
    }
}

pub fn rail_width(item: &FolderBox, box_width: f32) -> f32 {
    left_rail_width(item, item.settings, box_width)
}

pub fn scroll_id(id: BoxId) -> String {
    format!("directory-sidebar-{}", id.0)
}

pub fn pin_row_id(id: BoxId, index: usize) -> String {
    format!("sidebar-pin-{}-{index}", id.0)
}

pub fn location_row_id(id: BoxId, index: usize) -> String {
    format!("sidebar-location-{}-{index}", id.0)
}

pub fn content_height(
    pinned_count: usize,
    location_count: usize,
    ty: crate::window::typography::Style,
) -> f32 {
    let top = ty.row(SECTION_HEADER_HEIGHT)
        + if pinned_count == 0 {
            ty.row(EMPTY_SLOT_HEIGHT)
        } else {
            pinned_count as f32 * ty.row(ROW_HEIGHT)
        };
    if location_count == 0 {
        top
    } else {
        top + ty.row(SECTION_DIVIDER_HEIGHT)
            + ty.row(SECTION_HEADER_HEIGHT)
            + location_count as f32 * ty.row(ROW_HEIGHT)
    }
}

pub fn offset(
    pinned_count: usize,
    location_count: usize,
    box_height: f32,
    stored: f32,
    ty: crate::window::typography::Style,
) -> f32 {
    let visible = (box_height - 2.0 * END_INSET).max(0.0);
    let maximum = (content_height(pinned_count, location_count, ty) - visible).max(0.0);
    stored.clamp(0.0, maximum).round()
}

pub fn folder_display_name(path: &Path, home: &Path) -> String {
    if path == home {
        "Home".to_owned()
    } else if path == Path::new("/") {
        "/".to_owned()
    } else {
        path.file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned()
    }
}

pub fn folder_icon(path: &Path, home: &Path) -> Icon {
    if path == home {
        Icon::Home
    } else {
        Icon::Folder
    }
}

pub fn location_icon(kind: &LocationKind) -> Icon {
    match kind {
        LocationKind::Cloud(_) => Icon::Cloud,
        LocationKind::ExternalDrive => Icon::Drive,
        LocationKind::NetworkDrive { .. } => Icon::Network,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum HitZone {
    PinInsert {
        index: usize,
        marker: Rectangle,
    },
    Row {
        directory: PathBuf,
        label: String,
        rect: Rectangle,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RowAccess {
    scope_allowed: bool,
    safety_enabled: bool,
    os_granted: bool,
}

impl RowAccess {
    const fn can_enter(self) -> bool {
        self.scope_allowed && self.safety_enabled
    }

    const fn show_lock(self) -> bool {
        !self.safety_enabled || !self.os_granted
    }
}

pub struct Props<'a> {
    pub palette: a::Palette,
    pub item: &'a FolderBox,
    pub home: &'a Path,
    pub pinned: &'a [PathBuf],
    pub ungranted_pins: &'a std::collections::BTreeSet<PathBuf>,
    pub locations: &'a [DetectedLocation],
    pub safety_policy: crate::domain::SafetyPolicy,
    pub mode: Mode,
    pub interactive: bool,
    pub width: f32,
    pub dragged_pin: Option<usize>,
}

pub fn view(props: Props<'_>) -> Element<'_, Message> {
    if !props.item.has_sidebar() {
        return space().into();
    }
    let palette = props.palette;
    let mut content = column![section_header("FOLDERS", palette)].spacing(0);
    if props.pinned.is_empty() {
        content = content.push(empty_pinned_slot(palette));
    } else {
        for (index, path) in props.pinned.iter().enumerate() {
            content = content.push(pinned_row(&props, index, path));
        }
    }
    if !props.locations.is_empty() {
        content = content.push(section_divider(palette));
        content = content.push(section_header("LOCATIONS", palette));
        for (index, loc) in props.locations.iter().enumerate() {
            content = content.push(location_row(&props, index, loc));
        }
    }
    let scroller = a::scroll_area(content.width(Fill))
        .style(palette.scrollable())
        .id(scroll_id(props.item.id))
        .width(Fill)
        .height(Fill)
        .direction(scrollable::Direction::Vertical(a::scrollbar()))
        .on_scroll(move |viewport| {
            Message::SidebarScrolled(props.item.id, viewport.absolute_offset().y)
        });
    let scroller = context_menu::scrollbars(a::SCROLLBAR_WIDTH, 0.0, scroller.into());
    let rail = container(scroller)
        .padding(Padding {
            top: END_INSET,
            bottom: END_INSET,
            left: 1.0,
            right: 0.0,
        })
        .width((props.width - 1.0).max(0.0))
        .height(Fill);
    iced::widget::row![rail, rule::vertical(1).style(move |_| palette.rule())]
        .width(props.width)
        .height(Fill)
        .into()
}

fn section_header(title: &'static str, palette: a::Palette) -> Element<'static, Message> {
    let ty = palette.typography;
    container(
        ty.caption(title)
            .font(ty.heading_font())
            .color(palette.muted)
            .wrapping(text::Wrapping::None),
    )
    .padding([0, 12])
    .width(Fill)
    .height(ty.row(SECTION_HEADER_HEIGHT))
    .align_y(Center)
    .clip(true)
    .into()
}

fn empty_pinned_slot(palette: a::Palette) -> Element<'static, Message> {
    let ty = palette.typography;
    container(
        ty.caption("Drop folders to pin")
            .color(palette.muted)
            .wrapping(text::Wrapping::None),
    )
    .padding([0, 12])
    .width(Fill)
    .height(ty.row(EMPTY_SLOT_HEIGHT))
    .align_y(Center)
    .clip(true)
    .into()
}

fn section_divider(palette: a::Palette) -> Element<'static, Message> {
    let ty = palette.typography;
    container(rule::horizontal(1).style(move |_| palette.rule()))
        .padding([0, 12])
        .width(Fill)
        .height(ty.row(SECTION_DIVIDER_HEIGHT))
        .align_y(Center)
        .into()
}

fn pinned_row<'a>(props: &Props<'a>, index: usize, path: &'a Path) -> Element<'a, Message> {
    let palette = props.palette;
    let id = props.item.id;
    let active_nav = &props.item.standalone.navigation;
    let selected = active_nav.current() == path;
    let access = RowAccess {
        scope_allowed: active_nav.scope().allows(path),
        safety_enabled: props.safety_policy.check_read_directory(path).is_ok(),
        os_granted: !props.ungranted_pins.contains(path),
    };
    let enabled = props.interactive && props.mode != Mode::Layout;
    let label = folder_display_name(path, props.home);
    let icon = folder_icon(path, props.home);
    let view = props.item.standalone.view;
    let focus_msg = (enabled && access.can_enter()).then_some(Message::FocusTab(view));
    let inner = row_button(label, icon, selected, access, enabled, focus_msg, palette);
    let activation = enabled.then(|| {
        (
            view,
            Message::SidebarPinPressed(id, index, path.to_path_buf()),
        )
    });
    let menu_path = path.to_path_buf();
    let context: Option<Box<dyn Fn(Point) -> Message>> = enabled.then(|| {
        Box::new(move |point| Message::OpenSidebarMenu(id, menu_path.clone(), Some(point))) as _
    });
    let row_el = context_menu::press(activation, context, inner);
    let row_el = if enabled {
        let keys = vec![(
            keyboard::key::Named::F10,
            keyboard::Modifiers::SHIFT,
            Message::OpenSidebarMenu(id, path.to_path_buf(), None),
        )];
        focus_control::themed_highlight_keys_radius(
            pin_row_id(id, index),
            if access.can_enter() {
                Message::SidebarLocationActivated(id, path.to_path_buf())
            } else {
                Message::FocusTab(view)
            },
            row_el,
            keys,
            iced::border::Radius::default(),
            palette,
        )
    } else {
        row_el
    };
    let row_el: Element<'a, Message> = if props.dragged_pin == Some(index) {
        stack![
            row_el,
            container(space())
                .width(Fill)
                .height(Fill)
                .style(move |_| container::Style {
                    background: Some(
                        Color {
                            a: 0.65,
                            ..palette.editing_overlay
                        }
                        .into()
                    ),
                    ..container::Style::default()
                })
        ]
        .into()
    } else {
        row_el
    };
    with_row_tooltip(row_el, path, props.home, access, palette)
}

fn location_row<'a>(
    props: &Props<'a>,
    index: usize,
    loc: &'a DetectedLocation,
) -> Element<'a, Message> {
    let palette = props.palette;
    let id = props.item.id;
    let active_nav = &props.item.standalone.navigation;
    let selected = active_nav.current() == loc.path.as_path();
    let access = RowAccess {
        scope_allowed: active_nav.scope().allows(&loc.path),
        safety_enabled: props.safety_policy.check_read_directory(&loc.path).is_ok(),
        os_granted: loc.accessible,
    };
    let enabled = props.interactive && props.mode != Mode::Layout && access.can_enter();
    let icon = location_icon(&loc.kind);
    let view = props.item.standalone.view;
    let focus_msg = enabled.then_some(Message::FocusTab(view));
    let inner = row_button(
        loc.label.clone(),
        icon,
        selected,
        access,
        props.interactive && props.mode != Mode::Layout,
        focus_msg,
        palette,
    );
    let activation = enabled.then(|| {
        (
            view,
            Message::SidebarLocationActivated(id, loc.path.clone()),
        )
    });
    let inner = context_menu::press(activation, None, inner);
    let row_el = if enabled {
        focus_control::themed_highlight_keys_radius(
            location_row_id(id, index),
            Message::SidebarLocationActivated(id, loc.path.clone()),
            inner,
            Vec::new(),
            iced::border::Radius {
                bottom_left: if index + 1 == props.locations.len() {
                    CORNER_RADIUS
                } else {
                    0.0
                },
                ..iced::border::Radius::default()
            },
            palette,
        )
    } else {
        inner
    };
    with_row_tooltip(row_el, &loc.path, props.home, access, palette)
}

fn row_button(
    label: String,
    icon: Icon,
    selected: bool,
    access: RowAccess,
    interactive: bool,
    on_press: Option<Message>,
    palette: a::Palette,
) -> Element<'static, Message> {
    let ty = palette.typography;
    let allowed = access.can_enter();
    let alpha = if allowed { 1.0 } else { 0.45 };
    let text_color = if allowed {
        palette.tab_text
    } else {
        palette.muted
    };
    let mut row_items = iced::widget::row![
        icons::view(icon, ty.icon(15.0), Some(palette.icon(icon)), alpha),
        container(
            ty.text(label)
                .color(text_color)
                .wrapping(text::Wrapping::None)
        )
        .width(Fill)
        .clip(true)
    ]
    .spacing(8)
    .align_y(Center);
    if access.show_lock() {
        row_items = row_items.push(icons::view(
            Icon::Lock,
            ty.icon(12.5),
            Some(palette.muted),
            if allowed { 0.85 } else { 0.55 },
        ));
    }
    let content = container(row_items)
        .padding([0, 12])
        .width(Fill)
        .height(ty.row(ROW_HEIGHT))
        .align_y(Center)
        .clip(true);
    button(content)
        .padding(0)
        .width(Fill)
        .height(ty.row(ROW_HEIGHT))
        .on_press_maybe(on_press)
        .style(move |_, status| button::Style {
            text_color,
            background: Some(
                if selected {
                    palette.row_selected
                } else if interactive
                    && allowed
                    && matches!(status, button::Status::Hovered | button::Status::Pressed)
                {
                    palette.row_hover
                } else {
                    Color::TRANSPARENT
                }
                .into(),
            ),
            border: Border {
                radius: 4.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        })
        .into()
}

fn with_row_tooltip<'a>(
    content: Element<'a, Message>,
    path: &Path,
    home: &Path,
    access: RowAccess,
    palette: a::Palette,
) -> Element<'a, Message> {
    let mut tip = navigation_header::display_path(path, home);
    if !access.scope_allowed {
        tip.push_str(" (outside navigation scope)");
    } else if !access.safety_enabled {
        tip.push_str(" (turned off in Access and Safety)");
    } else if !access.os_granted {
        tip.push_str(" (macOS access not granted)");
    }
    tooltip(
        content,
        palette.typography.small(tip),
        tooltip::Position::Right,
    )
    .delay(Duration::from_millis(650))
    .style(move |_| palette.tooltip())
    .into()
}

pub(crate) fn insertion_marker(rect: Rectangle, palette: a::Palette) -> Element<'static, Message> {
    let line = container(space())
        .width(rect.width)
        .height(4)
        .style(move |_| container::Style {
            background: Some(palette.tool_active_icon.into()),
            border: Border {
                radius: 2.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        });
    let mut layers = iced::widget::Stack::new().push(iced::widget::pin(line).x(rect.x).y(rect.y));
    for x in [rect.x - 4.0, rect.x + rect.width - 4.0] {
        layers = layers.push(
            iced::widget::pin(container(space()).width(8).height(8).style(move |_| {
                container::Style {
                    background: Some(palette.tool_active_icon.into()),
                    border: Border {
                        color: palette.contrast_background,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..container::Style::default()
                }
            }))
            .x(x)
            .y(rect.y - 2.0),
        );
    }
    layers.into()
}

pub(crate) struct HitContext<'a> {
    pub item: &'a FolderBox,
    pub box_bounds: Rectangle,
    pub scroll_offset: f32,
    pub pinned: &'a [PathBuf],
    pub locations: &'a [DetectedLocation],
    pub home: &'a Path,
    pub all_directories: bool,
    pub ty: crate::window::typography::Style,
}

pub(crate) fn hit_zone(ctx: HitContext<'_>, point: Point) -> Option<HitZone> {
    if !ctx.item.has_sidebar() {
        return None;
    }
    let width = rail_width(ctx.item, ctx.box_bounds.width);
    let min_y = ctx.box_bounds.y + END_INSET;
    let max_y = (ctx.box_bounds.y + ctx.box_bounds.height - END_INSET).max(min_y);
    if width <= 2.0
        || point.x < ctx.box_bounds.x + 1.0
        || point.x >= ctx.box_bounds.x + width - 1.0
        || point.y < min_y
        || point.y >= max_y
    {
        return None;
    }
    let clamped = offset(
        ctx.pinned.len(),
        ctx.locations.len(),
        ctx.box_bounds.height,
        ctx.scroll_offset,
        ctx.ty,
    );
    let content_y = point.y - min_y + clamped;
    if let Some(zone) = top_section_zone(&ctx, width, (min_y, max_y), clamped, content_y) {
        return Some(zone);
    }
    bottom_section_zone(&ctx, width, (min_y, max_y), clamped, content_y)
}

fn top_section_zone(
    ctx: &HitContext<'_>,
    width: f32,
    (min_y, max_y): (f32, f32),
    clamped: f32,
    content_y: f32,
) -> Option<HitZone> {
    let header_h = ctx.ty.row(SECTION_HEADER_HEIGHT);
    let row_h = ctx.ty.row(ROW_HEIGHT);
    let empty_h = ctx.ty.row(EMPTY_SLOT_HEIGHT);
    let divider_h = ctx.ty.row(SECTION_DIVIDER_HEIGHT);
    let marker_at = |line_y: f32| Rectangle {
        x: ctx.box_bounds.x + 7.0,
        y: (min_y + line_y - clamped - 2.0).clamp(min_y, (max_y - 4.0).max(min_y)),
        width: (width - 14.0).max(1.0),
        height: 4.0,
    };
    if ctx.pinned.is_empty() {
        let limit = header_h
            + empty_h
            + if ctx.locations.is_empty() {
                f32::INFINITY
            } else {
                divider_h * 0.5
            };
        return (ctx.all_directories && content_y < limit).then(|| HitZone::PinInsert {
            index: 0,
            marker: marker_at(header_h + empty_h * 0.5),
        });
    }
    let top_end = header_h + ctx.pinned.len() as f32 * row_h;
    if content_y < header_h {
        return ctx.all_directories.then(|| HitZone::PinInsert {
            index: 0,
            marker: marker_at(header_h),
        });
    }
    if content_y < top_end {
        let idx = (((content_y - header_h) / row_h).floor() as usize).min(ctx.pinned.len() - 1);
        let row_top = header_h + idx as f32 * row_h;
        let rel = content_y - row_top;
        if ctx.all_directories && rel < row_h * 0.26 {
            return Some(HitZone::PinInsert {
                index: idx,
                marker: marker_at(row_top),
            });
        }
        if ctx.all_directories && rel > row_h * 0.74 {
            return Some(HitZone::PinInsert {
                index: idx + 1,
                marker: marker_at(row_top + row_h),
            });
        }
        let path = &ctx.pinned[idx];
        let rect = clipped_row_rect(
            ctx.box_bounds.x,
            width,
            (min_y, max_y),
            clamped,
            row_top,
            row_h,
        )?;
        return Some(HitZone::Row {
            directory: path.clone(),
            label: folder_display_name(path, ctx.home),
            rect,
        });
    }
    let tail_limit = if ctx.locations.is_empty() {
        f32::INFINITY
    } else {
        top_end + divider_h * 0.5
    };
    (ctx.all_directories && content_y < tail_limit).then(|| HitZone::PinInsert {
        index: ctx.pinned.len(),
        marker: marker_at(top_end),
    })
}

fn bottom_section_zone(
    ctx: &HitContext<'_>,
    width: f32,
    (min_y, max_y): (f32, f32),
    clamped: f32,
    content_y: f32,
) -> Option<HitZone> {
    if ctx.locations.is_empty() {
        return None;
    }
    let header_h = ctx.ty.row(SECTION_HEADER_HEIGHT);
    let row_h = ctx.ty.row(ROW_HEIGHT);
    let top_end = header_h
        + if ctx.pinned.is_empty() {
            ctx.ty.row(EMPTY_SLOT_HEIGHT)
        } else {
            ctx.pinned.len() as f32 * row_h
        };
    let loc_start = top_end + ctx.ty.row(SECTION_DIVIDER_HEIGHT) + header_h;
    let loc_end = loc_start + ctx.locations.len() as f32 * row_h;
    if content_y < loc_start || content_y >= loc_end {
        return None;
    }
    let idx = (((content_y - loc_start) / row_h).floor() as usize).min(ctx.locations.len() - 1);
    let row_top = loc_start + idx as f32 * row_h;
    let loc = &ctx.locations[idx];
    let rect = clipped_row_rect(
        ctx.box_bounds.x,
        width,
        (min_y, max_y),
        clamped,
        row_top,
        row_h,
    )?;
    Some(HitZone::Row {
        directory: loc.path.clone(),
        label: loc.label.clone(),
        rect,
    })
}

fn clipped_row_rect(
    box_x: f32,
    width: f32,
    (min_y, max_y): (f32, f32),
    clamped: f32,
    row_top: f32,
    row_h: f32,
) -> Option<Rectangle> {
    let raw = Rectangle {
        x: box_x + 1.0,
        y: min_y + row_top - clamped,
        width: (width - 2.0).max(1.0),
        height: row_h,
    };
    let viewport = Rectangle {
        x: box_x,
        y: min_y,
        width,
        height: (max_y - min_y).max(1.0),
    };
    raw.intersection(&viewport)
}

impl Desktop {
    pub(crate) fn sidebar_message(&mut self, message: Message) -> iced::Task<Message> {
        if !self.workspace_input_enabled() {
            return iced::Task::none();
        }
        match message {
            Message::SidebarScrolled(id, offset) => {
                self.sidebar_offsets.insert(id, offset);
                self.dispatch(Action::CancelPointer)
            }
            Message::SidebarPinPressed(id, index, path) => self.press_sidebar_pin(id, index, path),
            Message::SidebarLocationActivated(id, path) => self.activate_sidebar_location(id, path),
            _ => iced::Task::none(),
        }
    }

    pub(crate) fn activate_sidebar_location(
        &mut self,
        id: BoxId,
        path: PathBuf,
    ) -> iced::Task<Message> {
        let Some(item) = self
            .application
            .workspace()
            .boxes()
            .iter()
            .find(|item| item.id == id && item.has_sidebar())
        else {
            return iced::Task::none();
        };
        if !item.standalone.navigation.scope().allows(&path)
            || self
                .application
                .safety_policy(Some(item.standalone.view))
                .check_read_directory(&path)
                .is_err()
        {
            return iced::Task::none();
        }
        let view = item.standalone.view;
        if self.application.workspace().configuring(id)
            && let Some(draft) = &mut self.box_settings
            && draft.id == id
        {
            draft.focused = false;
            draft.range.focused = None;
        }
        if self.application.workspace().mode() == Mode::Browse {
            self.activate_view(view);
        }
        self.dispatch(Action::Navigate(view, Navigate::Folder(path)))
    }
}
