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
        if kind.is_some() {
            self.type_jump.dismiss_session();
            self.activate_view(view);
        }
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
        Task::batch([
            self.dispatch(Action::CancelPointer),
            self.reobserve_active_scrolls(),
        ])
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
        if self.workspace_input_enabled() {
            self.clear_motion_reveals();
        }
        let geometry = self.geometry();
        if self.application.workspace().mode() == Mode::Layout {
            return if status == Status::Ignored {
                self.pointer
                    .map_or_else(Task::none, |point| self.editing_press(point))
            } else {
                Task::none()
            };
        }
        if status == Status::Ignored
            && self.application.workspace().mode() == Mode::Browse
            && self.workspace_input_enabled()
            && !self.file_editor_active()
            && let Some(point) = self.pointer
            && point.y >= geometry.header_height
        {
            if geometry.viewport.contains(point) {
                let content_point = Point::new(
                    point.x - geometry.viewport.x + geometry.offset.x,
                    point.y - geometry.viewport.y + geometry.offset.y,
                );
                if let Some(box_view) = self
                    .application
                    .workspace()
                    .boxes()
                    .iter()
                    .find(|item| geometry.rect(item.rect).contains(content_point))
                    .map(|item| item.active_view().view)
                    .filter(|view| self.application.workspace().can_browse(*view))
                {
                    if self.active_view != Some(box_view) {
                        self.type_jump.dismiss_session();
                        self.activate_view(box_view);
                    }
                } else if !self
                    .application
                    .workspace()
                    .placements()
                    .any(|item| geometry.rect(item.rect).contains(content_point))
                {
                    self.deactivate_all_views();
                }
            } else {
                self.deactivate_all_views();
            }
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
            Event::Mouse(mouse::Event::WheelScrolled { .. }) => {
                if self.workspace_input_enabled() {
                    self.clear_motion_reveals();
                }
                Task::none()
            }
            Event::Window(iced_window::Event::Focused) => self.probe_path_focus(),
            Event::Window(iced_window::Event::Unfocused) => self.window_unfocused(),
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                repeat,
                ..
            }) => self.key_pressed(key, modifiers, text.as_deref(), repeat, status),
            _ => Task::none(),
        }
    }

    fn window_unfocused(&mut self) -> Task<Message> {
        self.window_drag = WindowDrag::Idle;
        self.modifiers = keyboard::Modifiers::empty();
        self.clear_context_menus();
        self.clear_trash_drafts();
        self.type_jump.dismiss_session();
        self.clear_motion_reveals();
        for state in self.views.values_mut() {
            state.earlier_ancestors = false;
        }
        self.pointer = None;
        self.cancel_card_drag();
        self.focused_path = None;
        self.focus_version = self.focus_version.saturating_add(1);
        self.dispatch(Action::CancelPointer)
    }

    #[cfg(test)]
    pub(crate) fn key(
        &mut self,
        key: Key,
        modifiers: keyboard::Modifiers,
        status: Status,
    ) -> Task<Message> {
        self.key_pressed(key, modifiers, None, false, status)
    }

    pub(crate) fn key_pressed(
        &mut self,
        key: Key,
        modifiers: keyboard::Modifiers,
        text: Option<&str>,
        repeat: bool,
        status: Status,
    ) -> Task<Message> {
        if repeat {
            let now = std::time::Instant::now();
            if status == Status::Captured
                || modifiers.command()
                || modifiers.control()
                || modifiers.logo()
                || !self.type_jump_allowed()
                || !self.type_jump.has_active_session_at(now)
            {
                return Task::none();
            }
            if !modifiers.alt() && key == Key::Named(key::Named::Backspace) {
                return self.type_jump_backspace(now);
            }
            if let Some(printable) = extract_jump_text(&key, modifiers, text) {
                return self.type_jump_input(&printable, now);
            }
            return Task::none();
        }
        if key == Key::Named(key::Named::Escape) {
            return self.handle_escape_key(status);
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
        if modifiers.command() || modifiers.control() || modifiers.logo() {
            return self.handle_shortcut_key(&key, modifiers);
        }
        if modifiers.is_empty() && key == Key::Named(key::Named::Enter) {
            return self.handle_enter_key();
        }
        if self.type_jump_allowed() {
            if !modifiers.alt() && key == Key::Named(key::Named::Backspace) {
                return self.type_jump_backspace(std::time::Instant::now());
            }
            if let Some(printable) = extract_jump_text(&key, modifiers, text) {
                return self.type_jump_input(&printable, std::time::Instant::now());
            }
        }
        Task::none()
    }

    fn handle_escape_key(&mut self, status: Status) -> Task<Message> {
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
        let can_show: Vec<ViewId> = self
            .views
            .iter()
            .filter_map(|(&view, state)| {
                (state.earlier_ancestors && self.can_show_earlier_ancestors(view)).then_some(view)
            })
            .collect();
        for state in self.views.values_mut() {
            state.earlier_ancestors = false;
        }
        if !can_show.is_empty() {
            return self.dispatch(Action::Escape);
        }
        let now = std::time::Instant::now();
        self.type_jump.expire_if_elapsed(now);
        if self.type_jump.has_active_session_at(now) {
            self.type_jump.dismiss_session();
            return self.dispatch(Action::Escape);
        }
        if self.file_drag.has_pending_or_active()
            || !self.file_drag.external_paths.is_empty()
            || self.tab_drag.is_some()
        {
            self.file_drag.clear();
            self.tab_drag = None;
            return self.dispatch(Action::Escape);
        }
        self.deactivate_all_views();
        Task::batch([self.dispatch(Action::Escape), self.focus_canvas()])
    }

    fn handle_shortcut_key(&mut self, key: &Key, modifiers: keyboard::Modifiers) -> Task<Message> {
        if self.launchers.draft.is_some() || self.file_editor_active() {
            return Task::none();
        }
        if let Some(task) = self.file_key(key, modifiers) {
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
        Task::none()
    }

    fn handle_enter_key(&mut self) -> Task<Message> {
        if !self.type_jump_allowed() {
            return Task::none();
        }
        let now = std::time::Instant::now();
        self.type_jump.expire_if_elapsed(now);
        if self.type_jump.has_zero_match_session(now) {
            self.type_jump.dismiss_session();
            return Task::none();
        }
        self.type_jump.dismiss_session();
        if let Some(view) = self.active_view {
            if !self.application.workspace().can_browse(view) {
                return Task::none();
            }
            if self.preview_target_active(view) {
                return self
                    .column_previews
                    .selected_entry(view)
                    .map_or_else(Task::none, |(_, path)| {
                        self.open_column_preview_entry(view, path)
                    });
            }
            let selected = self
                .views
                .get(&view)
                .and_then(|presentation| presentation.selected.clone());
            return selected.map_or_else(Task::none, |path| self.open_entry(view, path));
        }
        if let Some((winner_view, winner_path)) = self.type_jump.single_all_boxes_winner()
            && self.application.workspace().can_browse(winner_view)
            && self.visible_entry(winner_view, &winner_path).is_some()
        {
            self.activate_view(winner_view);
            return self.open_entry(winner_view, winner_path);
        }
        Task::none()
    }

    pub(crate) fn ancestor_popup_open(&self) -> bool {
        self.views
            .iter()
            .any(|(&view, state)| state.earlier_ancestors && self.can_show_earlier_ancestors(view))
    }

    pub(crate) fn type_jump_allowed(&self) -> bool {
        self.application.workspace().mode() == Mode::Browse
            && self.workspace_input_enabled()
            && self.window_drag == WindowDrag::Idle
            && self.tab_drag.is_none()
            && self.box_settings.is_none()
            && self.launchers.draft.is_none()
            && !self.file_drag.has_pending_or_active()
            && self.file_drag.external_paths.is_empty()
            && self.file_drag.outbound_session.is_none()
            && !self.settings_picker.pending()
            && self.focused_path.is_none()
            && !self.file_editor_active()
            && !self.ancestor_popup_open()
    }
}

fn extract_jump_text(
    key: &Key,
    modifiers: keyboard::Modifiers,
    text: Option<&str>,
) -> Option<String> {
    if modifiers.command() || modifiers.control() || modifiers.logo() {
        return None;
    }
    if let Key::Named(named) = key.as_ref() {
        return if named == key::Named::Space && !modifiers.alt() {
            Some(" ".to_owned())
        } else {
            None
        };
    }
    if let Some(raw) = text {
        let cleaned: String = raw.chars().filter(|ch| !ch.is_control()).collect();
        if !cleaned.is_empty() {
            return Some(cleaned);
        }
    }
    if !modifiers.alt()
        && let Key::Character(raw) = key.as_ref()
    {
        let cleaned: String = if modifiers.shift() {
            raw.chars()
                .filter(|ch| !ch.is_control())
                .flat_map(char::to_uppercase)
                .collect()
        } else {
            raw.chars().filter(|ch| !ch.is_control()).collect()
        };
        if !cleaned.is_empty() {
            return Some(cleaned);
        }
    }
    None
}
