//! Top-level window, mouse, keyboard, and focus-probing input routing.

use std::path::PathBuf;

use iced::event::{Event, Status};
use iced::keyboard::{self, Key, key};
use iced::widget::operation;
use iced::{Point, Size, Task, mouse, window as iced_window};

use crate::application::contracts::directory::EntryKind;
use crate::application::{Action, LoadState, Navigate};
use crate::domain::{Mode, ViewId};
use crate::{
    Desktop, Message,
    boxes::{launchers, navigation_header},
    canvas::workspace_header,
    dialogs::{box_settings, range_editor},
};

const WINDOW_DRAG_THRESHOLD: f32 = 2.0;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) enum WindowDrag {
    #[default]
    Idle,
    Pressed,
    Armed(Point),
}

fn can_start_window_drag() -> bool {
    #[cfg(all(target_os = "macos", not(test)))]
    {
        crate::platform::drag::can_start_window_drag()
    }
    #[cfg(any(not(target_os = "macos"), test))]
    {
        true
    }
}

impl Desktop {
    pub(crate) fn open_entry(&mut self, view: ViewId, path: PathBuf) -> Task<Message> {
        self.file_drag.clear();
        if !self.file_view_input_enabled(view) {
            return Task::none();
        }
        let kind = self.visible_entry(view, &path).map(|entry| entry.kind);
        match kind {
            Some(EntryKind::Directory) => {
                self.dispatch(Action::Navigate(view, Navigate::Folder(path)))
            }
            Some(EntryKind::File | EntryKind::Other) => self.dispatch(Action::OpenFile(view, path)),
            None => Task::none(),
        }
    }

    pub(crate) fn visible_entry(
        &self,
        view: ViewId,
        path: &std::path::Path,
    ) -> Option<&crate::application::contracts::directory::DirectoryEntry> {
        self.application
            .load(view)
            .and_then(LoadState::listing)
            .and_then(|listing| {
                listing.entry(path).filter(|entry| {
                    (entry.kind != EntryKind::Directory
                        || self.application.workspace().view(view).is_some_and(|item| {
                            item.navigation.scope().kind() == crate::domain::ScopeKind::Free
                        })
                        || listing.entry_target(entry).is_some_and(|target| {
                            self.application
                                .workspace()
                                .view(view)
                                .is_some_and(|item| item.navigation.scope().allows(target))
                        }))
                        && (self
                            .application
                            .workspace()
                            .view(view)
                            .is_some_and(|item| item.settings.show_hidden)
                            || !entry.hidden)
                })
            })
    }

    pub(crate) fn probe_path_focus(&mut self) -> Task<Message> {
        self.focus_version = self.focus_version.saturating_add(1);
        let version = self.focus_version;
        let paths = self
            .application
            .workspace()
            .active_views()
            .enumerate()
            .map(|(index, item)| {
                let view = item.view;
                operation::is_focused(navigation_header::path_id(view))
                    .map(move |focused| (index, Some(view), focused))
            });
        let card_id = self.box_settings.as_ref().map_or_else(
            || "box-settings-closed".into(),
            |draft| box_settings::folder_id(draft.view),
        );
        let card = operation::is_focused(card_id).map(|focused| (usize::MAX, None, focused));
        let view = self
            .box_settings
            .as_ref()
            .map_or(ViewId(0), |draft| draft.view);
        let start = operation::is_focused(range_editor::start_id(view))
            .map(|focused| (usize::MAX - 1, None, focused));
        let stops = operation::is_focused(range_editor::stops_id(view))
            .map(|focused| (usize::MAX - 2, None, focused));
        let launchers = self.application.workspace().launchers().iter().map(|item| {
            operation::is_focused(format!("launcher-{}", item.id.0))
                .map(|focused| (usize::MAX - 3, None, focused))
        });
        Task::batch(paths.chain([card, start, stops]).chain(launchers))
            .collect()
            .map(move |observations| {
                if observations
                    .iter()
                    .any(|(index, _, focused)| *index == usize::MAX - 3 && *focused)
                {
                    return Message::Launcher(launchers::Event::FocusChecked(version));
                }
                let box_folder = observations
                    .iter()
                    .any(|(index, _, focused)| *index == usize::MAX && *focused);
                let boundary = if observations
                    .iter()
                    .any(|(index, _, focused)| *index == usize::MAX - 1 && *focused)
                {
                    Some(range_editor::Field::Start)
                } else if observations
                    .iter()
                    .any(|(index, _, focused)| *index == usize::MAX - 2 && *focused)
                {
                    Some(range_editor::Field::Stops)
                } else {
                    None
                };
                let focused = observations
                    .into_iter()
                    .filter(|(_, _, focused)| *focused)
                    .max_by_key(|(index, _, _)| *index)
                    .and_then(|(_, view, _)| view);
                Message::PathFocusChecked {
                    focused,
                    box_folder,
                    boundary,
                    version,
                }
            })
    }

    pub(crate) fn adopt_viewport(&mut self, size: Size) -> Task<Message> {
        if self.size == size {
            return Task::none();
        }
        self.size = size;
        self.window_drag = WindowDrag::Idle;
        self.clear_context_menus();
        self.cancel_card_drag();
        self.pointer = None;
        self.dispatch(Action::CancelPointer)
    }

    fn can_drag_header(&self, point: Point) -> bool {
        let geometry = self.geometry();
        !self.modal_open()
            && workspace_header::can_drag(
                point,
                self.size.width,
                geometry.header_height,
                self.application.workspace().mode(),
            )
    }

    pub(crate) fn drag_window(&mut self) -> Task<Message> {
        if self.modal_open() {
            self.window_drag = WindowDrag::Idle;
            return Task::none();
        }
        let Some(point) = self.pointer else {
            self.window_drag = WindowDrag::Pressed;
            return Task::none();
        };
        if self.can_drag_header(point) {
            self.window_drag = WindowDrag::Armed(point);
            self.dispatch(Action::CancelPointer)
        } else {
            self.window_drag = WindowDrag::Idle;
            Task::none()
        }
    }

    fn move_window_drag(&mut self, position: Point) -> Option<Task<Message>> {
        match self.window_drag {
            WindowDrag::Idle => None,
            WindowDrag::Pressed => {
                if self.can_drag_header(position) {
                    self.window_drag = WindowDrag::Armed(position);
                    Some(self.dispatch(Action::CancelPointer))
                } else {
                    self.window_drag = WindowDrag::Idle;
                    None
                }
            }
            WindowDrag::Armed(origin) => {
                if origin.distance(position) < WINDOW_DRAG_THRESHOLD {
                    return Some(Task::none());
                }
                self.window_drag = WindowDrag::Idle;
                if can_start_window_drag() {
                    Some(
                        self.window_runtime
                            .id()
                            .map_or_else(Task::none, iced_window::drag),
                    )
                } else {
                    Some(Task::none())
                }
            }
        }
    }

    fn left_button_pressed(&mut self, status: Status) -> Task<Message> {
        let geometry = self.geometry();
        if self.application.workspace().mode() == Mode::Layout {
            return if status == Status::Ignored {
                self.pointer
                    .map_or_else(Task::none, |point| self.editing_press(point))
            } else {
                Task::none()
            };
        }
        if let Some(point) = self.pointer
            && geometry.can_start(point)
            && status == Status::Ignored
        {
            let task = self.dispatch(Action::BeginItemDraw(geometry.cell(point), geometry.extent));
            return Task::batch([task, self.probe_path_focus()]);
        }
        self.probe_path_focus()
    }

    pub(crate) fn input(&mut self, event: Event, status: Status) -> Task<Message> {
        if let Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) = &event {
            self.modifiers = *modifiers;
        }
        if let Some(task) = self.blocked_input(&event, status) {
            self.window_drag = WindowDrag::Idle;
            return task;
        }
        if let Some(task) = self.window_file_drag_event(&event) {
            return task;
        }
        match event {
            Event::Window(iced_window::Event::CloseRequested) => {
                self.dispatch(Action::RequestClose)
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                self.pointer = Some(position);
                if let Some(task) = self.move_window_drag(position) {
                    return task;
                }
                if self.move_file_drag(position) {
                    return Task::none();
                }
                if self.tab_drag.is_some() {
                    self.move_tab_drag(position);
                    return Task::none();
                }
                if self.drag_card_to(position) {
                    return Task::none();
                }
                if self.application.workspace().layout_preview().is_some() {
                    return self.editing_message(Message::LayoutPointerMoved(position));
                }
                if self.application.workspace().draw_preview().is_some() {
                    self.dispatch(Action::DrawTo(self.geometry().cell(position)))
                } else {
                    Task::none()
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                self.left_button_pressed(status)
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                self.window_drag = WindowDrag::Idle;
                if self.file_drag.has_pending_or_active() {
                    return self.finish_file_drag();
                }
                if self.tab_drag.is_some() {
                    return self.finish_tab_drag();
                }
                self.cancel_card_drag();
                if self.application.workspace().layout_preview().is_some() {
                    return self.editing_message(Message::FinishLayout);
                }
                self.dispatch(Action::FinishDraw)
            }
            Event::Window(
                iced_window::Event::Opened { .. }
                | iced_window::Event::Resized(_)
                | iced_window::Event::Rescaled(_),
            ) => {
                self.window_drag = WindowDrag::Idle;
                self.clear_context_menus();
                self.clear_trash_drafts();
                // Subscriptions can arrive after a live-resize redraw. The root
                // sensor owns size; queued window events only cancel gestures.
                self.pointer = None;
                self.cancel_card_drag();
                self.dispatch(Action::CancelPointer)
            }
            Event::Window(iced_window::Event::Unfocused) => {
                self.window_drag = WindowDrag::Idle;
                self.modifiers = keyboard::Modifiers::empty();
                self.clear_context_menus();
                self.clear_trash_drafts();
                self.pointer = None;
                self.cancel_card_drag();
                self.focused_path = None;
                self.focus_version = self.focus_version.saturating_add(1);
                self.dispatch(Action::CancelPointer)
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                repeat,
                ..
            }) if !repeat => self.key(key, modifiers, status),
            _ => Task::none(),
        }
    }

    pub(crate) fn key(
        &mut self,
        key: Key,
        modifiers: keyboard::Modifiers,
        status: Status,
    ) -> Task<Message> {
        if key == Key::Named(key::Named::Escape) {
            if let Some(view) = self
                .box_settings
                .as_ref()
                .filter(|draft| draft.range_page)
                .map(|draft| draft.view)
            {
                return self.cancel_range_page(view);
            }
            if let Some(task) = self.file_escape() {
                return task;
            }
            if self.application.workspace().mode() != Mode::Browse {
                let task = self.dispatch(Action::Escape);
                self.focused_path = None;
                self.focus_version = self.focus_version.saturating_add(1);
                return Task::batch([task, operation::focus("workspace-canvas")]);
            }
            if status == Status::Captured {
                self.focus_version = self.focus_version.saturating_add(1);
                if let Some(view) = self.focused_path.take() {
                    let presentation = self.views.entry(view).or_default();
                    presentation.draft = None;
                    presentation.invalid = false;
                }
                return Task::none();
            }
            return self.dispatch(Action::Escape);
        }
        if status == Status::Captured {
            return Task::none();
        }
        if key == Key::Named(key::Named::Tab) {
            let task = if modifiers.shift() {
                operation::focus_previous()
            } else {
                operation::focus_next()
            };
            return task.chain(self.probe_path_focus());
        }
        if !modifiers.is_empty() {
            if self.launchers.draft.is_some() {
                return Task::none();
            }
            if self.file_editor_active() {
                return Task::none();
            }
            if let Some(task) = self.file_key(&key, modifiers) {
                return task;
            }
            if !modifiers.shift()
                && !modifiers.alt()
                && matches!(key.as_ref(), Key::Character("r" | "R"))
                && (modifiers.command() || modifiers.control())
            {
                // Unsupported editor shortcuts may be left uncaptured by Iced.
                if self.focused_path.is_some()
                    || self
                        .box_settings
                        .as_ref()
                        .is_some_and(|draft| draft.focused || draft.range.focused.is_some())
                {
                    return Task::none();
                }
                return self.dispatch(Action::ReloadAll);
            }
            if !modifiers.shift()
                && !modifiers.alt()
                && matches!(key.as_ref(), Key::Character("z" | "Z"))
                && (modifiers.command() || modifiers.control())
            {
                if self.focused_path.is_some()
                    || self
                        .box_settings
                        .as_ref()
                        .is_some_and(|draft| draft.focused || draft.range.focused.is_some())
                {
                    return Task::none();
                }
                return self.dispatch(Action::Undo);
            }
            return Task::none();
        }
        match key.as_ref() {
            Key::Named(key::Named::Enter) => {
                let selected = self.active_view.and_then(|view| {
                    self.views
                        .get(&view)
                        .and_then(|presentation| presentation.selected.clone())
                        .map(|path| (view, path))
                });
                if let Some((view, path)) = selected {
                    self.open_entry(view, path)
                } else {
                    Task::none()
                }
            }
            _ => Task::none(),
        }
    }
}
