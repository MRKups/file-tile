//! Browse-mode `Cmd+F` global Search overlay state, keyboard routing, and debounced dispatch.

mod view;

use std::time::{Duration, Instant};

use iced::event::Status;
use iced::keyboard::{self, Key};
use iced::widget::operation;
use iced::{Event, Task};

use crate::application::files::search::{
    OpenTilesMatches, SearchResult, SystemSearchStatus, meets_system_query_minimum, normalize_query,
};
use crate::application::{Action as AppAction, Application};
use crate::domain::{Mode, ViewId};
use crate::runtime::input::WindowDrag;
use crate::{Desktop, Message};

pub(crate) use view::view;

const DEBOUNCE_DELAY: Duration = Duration::from_millis(135);
const INPUT_ID: &str = "global-search-input";
pub(super) const INITIAL_VISIBLE_RESULTS: usize = 25;
pub(super) const EXPAND_RESULTS_STEP: usize = 50;

#[derive(Debug)]
pub(crate) struct State {
    pub(crate) query: String,
    pub(crate) open_tiles: OpenTilesMatches,
    pub(super) open_tiles_limit: usize,
    pub(super) system_limit: usize,
    pub(super) selected_index: usize,
    pub(super) debounce_deadline: Option<Instant>,
    pub(super) hide_open_error: bool,
    pub(super) return_focus: Option<ViewId>,
}

impl State {
    fn new(return_focus: Option<ViewId>) -> Self {
        Self {
            query: String::new(),
            open_tiles: OpenTilesMatches::default(),
            open_tiles_limit: INITIAL_VISIBLE_RESULTS,
            system_limit: INITIAL_VISIBLE_RESULTS,
            selected_index: 0,
            debounce_deadline: None,
            hide_open_error: false,
            return_focus,
        }
    }

    pub(crate) fn has_query(&self) -> bool {
        !normalize_query(&self.query).is_empty()
    }

    pub(super) fn visible_open_tiles(&self) -> &[SearchResult] {
        let count = self.open_tiles.results.len().min(self.open_tiles_limit);
        &self.open_tiles.results[..count]
    }

    pub(super) fn visible_system_results<'a>(
        &self,
        application: &'a Application,
    ) -> &'a [SearchResult] {
        if !meets_system_query_minimum(&self.query) || self.debounce_deadline.is_some() {
            return &[];
        }
        let all = system_results(application);
        let count = all.len().min(self.system_limit);
        &all[..count]
    }

    pub(crate) fn clamp_selection(&mut self, application: &Application) {
        let total = self.total_results(application);
        if total == 0 {
            self.selected_index = 0;
        } else if self.selected_index >= total {
            self.selected_index = total - 1;
        }
    }

    fn total_results(&self, application: &Application) -> usize {
        if !self.has_query() {
            return 0;
        }
        self.visible_open_tiles().len() + self.visible_system_results(application).len()
    }

    fn result_at<'a>(
        &'a self,
        application: &'a Application,
        index: usize,
    ) -> Option<&'a SearchResult> {
        let visible_open = self.visible_open_tiles();
        if index < visible_open.len() {
            visible_open.get(index)
        } else {
            self.visible_system_results(application)
                .get(index - visible_open.len())
        }
    }
}

fn system_results(application: &Application) -> &[SearchResult] {
    match application.system_search_status() {
        SystemSearchStatus::Ready { results, .. } => results,
        _ => &[],
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Action {
    Open,
    Close,
    QueryChanged(String),
    DebounceTick(Instant),
    ShowMoreOpenTiles,
    ShowMoreThisMac,
    ActivateSelected,
    ActivateIndex(usize),
}

impl Desktop {
    pub(crate) fn can_open_search(&self, allow_dismiss_menus: bool) -> bool {
        self.application.workspace().mode() == Mode::Browse
            && self.application.accepts_workspace_input()
            && self.application.file_operation().is_none()
            && !self.modal_open()
            && (allow_dismiss_menus || !self.context_menu_open())
            && self.box_settings.is_none()
            && self.launchers.draft.is_none()
            && !self.settings_picker.pending()
            && !self.file_editor_active()
            && matches!(self.window_drag, WindowDrag::Idle)
            && self.tab_drag.is_none()
            && self.sidebar_drag.is_none()
            && !self.file_drag.has_pending_or_active()
            && self.file_drag.external_paths.is_empty()
            && self.file_drag.outbound_session.is_none()
    }

    pub(crate) fn open_search(&mut self) -> Task<Message> {
        if !self.can_open_search(true) {
            return Task::none();
        }
        self.clear_context_menus();
        self.type_jump.dismiss_session();
        self.pending_search = None;
        self.pending_search_open = None;
        self.application.update(AppAction::CancelSearch);
        self.pointer = None;
        for presentation in self.views.values_mut() {
            presentation.hovered = None;
        }
        let return_focus = self.focused_path.take();
        self.focus_version = self.focus_version.saturating_add(1);
        self.search = Some(State::new(return_focus));
        operation::focus(INPUT_ID)
    }

    pub(crate) fn close_search(&mut self) -> Task<Message> {
        self.pending_search = None;
        self.pending_search_open = None;
        let return_focus = self.search.take().and_then(|state| state.return_focus);
        self.application.update(AppAction::CancelSearch);
        self.restore_workspace_focus(return_focus)
    }

    pub(crate) fn search_message(&mut self, action: Action) -> Task<Message> {
        match action {
            Action::Open => self.open_search(),
            Action::Close => self.close_search(),
            Action::QueryChanged(query) => self.search_query_changed(query),
            Action::DebounceTick(now) => self.search_debounce_tick(now),
            Action::ShowMoreOpenTiles => self.show_more_open_tiles(),
            Action::ShowMoreThisMac => self.show_more_this_mac(),
            Action::ActivateSelected => {
                let Some(selected) = self.search.as_ref().map(|state| state.selected_index) else {
                    return Task::none();
                };
                self.activate_search_index(selected)
            }
            Action::ActivateIndex(index) => self.activate_search_index(index),
        }
    }

    fn search_query_changed(&mut self, query: String) -> Task<Message> {
        let open_tiles = self.application.search_open_tiles(&query);
        let Some(state) = &mut self.search else {
            return Task::none();
        };
        state.query = query;
        state.open_tiles = open_tiles;
        state.open_tiles_limit = INITIAL_VISIBLE_RESULTS;
        state.system_limit = INITIAL_VISIBLE_RESULTS;
        state.selected_index = 0;
        state.hide_open_error = true;
        self.pending_search = None;
        if !state.has_query() {
            state.debounce_deadline = None;
            self.application.update(AppAction::CancelSearch);
            return Task::none();
        }
        if !meets_system_query_minimum(&state.query) {
            state.debounce_deadline = None;
            self.application.update(AppAction::CancelSearch);
            return view::reset_scroll();
        }
        state.debounce_deadline = Some(Instant::now() + DEBOUNCE_DELAY);
        view::reset_scroll()
    }

    fn show_more_open_tiles(&mut self) -> Task<Message> {
        let Some(state) = &mut self.search else {
            return Task::none();
        };
        let old_open = state.visible_open_tiles().len();
        state.open_tiles_limit = state.open_tiles_limit.saturating_add(EXPAND_RESULTS_STEP);
        let new_open = state.visible_open_tiles().len();
        if state.selected_index >= old_open {
            state.selected_index = state
                .selected_index
                .saturating_add(new_open.saturating_sub(old_open));
        }
        state.clamp_selection(&self.application);
        operation::focus(INPUT_ID)
    }

    fn show_more_this_mac(&mut self) -> Task<Message> {
        let Some(state) = &mut self.search else {
            return Task::none();
        };
        state.system_limit = state.system_limit.saturating_add(EXPAND_RESULTS_STEP);
        state.clamp_selection(&self.application);
        operation::focus(INPUT_ID)
    }

    fn search_debounce_tick(&mut self, now: Instant) -> Task<Message> {
        let Some(state) = &mut self.search else {
            return Task::none();
        };
        if !state
            .debounce_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            return Task::none();
        }
        state.debounce_deadline = None;
        let query = state.query.clone();
        self.dispatch(AppAction::SearchSystem { query })
    }

    fn step_search_selection(&mut self, delta: isize) -> Task<Message> {
        let Some(state) = &mut self.search else {
            return Task::none();
        };
        let total = state.total_results(&self.application);
        if total == 0 {
            state.selected_index = 0;
            return Task::none();
        }
        state.hide_open_error = true;
        state.selected_index = if delta < 0 {
            state.selected_index.saturating_sub((-delta) as usize)
        } else {
            state
                .selected_index
                .saturating_add(delta as usize)
                .min(total - 1)
        };
        view::reveal_selected()
    }

    fn activate_search_index(&mut self, index: usize) -> Task<Message> {
        if self.application.search_opening_busy() {
            return Task::none();
        }
        let Some(state) = &mut self.search else {
            return Task::none();
        };
        let Some(item) = state.result_at(&self.application, index).cloned() else {
            return Task::none();
        };
        state.hide_open_error = false;
        state.selected_index = index;
        self.dispatch(AppAction::OpenSearchResult {
            path: item.path,
            kind: item.kind,
        })
    }

    pub(crate) fn search_input(&mut self, event: Event, status: Status) -> Task<Message> {
        let Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modifiers,
            repeat,
            ..
        }) = event
        else {
            return Task::none();
        };
        match key {
            Key::Named(keyboard::key::Named::Escape) if modifiers.is_empty() && !repeat => {
                return self.close_search();
            }
            Key::Character(ref ch)
                if !repeat
                    && !modifiers.shift()
                    && !modifiers.alt()
                    && (modifiers.command() || modifiers.control())
                    && ch.eq_ignore_ascii_case("f") =>
            {
                return operation::focus(INPUT_ID);
            }
            Key::Named(keyboard::key::Named::ArrowUp)
                if status == Status::Ignored && modifiers.is_empty() =>
            {
                return self.step_search_selection(-1);
            }
            Key::Named(keyboard::key::Named::ArrowDown)
                if status == Status::Ignored && modifiers.is_empty() =>
            {
                return self.step_search_selection(1);
            }
            Key::Named(keyboard::key::Named::Enter)
                if status == Status::Ignored && modifiers.is_empty() && !repeat =>
            {
                return self.search_message(Action::ActivateSelected);
            }
            Key::Named(keyboard::key::Named::Tab) if modifiers.is_empty() && !repeat => {
                return operation::focus_next();
            }
            Key::Named(keyboard::key::Named::Tab)
                if modifiers == keyboard::Modifiers::SHIFT && !repeat =>
            {
                return operation::focus_previous();
            }
            _ => {}
        }
        Task::none()
    }
}
