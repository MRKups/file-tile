//! Shared context-menu presentation and input, without model or filesystem effects.

use crate::{Desktop, Message, widgets::focus_control, window::appearance as a, window::icons};
use iced::widget::{button, container, mouse_area, opaque, pin, row, space, stack};
use iced::{Border, Center, Element, Fill, Point, Rectangle, Size, Task, keyboard};
mod press;
mod target;
pub(crate) use press::background;
pub(crate) use press::background_with_focus;
pub(crate) use press::scrollbars;
pub(crate) use press::view as press;
pub(crate) use press::window_coordinates;

pub(crate) const ROW_HEIGHT: f32 = 34.0;
pub(crate) const SURFACE_PADDING: f32 = 6.0;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Identity {
    Tab(u64),
    Sidebar(u64),
    Entry(u64),
    Folder(u64),
}

pub(crate) fn is_target(state: &dyn std::any::Any) -> bool {
    state.is::<target::Target>() || state.is::<target::Scrollbars>()
}

pub(crate) fn opening_focus(point: Option<Point>, first: &'static str) -> &'static str {
    if point.is_some() {
        "context-menu-no-selection"
    } else {
        first
    }
}

impl Desktop {
    pub(crate) fn context_menu_open(&self) -> bool {
        self.tab_menu.is_some()
            || self.sidebar_menu.is_some()
            || self.entry_menu.is_some()
            || self.folder_menu.is_some()
            || self.sort_menu.is_some()
    }

    pub(crate) fn prepare_context_menu(&mut self) {
        self.focus_version = self.focus_version.saturating_add(1);
        self.focused_path = None;
        self.type_jump.dismiss_session();
        for state in self.views.values_mut() {
            state.earlier_ancestors = false;
        }
        self.clear_trash_drafts();
        if let Some(draft) = &mut self.box_settings {
            draft.focused = false;
            draft.range.focused = None;
        }
        self.cancel_card_drag();
        self.sidebar_drag = None;
        self.file_drag.clear();
    }

    pub(crate) fn clear_context_menus(&mut self) {
        self.sort_menu = None;
        self.tab_menu = None;
        self.sidebar_menu = None;
        self.entry_menu = None;
        self.folder_menu = None;
    }

    pub(crate) fn reconcile_context_menus(&mut self) {
        self.reconcile_sort_menu();
        self.reconcile_tab_menu();
        self.reconcile_sidebar_menu();
        self.reconcile_entry_menu();
        if self
            .folder_menu
            .as_ref()
            .is_some_and(|menu| !self.folder_menu_valid(menu))
        {
            self.folder_menu = None;
        }
    }

    pub(crate) fn context_menu_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenFolderMenu(view, point) => self.open_folder_menu(view, point),
            Message::OpenPreviewFolderMenu(view, point) => {
                self.open_preview_folder_menu(view, point)
            }
            Message::FolderMenuAction(generation, command) => {
                self.folder_menu_action(generation, command)
            }
            Message::DismissFolderMenu(generation) => {
                if self
                    .folder_menu
                    .as_ref()
                    .is_some_and(|menu| menu.generation == generation)
                {
                    self.folder_menu = None;
                }
                Task::none()
            }
            Message::OpenTabMenu(id, view, point) => self.open_tab_menu(id, view, point),
            Message::DismissTabMenu(generation) => self.dismiss_tab_menu(generation),
            Message::CloseMenuTab(generation) => self.close_menu_tab(generation),
            Message::OpenSidebarMenu(id, path, point) => self.open_sidebar_menu(id, path, point),
            Message::DismissSidebarMenu(generation) => self.dismiss_sidebar_menu(generation),
            Message::UnpinSidebarMenu(generation) => self.unpin_sidebar_menu(generation),
            Message::OpenEntryMenu(view, path, point) => self.open_entry_menu(view, path, point),
            Message::DismissEntryMenu(generation) => self.dismiss_entry_menu(generation),
            Message::EntryMenuAction(generation, action) => {
                self.entry_menu_action(generation, action)
            }
            Message::ReplaceContextMenu(identity, target) => {
                self.replace_context_menu(identity, target)
            }
            _ => Task::none(),
        }
    }

    fn replace_context_menu(
        &mut self,
        identity: Identity,
        target: Option<Box<Message>>,
    ) -> Task<Message> {
        let current = match identity {
            Identity::Folder(generation) => self
                .folder_menu
                .as_ref()
                .is_some_and(|menu| menu.generation == generation),
            Identity::Tab(generation) => self
                .tab_menu
                .as_ref()
                .is_some_and(|menu| menu.generation == generation),
            Identity::Sidebar(generation) => self
                .sidebar_menu
                .as_ref()
                .is_some_and(|menu| menu.generation == generation),
            Identity::Entry(generation) => self
                .entry_menu
                .as_ref()
                .is_some_and(|menu| menu.generation == generation),
        };
        if !current {
            return Task::none();
        }
        if let Some(target) = target {
            self.clear_context_menus();
            match *target {
                message @ (Message::OpenEntryMenu(..)
                | Message::OpenTabMenu(..)
                | Message::OpenSidebarMenu(..)
                | Message::OpenFolderMenu(..)
                | Message::OpenPreviewFolderMenu(..)) => self.context_menu_message(message),
                _ => Task::none(),
            }
        } else {
            match identity {
                Identity::Folder(generation) => {
                    self.context_menu_message(Message::DismissFolderMenu(generation))
                }
                Identity::Tab(generation) => self.dismiss_tab_menu(generation),
                Identity::Sidebar(generation) => self.dismiss_sidebar_menu(generation),
                Identity::Entry(generation) => self.dismiss_entry_menu(generation),
            }
        }
    }
}

pub(crate) fn item<'a>(
    id: &'static str,
    icon_id: impl Into<iced::advanced::widget::Id>,
    icon: icons::Icon,
    label: &'static str,
    action: Option<Message>,
    palette: a::Palette,
) -> Element<'a, Message> {
    item_with_suffix(id, icon_id, icon, label, None, action, palette)
}

pub(crate) fn item_with_suffix<'a>(
    id: &'static str,
    icon_id: impl Into<iced::advanced::widget::Id>,
    icon: icons::Icon,
    label: &'static str,
    suffix: Option<&'static str>,
    action: Option<Message>,
    palette: a::Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    let enabled = action.is_some();
    let mut row_content = row![
        container(icons::view(
            icon,
            14.0,
            Some(if enabled { palette.text } else { palette.muted }),
            1.0
        ))
        .id(icon_id),
        ty.text(label),
    ]
    .spacing(8)
    .align_y(Center);
    if let Some(suffix) = suffix {
        row_content = row_content
            .push(space().width(Fill))
            .push(ty.small(suffix).color(palette.muted));
    }
    let label = container(row_content).width(Fill).center_y(Fill);
    let control = button(label)
        .width(Fill)
        .height(ty.row(ROW_HEIGHT))
        .padding([0, 12])
        .style(move |_, status| button::Style {
            text_color: if status == button::Status::Disabled {
                palette.muted
            } else {
                palette.text
            },
            background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                .then_some(palette.row_hover.into()),
            border: Border {
                radius: 5.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        })
        .on_press_maybe(action.clone());
    match action {
        Some(action) => {
            focus_control::themed_highlight_keys(id, action, control, Vec::new(), palette)
        }
        None => control.into(),
    }
}

pub(crate) fn bounds(position: Point, size: Size, width: f32, height: f32) -> Rectangle {
    let width = width.min((size.width - 16.0).max(1.0));
    let height = height.min((size.height - 16.0).max(1.0));
    Rectangle {
        x: position.x.clamp(8.0, (size.width - width - 8.0).max(8.0)),
        y: position.y.clamp(8.0, (size.height - height - 8.0).max(8.0)),
        width,
        height,
    }
}

pub(crate) fn divider<'a>(palette: a::Palette) -> Element<'a, Message> {
    iced::widget::container(iced::widget::rule::horizontal(1).style(move |_| palette.rule()))
        .padding([4, 12])
        .into()
}

pub(crate) fn surface<'a>(
    id: &'static str,
    contents: impl Into<Element<'a, Message>>,
    rect: Rectangle,
    dismiss: Message,
    palette: a::Palette,
) -> Element<'a, Message> {
    styled_surface(
        id,
        contents,
        rect,
        dismiss,
        container::Style {
            background: Some(palette.contrast_background.into()),
            ..palette.panel()
        },
    )
}

pub(crate) fn styled_surface<'a>(
    id: &'static str,
    contents: impl Into<Element<'a, Message>>,
    rect: Rectangle,
    dismiss: Message,
    style: iced::widget::container::Style,
) -> Element<'a, Message> {
    let panel = container(contents)
        .id(id)
        .padding(SURFACE_PADDING)
        .width(rect.width)
        .height(rect.height)
        .style(move |_| container::Style {
            border: Border {
                radius: 8.0.into(),
                ..style.border
            },
            shadow: iced::Shadow {
                color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.12),
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 14.0,
            },
            ..style
        });
    let backdrop = opaque(
        mouse_area(container(space()).width(Fill).height(Fill))
            .on_release(dismiss.clone())
            .on_right_press(dismiss.clone())
            .on_scroll(move |_| dismiss.clone()),
    );
    stack![backdrop, pin(opaque(panel)).x(rect.x).y(rect.y)]
        .width(Fill)
        .height(Fill)
        .into()
}

pub(crate) fn input(event: iced::Event) -> Task<Message> {
    match event {
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(key),
            modifiers,
            repeat: false,
            ..
        }) => match key {
            keyboard::key::Named::Tab => {
                if modifiers.shift() {
                    iced::widget::operation::focus_previous()
                } else {
                    iced::widget::operation::focus_next()
                }
            }
            keyboard::key::Named::ArrowUp if modifiers.is_empty() => {
                iced::widget::operation::focus_previous()
            }
            keyboard::key::Named::ArrowDown if modifiers.is_empty() => {
                iced::widget::operation::focus_next()
            }
            _ => Task::none(),
        },
        _ => Task::none(),
    }
}
