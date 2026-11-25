//! Vertical tab presentation and keyboard actions. Navigation remains application-owned.

use crate::application::Action;
use crate::domain::{BoxId, FolderBox, FolderView, Mode, TabCommand, ViewId};
use crate::{
    Message, boxes::navigation_header, canvas::geometry, widgets::focus_control,
    window::appearance as a,
};
use iced::widget::{button, column, container, keyed_column, row, scrollable, text, tooltip};
use iced::{Center, Element, Fill, Rectangle, keyboard};
use std::path::Path;
mod retained_tab;

pub const ROW_HEIGHT: f32 = 48.0;
pub const ADD_HEIGHT: f32 = 30.0;
pub const END_INSET: f32 = 1.0;
const CORNER_RADIUS: f32 = 9.0;

pub fn width(item: &FolderBox, box_width: f32) -> f32 {
    if item.tabs.as_ref().is_some_and(|tabs| tabs.visible()) {
        (3.0 * geometry::PITCH - geometry::GAP).min(box_width * 0.65)
    } else {
        0.0
    }
}
pub fn tab_id(view: ViewId) -> String {
    format!("directory-tab-{}", view.0)
}
pub fn scroll_id(id: BoxId) -> String {
    format!("directory-tabs-{}", id.0)
}
pub fn add_id(id: BoxId) -> String {
    format!("directory-tab-add-{}", id.0)
}

pub fn offset(
    item: &FolderBox,
    height: f32,
    stored: f32,
    ty: crate::window::typography::Style,
) -> f32 {
    let count = item.tabs.as_ref().map_or(0, |tabs| tabs.views().len());
    let maximum = (count as f32 * ty.row(ROW_HEIGHT)
        - (height - ty.row(ADD_HEIGHT) - 2.0 * END_INSET).max(0.0))
    .max(0.0);
    // Match Iced's clamped, rounded absolute translation after list/box shrinkage.
    stored.clamp(0.0, maximum).round()
}

impl crate::Desktop {
    pub(crate) fn tab_message(&mut self, message: Message) -> iced::Task<Message> {
        if !self.workspace_input_enabled() {
            return iced::Task::none();
        }
        match message {
            Message::BeginTabDrag(id, view, point) => self.begin_tab_drag(id, view, point),
            Message::MoveTabDrag(point) => {
                self.move_tab_drag(point);
                iced::Task::none()
            }
            Message::FinishTabDrag => self.finish_tab_drag(),
            Message::TabRailScrolled(id, offset) => {
                self.tab_offsets.insert(id, offset);
                self.dispatch(Action::CancelPointer)
            }
            Message::FocusTab(view) => self.focus_tab(view),
            _ => iced::Task::none(),
        }
    }

    pub(crate) fn reconcile_tab_activation(
        &mut self,
        before: std::collections::HashMap<BoxId, ViewId>,
    ) -> iced::Task<Message> {
        let activated = self
            .application
            .workspace()
            .boxes()
            .iter()
            .find(|item| {
                before
                    .get(&item.id)
                    .is_some_and(|view| *view != item.active_view().view)
            })
            .map(|item| {
                (
                    item.active_view().view,
                    item.tabs.as_ref().is_some_and(|tabs| tabs.visible()),
                )
            });
        for item in self.application.workspace().boxes() {
            if let Some(previous) = before
                .get(&item.id)
                .copied()
                .filter(|view| *view != item.active_view().view)
            {
                if let Some(state) = self.views.get_mut(&previous) {
                    state.motion = None;
                }
                if let Some(state) = self.views.get_mut(&item.active_view().view) {
                    state.motion = None;
                }
            }
            if item.active_view().view == item.standalone.view
                || before.get(&item.id) == Some(&item.standalone.view)
            {
                self.tab_offsets.remove(&item.id);
            }
        }
        self.tab_offsets.retain(|id, _| {
            self.application
                .workspace()
                .boxes()
                .iter()
                .any(|item| item.id == *id)
        });
        self.sidebar_offsets.retain(|id, _| {
            self.application
                .workspace()
                .boxes()
                .iter()
                .any(|item| item.id == *id && item.has_sidebar())
        });
        if let Some((view, tabbed)) = activated {
            self.type_jump.dismiss_session();
            self.activate_view(view);
            self.focused_path = None;
            self.focus_version = self.focus_version.saturating_add(1);
            if tabbed {
                self.focus_tab(view)
            } else {
                iced::widget::operation::focus("workspace-canvas")
            }
        } else {
            iced::Task::none()
        }
    }
    pub(crate) fn focus_tab(&mut self, view: ViewId) -> iced::Task<Message> {
        if self.application.workspace().mode() == crate::domain::Mode::Browse
            && self.application.workspace().is_active(view)
        {
            self.activate_view(view);
        }
        let Some(item) = self.application.workspace().boxes().iter().find(|item| {
            item.tabs.as_ref().is_some_and(|tabs| {
                tabs.visible() && tabs.views().iter().any(|item| item.view == view)
            })
        }) else {
            return iced::Task::none();
        };
        if self.application.workspace().configuring(item.id)
            && let Some(draft) = &mut self.box_settings
            && draft.id == item.id
        {
            // A tab-body press focuses its target before requesting activation.
            // Suppress the old editor's same-press blur, including a same-tab no-op.
            draft.focused = false;
            draft.range.focused = None;
        }
        let Some(index) = item
            .tabs
            .as_ref()
            .and_then(|tabs| tabs.views().iter().position(|item| item.view == view))
        else {
            return iced::Task::none();
        };
        let geometry = self.geometry();
        let ty = geometry.typography;
        let height = geometry.rect(item.rect).height;
        let row_height = ty.row(ROW_HEIGHT);
        let visible = (height - ty.row(ADD_HEIGHT) - 2.0 * END_INSET).max(0.0);
        let current = offset(
            item,
            height,
            self.tab_offsets.get(&item.id).copied().unwrap_or(0.0),
            ty,
        );
        let top = index as f32 * row_height;
        let target = if top < current || visible < row_height {
            top
        } else if top + row_height > current + visible {
            top + row_height - visible
        } else {
            current
        };
        let target = offset(item, height, target, ty);
        self.tab_offsets.insert(item.id, target);
        iced::widget::operation::scroll_to(
            scroll_id(item.id),
            iced::widget::operation::AbsoluteOffset { x: 0.0, y: target },
        )
        .chain(iced::widget::operation::focus(tab_id(view)))
    }
}

pub struct Props<'a> {
    pub palette: a::Palette,
    pub item: &'a FolderBox,
    pub home: &'a Path,
    pub mode: Mode,
    pub interactive: bool,
    pub width: f32,
    pub dragged: Option<ViewId>,
}

pub fn view(props: Props<'_>) -> Element<'_, Message> {
    let palette = props.palette;
    let ty = palette.typography;
    let add_height = ty.row(ADD_HEIGHT);
    let Some(tabs) = props.item.tabs.as_ref().filter(|tabs| tabs.visible()) else {
        return iced::widget::space().into();
    };
    let rows = keyed_column(
        tabs.views()
            .iter()
            .enumerate()
            .map(|(index, view)| (view.view, tab_row(&props, view, index, tabs.views()))),
    )
    .width(Fill);
    let rows = a::scroll_area(rows)
        .style(palette.scrollable())
        .id(scroll_id(props.item.id))
        .width(Fill)
        .height(Fill)
        .direction(scrollable::Direction::Vertical(a::scrollbar()))
        .on_scroll(move |viewport| {
            Message::TabRailScrolled(props.item.id, viewport.absolute_offset().y)
        });
    let rows = crate::menus::context_menu::scrollbars(a::SCROLLBAR_WIDTH, 0.0, rows.into());
    let add =
        button(
            container(
                ty.text("+")
                    .size(ty.size(a::HEADING_SIZE))
                    .color(palette.muted),
            )
            .width(Fill)
            .height(add_height)
            .align_x(Center)
            .align_y(Center),
        )
        .padding(0)
        .width(Fill)
        .height(add_height)
        .style(move |_, status| button::Style {
            text_color: palette.muted,
            background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                .then_some(palette.row_hover.into()),
            border: iced::Border {
                radius: iced::border::Radius {
                    bottom_left: CORNER_RADIUS,
                    ..iced::border::Radius::default()
                },
                ..iced::Border::default()
            },
            ..button::Style::default()
        })
        .on_press_maybe((props.interactive && props.mode != Mode::Layout).then_some(
            Message::Action(Action::ChangeTabs(props.item.id, TabCommand::Add)),
        ));
    let add: Element<'_, Message> = if props.interactive && props.mode != Mode::Layout {
        tooltip(
            focus_control::themed_highlight_keys_radius(
                add_id(props.item.id),
                Message::Action(Action::ChangeTabs(props.item.id, TabCommand::Add)),
                add,
                Vec::new(),
                iced::border::Radius {
                    bottom_left: CORNER_RADIUS,
                    ..iced::border::Radius::default()
                },
                palette,
            ),
            ty.small("Add tab"),
            tooltip::Position::Top,
        )
        .style(move |_| palette.tooltip())
        .into()
    } else {
        iced::widget::space().height(add_height).into()
    };
    let rail = container(column![rows, container(add).id(add_id(props.item.id))].height(Fill))
        // The rectangular scroller stays inside the curved shell ends. This also
        // contains hover/focus/drag fills when a partially scrolled row is first.
        .padding(iced::Padding {
            top: END_INSET,
            bottom: END_INSET,
            left: 1.0,
            right: 0.0,
        })
        .width((props.width - 1.0).max(0.0))
        .height(Fill);
    row![
        rail,
        iced::widget::rule::vertical(1).style(move |_| palette.rule()),
    ]
    .width(props.width)
    .height(Fill)
    .into()
}

fn tab_row<'a>(
    props: &Props<'a>,
    view: &'a FolderView,
    index: usize,
    views: &[FolderView],
) -> Element<'a, Message> {
    let palette = props.palette;
    let path = navigation_header::display_path(view.navigation.current(), props.home);
    let selected = tab_select(props, view, index, views, path.clone());
    let controls = row![selected]
        .width(Fill)
        .height(palette.typography.row(ROW_HEIGHT));
    let controls = retained_tab::view(view.view, props.interactive, controls.into());
    let row: Element<'a, Message> = container(controls)
        .id(tab_id(view.view))
        .clip(true)
        .style(move |_| container::Style {
            // The button owns its feedback fill. No rectangular row outline may
            // overpaint the rail's rounded shell.
            ..container::Style::default()
        })
        .into();
    let row: Element<'a, Message> = if props.dragged == Some(view.view) {
        let top_radius = if index == 0 { CORNER_RADIUS } else { 0.0 };
        iced::widget::stack![
            row,
            container(iced::widget::space())
                .width(Fill)
                .height(Fill)
                .style(move |_| container::Style {
                    background: Some(
                        iced::Color {
                            a: 0.65,
                            ..palette.editing_overlay
                        }
                        .into()
                    ),
                    border: iced::Border {
                        radius: iced::border::Radius {
                            top_left: top_radius,
                            ..iced::border::Radius::default()
                        },
                        ..iced::Border::default()
                    },
                    ..container::Style::default()
                })
        ]
        .into()
    } else {
        row
    };
    tooltip(
        row,
        palette.typography.small(path),
        tooltip::Position::Right,
    )
    .delay(std::time::Duration::from_millis(650))
    .style(move |_| palette.tooltip())
    .into()
}

fn tab_select<'a>(
    props: &Props<'a>,
    view: &'a FolderView,
    index: usize,
    views: &[FolderView],
    path: String,
) -> Element<'a, Message> {
    let palette = props.palette;
    let ty = palette.typography;
    let name = view
        .navigation
        .current()
        .file_name()
        .unwrap_or_else(|| view.navigation.current().as_os_str())
        .to_string_lossy();
    let active = props.item.active_view().view == view.view;
    let id = props.item.id;
    let action = Message::Action(Action::ChangeTabs(id, TabCommand::Activate(view.view)));
    let selected = container(
        column![
            ty.text(name)
                .color(palette.tab_text)
                .wrapping(text::Wrapping::None),
            ty.caption(path)
                .color(palette.muted)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(4),
    )
    .padding([6, 16])
    .width(Fill)
    .height(ty.row(ROW_HEIGHT))
    .align_y(Center)
    .clip(true);
    let top_radius = if index == 0 { CORNER_RADIUS } else { 0.0 };
    let selected = button(selected)
        .padding(0)
        .width(Fill)
        .height(ty.row(ROW_HEIGHT))
        .on_press_maybe(
            (props.interactive && props.mode != Mode::Layout)
                .then_some(Message::FocusTab(view.view)),
        )
        .style(move |_, status| button::Style {
            text_color: palette.text,
            background: Some(
                if active {
                    palette.row_selected
                } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                    palette.row_hover
                } else {
                    iced::Color::TRANSPARENT
                }
                .into(),
            ),
            border: iced::Border {
                radius: iced::border::Radius {
                    top_left: top_radius,
                    ..iced::border::Radius::default()
                },
                ..iced::Border::default()
            },
            ..button::Style::default()
        });
    let target = view.view;
    let context: Option<Box<dyn Fn(iced::Point) -> Message>> = (props.interactive
        && props.mode != Mode::Layout)
        .then(|| Box::new(move |point| Message::OpenTabMenu(id, target, Some(point))) as _);
    let selected = crate::menus::context_menu::press(
        (props.interactive && props.mode != Mode::Layout).then_some((view.view, action.clone())),
        context,
        selected.into(),
    );
    let keys = keys(id, index, views, props.mode);
    if props.interactive {
        focus_control::themed_highlight_keys_radius(
            tab_id(view.view),
            if props.mode == Mode::Layout {
                Message::FocusTab(view.view)
            } else {
                action
            },
            selected,
            keys,
            iced::border::Radius {
                top_left: top_radius,
                ..iced::border::Radius::default()
            },
            palette,
        )
    } else {
        selected
    }
}

fn keys(
    id: BoxId,
    index: usize,
    views: &[FolderView],
    mode: Mode,
) -> Vec<(keyboard::key::Named, keyboard::Modifiers, Message)> {
    use keyboard::{Modifiers as M, key::Named as K};
    let previous = if mode == Mode::Layout {
        index.saturating_sub(1)
    } else {
        (index + views.len() - 1) % views.len()
    };
    let next = if mode == Mode::Layout {
        (index + 1).min(views.len() - 1)
    } else {
        (index + 1) % views.len()
    };
    let mut keys = [
        (K::ArrowUp, previous),
        (K::ArrowDown, next),
        (K::Home, 0),
        (K::End, views.len() - 1),
    ]
    .into_iter()
    .map(|(key, target)| {
        (
            key,
            M::empty(),
            if mode == Mode::Layout {
                Message::FocusTab(views[target].view)
            } else {
                Message::Action(Action::ChangeTabs(
                    id,
                    TabCommand::Activate(views[target].view),
                ))
            },
        )
    })
    .collect::<Vec<_>>();
    if mode == Mode::Layout {
        for (key, target) in [
            (K::ArrowUp, index.saturating_sub(1)),
            (K::ArrowDown, (index + 1).min(views.len() - 1)),
        ] {
            keys.push((
                key,
                M::ALT,
                Message::Action(Action::ChangeTabs(
                    id,
                    TabCommand::Reorder {
                        view: views[index].view,
                        position: target,
                    },
                )),
            ));
        }
    } else {
        keys.push((
            K::F10,
            M::SHIFT,
            Message::OpenTabMenu(id, views[index].view, None),
        ));
    }
    keys
}

pub fn row_at(
    item: &FolderBox,
    bounds: Rectangle,
    offset: f32,
    point: iced::Point,
    ty: crate::window::typography::Style,
) -> Option<(ViewId, usize)> {
    let tabs = item.tabs.as_ref().filter(|tabs| tabs.visible())?;
    if point.x < bounds.x + 1.0
        || point.x >= bounds.x + width(item, bounds.width) - 1.0
        || point.y < bounds.y + END_INSET
        || point.y >= bounds.y + (bounds.height - ty.row(ADD_HEIGHT) - END_INSET).max(END_INSET)
    {
        return None;
    }
    let index = ((point.y - bounds.y - END_INSET + self::offset(item, bounds.height, offset, ty))
        / ty.row(ROW_HEIGHT))
    .floor() as usize;
    tabs.views().get(index).map(|view| (view.view, index))
}
