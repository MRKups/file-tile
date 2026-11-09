//! Transient Browse type-to-jump session state, single-pane and all-boxes evaluation,
//! and programmatic row/tile reveal without arming file drag.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use iced::Task;

use crate::application::LoadState;
use crate::domain::matching::{
    collapsed_repeat_unit, count_matches, has_prefix_match, pick_best_match, pick_cycled_match,
    select_jump_match,
};
use crate::domain::{FileView, ViewId};
use crate::{
    Desktop, Message,
    boxes::{file_grid, file_list, folder_box},
    widgets::keystroke_hud,
    window::appearance as a,
};

pub(crate) const HOLD_DURATION: Duration = Duration::from_millis(900);
pub(crate) const FADE_DURATION: Duration = Duration::from_millis(180);
const MAX_QUERY_CHARS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
enum SessionTarget {
    AllBoxes,
    Primary { view: ViewId, directory: PathBuf },
    ColumnPreview { view: ViewId, directory: PathBuf },
}

#[derive(Debug, Clone)]
struct Session {
    target: SessionTarget,
    query: String,
    cycled_single_char: bool,
    match_count: usize,
    fade_start: Instant,
    fade_started_at: Option<Instant>,
    fade: f32,
}

#[derive(Debug, Default)]
pub(crate) struct State {
    session: Option<Session>,
    all_boxes_matches: HashMap<ViewId, PathBuf>,
}

impl State {
    pub(crate) fn expire_if_elapsed(&mut self, now: Instant) {
        if self
            .session
            .as_ref()
            .is_some_and(|session| now >= session.fade_start + FADE_DURATION)
        {
            self.session = None;
        }
    }

    pub(crate) fn has_active_session_at(&self, now: Instant) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| now < session.fade_start + FADE_DURATION)
    }

    pub(crate) fn has_zero_match_session(&self, now: Instant) -> bool {
        self.session.as_ref().is_some_and(|session| {
            now < session.fade_start + FADE_DURATION && session.match_count == 0
        })
    }

    pub(crate) fn is_fading(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| session.fade_started_at.is_some())
    }

    pub(crate) fn dismiss_session(&mut self) {
        self.session = None;
    }

    pub(crate) fn begin_fade(&mut self, now: Instant) {
        if let Some(session) = &mut self.session
            && session.fade_started_at.is_none()
            && now >= session.fade_start
        {
            session.fade_started_at = Some(session.fade_start);
            self.tick(now);
        }
    }

    pub(crate) fn tick(&mut self, now: Instant) {
        let Some(session) = &mut self.session else {
            return;
        };
        if session.fade_started_at.is_none() && now >= session.fade_start {
            session.fade_started_at = Some(session.fade_start);
        }
        let Some(started) = session.fade_started_at else {
            return;
        };
        let elapsed = now.saturating_duration_since(started).as_secs_f32();
        let total = FADE_DURATION.as_secs_f32();
        if elapsed >= total {
            self.session = None;
        } else {
            let u = (elapsed / total).clamp(0.0, 1.0);
            session.fade = 0.5 * (1.0 + (std::f32::consts::PI * u).cos());
        }
    }

    pub(crate) fn single_all_boxes_winner(&self) -> Option<(ViewId, PathBuf)> {
        if self.all_boxes_matches.len() != 1 {
            return None;
        }
        self.all_boxes_matches
            .iter()
            .next()
            .map(|(view, path)| (*view, path.clone()))
    }

    pub(crate) fn target_view(&self) -> Option<ViewId> {
        match &self.session.as_ref()?.target {
            SessionTarget::AllBoxes => None,
            SessionTarget::Primary { view, .. } | SessionTarget::ColumnPreview { view, .. } => {
                Some(*view)
            }
        }
    }

    pub(crate) fn hud_props(
        &self,
        placement: keystroke_hud::Placement,
        glow_clip: Option<iced::Rectangle>,
        matched_boxes: Vec<iced::Rectangle>,
        palette: a::Palette,
    ) -> Option<keystroke_hud::Props<'_>> {
        let session = self.session.as_ref()?;
        Some(keystroke_hud::Props {
            query: &session.query,
            match_count: session.match_count,
            placement,
            glow_clip,
            matched_boxes,
            fade: session.fade,
            fade_start: session.fade_start,
            fading: session.fade_started_at.is_some(),
            on_fade_start: Message::TypeJumpBeginFade,
            palette,
        })
    }
}

fn clamp_query(raw: &str) -> String {
    raw.chars().take(MAX_QUERY_CHARS).collect()
}

fn same_single_char_as_session(session: &Session, appended: &str) -> bool {
    if session.query.chars().count() != 1 || appended.chars().count() != 1 {
        return false;
    }
    let combined = format!("{}{appended}", session.query);
    collapsed_repeat_unit(&combined).is_some()
}

struct EntryScrollBounds {
    top: f32,
    bottom: f32,
    content_height: f32,
    viewport_height: f32,
    has_motion: bool,
}

impl Desktop {
    pub(crate) fn type_jump_hud_placement(&self) -> keystroke_hud::Placement {
        let Some(view) = self.type_jump.target_view() else {
            return keystroke_hud::Placement::Window;
        };
        let Some(item) = self.application.workspace().box_for_view(view) else {
            return keystroke_hud::Placement::Hidden;
        };
        let geometry = self.geometry();
        let scroller = folder_box::scroller_geometry(geometry, item);
        let box_rect = geometry.rect(item.rect);
        let projected = iced::Rectangle {
            x: box_rect.x + geometry.viewport.x - geometry.offset.x,
            y: box_rect.y + geometry.viewport.y - geometry.offset.y,
            width: box_rect.width,
            height: box_rect.height,
        };
        let Some(visible) = projected.intersection(&geometry.viewport) else {
            return keystroke_hud::Placement::Hidden;
        };
        if visible.width < 56.0 || visible.height < 36.0 {
            return keystroke_hud::Placement::Hidden;
        }
        let min_bottom = if scroller.compact { 6.0 } else { 10.0 };
        let desired_bottom = if scroller.compact {
            8.0
        } else {
            (scroller.footer_height + 10.0).min((projected.height - 36.0).max(8.0))
        };
        let bottom_clipped = ((projected.y + projected.height)
            - (geometry.viewport.y + geometry.viewport.height))
            .max(0.0);
        let bottom_offset = (desired_bottom - bottom_clipped)
            .max(min_bottom)
            .min((visible.height - 32.0).max(0.0));
        keystroke_hud::Placement::InBox {
            bounds: visible,
            bottom_offset,
        }
    }

    pub(crate) fn type_jump_matched_boxes(
        &self,
    ) -> (Option<iced::Rectangle>, Vec<iced::Rectangle>) {
        if !self
            .type_jump
            .session
            .as_ref()
            .is_some_and(|session| session.target == SessionTarget::AllBoxes)
            || self.type_jump.all_boxes_matches.is_empty()
        {
            return (None, Vec::new());
        }
        let geometry = self.geometry();
        let mut views: Vec<ViewId> = self.type_jump.all_boxes_matches.keys().copied().collect();
        views.sort_by_key(|view| view.0);
        let mut boxes = Vec::with_capacity(views.len());
        for view in views {
            let Some(item) = self.application.workspace().box_for_view(view) else {
                continue;
            };
            let box_rect = geometry.rect(item.rect);
            let projected = iced::Rectangle {
                x: box_rect.x + geometry.viewport.x - geometry.offset.x,
                y: box_rect.y + geometry.viewport.y - geometry.offset.y,
                width: box_rect.width,
                height: box_rect.height,
            };
            if projected.intersection(&geometry.viewport).is_some() {
                boxes.push(projected);
            }
        }
        if boxes.is_empty() {
            return (None, Vec::new());
        }
        let pad = crate::canvas::geometry::GUTTER;
        let surface_size = geometry.size();
        let top_pad = if geometry.offset.y <= 0.5 { pad } else { 0.0 };
        let left_pad = if geometry.offset.x <= 0.5 { pad } else { 0.0 };
        let right_pad = if geometry.offset.x + geometry.viewport.width >= surface_size.width - 0.5 {
            pad
        } else {
            0.0
        };
        let bottom_pad =
            if geometry.offset.y + geometry.viewport.height >= surface_size.height - 0.5 {
                pad
            } else {
                0.0
            };
        let clip_x = (geometry.viewport.x - left_pad).max(0.0);
        let clip_y = (geometry.viewport.y - top_pad).max(geometry.header_height);
        let clip_right = geometry.viewport.x + geometry.viewport.width + right_pad;
        let clip_bottom = geometry.viewport.y + geometry.viewport.height + bottom_pad;
        let glow_clip = iced::Rectangle {
            x: clip_x,
            y: clip_y,
            width: (clip_right - clip_x).max(0.0),
            height: (clip_bottom - clip_y).max(0.0),
        };
        (Some(glow_clip), boxes)
    }

    fn count_session_matches(&self, target: &SessionTarget, query: &str) -> usize {
        if query.is_empty() {
            return 0;
        }
        match target {
            SessionTarget::AllBoxes => self
                .application
                .browsable_box_views()
                .into_iter()
                .map(|(_, view)| {
                    let candidates = self.application.jump_candidates(view);
                    count_matches(
                        candidates.iter().map(|cand| cand.entry.name.as_os_str()),
                        query,
                    )
                })
                .sum(),
            SessionTarget::Primary { view, .. } => {
                let candidates = self.application.jump_candidates(*view);
                count_matches(
                    candidates.iter().map(|cand| cand.entry.name.as_os_str()),
                    query,
                )
            }
            SessionTarget::ColumnPreview { view, .. } => {
                let Some(scope) = self
                    .application
                    .workspace()
                    .view(*view)
                    .map(|item| item.navigation.scope().clone())
                else {
                    return 0;
                };
                let candidates = self.column_previews.jump_candidates(*view, &scope);
                count_matches(
                    candidates.iter().map(|cand| cand.entry.name.as_os_str()),
                    query,
                )
            }
        }
    }

    pub(crate) fn activate_view(&mut self, view: ViewId) {
        self.active_view = Some(view);
        self.column_previews.set_focused(None);
        self.column_previews.clear_selection(view);
        for (&other, state) in &mut self.views {
            if other != view {
                state.earlier_ancestors = false;
            }
        }
        self.clear_other_all_boxes_matches(Some(view));
    }

    pub(crate) fn activate_column_preview(&mut self, view: ViewId) {
        self.active_view = Some(view);
        self.column_previews.set_focused(Some(view));
        for state in self.views.values_mut() {
            state.earlier_ancestors = false;
        }
        self.clear_other_all_boxes_matches(Some(view));
    }

    pub(crate) fn deactivate_all_views(&mut self) {
        self.active_view = None;
        self.column_previews.set_focused(None);
        for state in self.views.values_mut() {
            state.earlier_ancestors = false;
        }
        self.type_jump.dismiss_session();
        self.clear_other_all_boxes_matches(None);
    }

    pub(crate) fn clear_other_all_boxes_matches(&mut self, keep: Option<ViewId>) {
        let previous = std::mem::take(&mut self.type_jump.all_boxes_matches);
        for (view, matched_path) in previous {
            if Some(view) == keep {
                continue;
            }
            if let Some(state) = self.views.get_mut(&view)
                && state.selected.as_ref() == Some(&matched_path)
            {
                state.selected = None;
                state.selection.clear();
                state.selection_anchor = None;
                state.reveal_after_motion = false;
            }
        }
    }

    pub(crate) fn preview_target_active(&self, view: ViewId) -> bool {
        if self.active_view != Some(view) || self.column_previews.retained_listing(view).is_none() {
            return false;
        }
        let Some(item) = self.application.workspace().box_for_view(view) else {
            return false;
        };
        let geom = folder_box::scroller_geometry(self.geometry(), item);
        geom.two_columns
            && (self.column_previews.is_focused(view)
                || self.column_previews.selected_entry(view).is_some())
    }

    fn current_jump_target(&self) -> Option<SessionTarget> {
        let Some(view) = self.active_view else {
            return Some(SessionTarget::AllBoxes);
        };
        if !self.application.workspace().can_browse(view) {
            return None;
        }
        if self.preview_target_active(view) {
            let directory = self.column_previews.get(view)?.directory.clone();
            return Some(SessionTarget::ColumnPreview { view, directory });
        }
        let directory = self
            .application
            .workspace()
            .view(view)?
            .navigation
            .current()
            .to_path_buf();
        Some(SessionTarget::Primary { view, directory })
    }

    fn session_target_still_valid(&self, target: &SessionTarget) -> bool {
        match target {
            SessionTarget::AllBoxes => self.active_view.is_none(),
            SessionTarget::Primary { view, directory } => {
                self.active_view == Some(*view)
                    && !self.preview_target_active(*view)
                    && self.application.workspace().can_browse(*view)
                    && self
                        .application
                        .workspace()
                        .view(*view)
                        .is_some_and(|item| item.navigation.current() == directory)
            }
            SessionTarget::ColumnPreview { view, directory } => {
                self.preview_target_active(*view)
                    && self
                        .column_previews
                        .get(*view)
                        .is_some_and(|preview| &preview.directory == directory)
            }
        }
    }

    fn entry_scroll_bounds(&self, view: ViewId, display_index: usize) -> Option<EntryScrollBounds> {
        let item = self.application.workspace().box_for_view(view)?;
        let listing = self
            .application
            .load(view)
            .and_then(LoadState::retained_listing)?;
        let geometry = self.geometry();
        let scroller = folder_box::scroller_geometry(geometry, item);
        let ty = geometry.typography;
        let show_hidden = item.settings.show_hidden;
        let order = self.application.sort_indices(view);
        let motion = self
            .views
            .get(&view)
            .and_then(|state| state.motion.as_ref());
        let (raw_top, raw_bottom, raw_content_h) = if item.settings.file_view == FileView::Grid {
            file_grid::entry_vertical_bounds(
                display_index,
                scroller.content_width,
                &listing,
                order,
                show_hidden,
                motion,
                ty,
            )
        } else {
            file_list::entry_vertical_bounds(
                display_index,
                &listing,
                order,
                show_hidden,
                motion,
                ty,
            )
        };
        let (top, bottom, content_height) = if scroller.compact {
            (
                raw_top + scroller.nav_height,
                raw_bottom + scroller.nav_height,
                raw_content_h + scroller.nav_height + scroller.footer_height,
            )
        } else {
            (raw_top, raw_bottom, raw_content_h)
        };
        let has_motion = if item.settings.file_view == FileView::Grid {
            motion.is_some_and(crate::boxes::entry_motion::ViewMotion::has_deletions)
        } else {
            motion.is_some_and(crate::boxes::entry_motion::ViewMotion::has_layout_motion)
        };
        Some(EntryScrollBounds {
            top,
            bottom,
            content_height,
            viewport_height: scroller.viewport_height,
            has_motion,
        })
    }

    pub(crate) fn jump_to_entry(
        &mut self,
        view: ViewId,
        path: PathBuf,
        display_index: usize,
    ) -> Task<Message> {
        let Some(bounds) = self.entry_scroll_bounds(view, display_index) else {
            return Task::none();
        };
        let presentation = self.views.entry(view).or_default();
        presentation.pending_selection.clear();
        presentation.reveal_selected = false;
        presentation.reveal_after_motion = bounds.has_motion;
        presentation.selection.clear();
        presentation.selection.insert(path.clone());
        presentation.selection_anchor = Some(path.clone());
        presentation.selected = Some(path.clone());
        presentation.hint = None;
        let next_offset = file_list::reveal_scroll_offset(
            presentation.scroll.offset,
            bounds.viewport_height,
            bounds.content_height,
            bounds.top,
            bounds.bottom,
        );
        presentation.scroll = file_list::ScrollWindow {
            offset: next_offset,
            height: bounds.viewport_height,
        };
        let generation = presentation.listing_revision;
        self.column_previews.clear_selection(view);
        if self.column_previews.is_focused(view) {
            self.column_previews.set_focused(None);
        }
        self.demand_single_thumbnail(view, &path);
        Task::batch([
            file_list::reveal_scroll(view, generation, bounds.top, bounds.bottom),
            self.fill_thumbnails(),
        ])
    }

    pub(crate) fn clear_motion_reveals(&mut self) {
        for state in self.views.values_mut() {
            state.reveal_after_motion = false;
        }
    }

    pub(crate) fn reveal_selected_entry(&mut self, view: ViewId) -> Task<Message> {
        if !self.workspace_input_enabled()
            || self.application.workspace().mode() != crate::domain::Mode::Browse
            || !self.application.workspace().can_browse(view)
        {
            return Task::none();
        }
        let Some(selected) = self
            .views
            .get(&view)
            .and_then(|state| state.selected.clone())
        else {
            return Task::none();
        };
        let Some(display_index) = self
            .application
            .ordered_entries(view)
            .position(|entry| entry.path == selected)
        else {
            return Task::none();
        };
        let Some(bounds) = self.entry_scroll_bounds(view, display_index) else {
            return Task::none();
        };
        let presentation = self.views.entry(view).or_default();
        let next_offset = file_list::reveal_scroll_offset(
            presentation.scroll.offset,
            bounds.viewport_height,
            bounds.content_height,
            bounds.top,
            bounds.bottom,
        );
        presentation.scroll = file_list::ScrollWindow {
            offset: next_offset,
            height: bounds.viewport_height,
        };
        let generation = presentation.listing_revision;
        file_list::reveal_scroll(view, generation, bounds.top, bounds.bottom)
    }

    pub(crate) fn type_jump_input(&mut self, appended: &str, now: Instant) -> Task<Message> {
        if appended.is_empty() {
            return Task::none();
        }
        self.type_jump.expire_if_elapsed(now);
        let Some(target) = self.current_jump_target() else {
            return Task::none();
        };
        let (proposed, force_cycle, fresh_session) = match &self.type_jump.session {
            Some(prev) if prev.target == target => {
                if prev.cycled_single_char && same_single_char_as_session(prev, appended) {
                    (prev.query.clone(), true, false)
                } else {
                    (
                        clamp_query(&format!("{}{appended}", prev.query)),
                        false,
                        false,
                    )
                }
            }
            _ => (clamp_query(appended), false, true),
        };
        let jump_task = match target {
            SessionTarget::AllBoxes => {
                if fresh_session {
                    self.clear_browsable_selections_for_all_boxes();
                }
                self.evaluate_all_boxes_jump(proposed, force_cycle, false, now)
            }
            SessionTarget::Primary { view, directory } => {
                self.evaluate_primary_jump(view, directory, proposed, force_cycle, false, now)
            }
            SessionTarget::ColumnPreview { view, directory } => {
                self.evaluate_preview_jump(view, directory, proposed, force_cycle, false, now)
            }
        };
        Task::batch([jump_task, self.focus_canvas()])
    }

    pub(crate) fn type_jump_backspace(&mut self, now: Instant) -> Task<Message> {
        self.type_jump.expire_if_elapsed(now);
        let Some(prev) = self.type_jump.session.clone() else {
            return Task::none();
        };
        let Some(target) = self.current_jump_target() else {
            self.type_jump.dismiss_session();
            return Task::none();
        };
        if prev.target != target {
            self.type_jump.dismiss_session();
            return Task::none();
        }
        let mut shortened = prev.query;
        shortened.pop();
        if shortened.is_empty() {
            self.type_jump.dismiss_session();
            if target == SessionTarget::AllBoxes {
                self.clear_other_all_boxes_matches(None);
            }
            return self.focus_canvas();
        }
        let jump_task = match target {
            SessionTarget::AllBoxes => self.evaluate_all_boxes_jump(shortened, false, true, now),
            SessionTarget::Primary { view, directory } => {
                self.evaluate_primary_jump(view, directory, shortened, false, true, now)
            }
            SessionTarget::ColumnPreview { view, directory } => {
                self.evaluate_preview_jump(view, directory, shortened, false, true, now)
            }
        };
        Task::batch([jump_task, self.focus_canvas()])
    }

    fn evaluate_primary_jump(
        &mut self,
        view: ViewId,
        directory: PathBuf,
        proposed: String,
        force_cycle: bool,
        from_top: bool,
        now: Instant,
    ) -> Task<Message> {
        self.clear_other_all_boxes_matches(None);
        let selected_now = self
            .views
            .get(&view)
            .and_then(|state| state.selected.clone());
        let (pick, chosen) = {
            let candidates = self.application.jump_candidates(view);
            let current = if from_top {
                None
            } else {
                selected_now.as_deref().and_then(|sel| {
                    candidates
                        .iter()
                        .position(|cand| cand.entry.path.as_path() == sel)
                })
            };
            let names = candidates.iter().map(|cand| cand.entry.name.as_os_str());
            let pick = select_jump_match(names, &proposed, force_cycle, current);
            let chosen = pick.and_then(|hit| {
                candidates
                    .get(hit.candidate_index)
                    .map(|cand| (cand.entry.path.clone(), cand.display_index))
            });
            (pick, chosen)
        };
        let target = SessionTarget::Primary { view, directory };
        let (query, cycled, match_count, task) = match (pick, chosen) {
            (Some(hit), Some((path, display_index))) => {
                let effective = if hit.effective_query_collapsed {
                    collapsed_repeat_unit(&proposed)
                        .unwrap_or(&proposed)
                        .to_string()
                } else {
                    proposed
                };
                let total = self.count_session_matches(&target, &effective);
                (
                    effective,
                    hit.effective_query_collapsed,
                    total,
                    self.jump_to_entry(view, path, display_index),
                )
            }
            _ => (proposed, false, 0, Task::none()),
        };
        self.record_jump_session(target, query, cycled, match_count, now);
        task
    }

    fn evaluate_preview_jump(
        &mut self,
        view: ViewId,
        directory: PathBuf,
        proposed: String,
        force_cycle: bool,
        from_top: bool,
        now: Instant,
    ) -> Task<Message> {
        self.clear_other_all_boxes_matches(None);
        let Some(scope) = self
            .application
            .workspace()
            .view(view)
            .map(|item| item.navigation.scope().clone())
        else {
            return Task::none();
        };
        let selected_now = self.column_previews.selected_entry(view).map(|(_, p)| p);
        let (pick, chosen) = {
            let candidates = self.column_previews.jump_candidates(view, &scope);
            let current = if from_top {
                None
            } else {
                selected_now.as_deref().and_then(|sel| {
                    candidates
                        .iter()
                        .position(|cand| cand.entry.path.as_path() == sel)
                })
            };
            let names = candidates.iter().map(|cand| cand.entry.name.as_os_str());
            let pick = select_jump_match(names, &proposed, force_cycle, current);
            let chosen = pick.and_then(|hit| {
                candidates
                    .get(hit.candidate_index)
                    .map(|cand| (cand.entry.path.clone(), cand.display_index))
            });
            (pick, chosen)
        };
        let geometry = self.geometry();
        let viewport_h = self
            .application
            .workspace()
            .box_for_view(view)
            .map_or(0.0, |item| {
                folder_box::scroller_geometry(geometry, item).viewport_height
            });
        let row_h = geometry.typography.row(file_list::ROW_HEIGHT);
        let target = SessionTarget::ColumnPreview { view, directory };
        let (query, cycled, match_count, task) = match (pick, chosen) {
            (Some(hit), Some((path, display_index))) => {
                let effective = if hit.effective_query_collapsed {
                    collapsed_repeat_unit(&proposed)
                        .unwrap_or(&proposed)
                        .to_string()
                } else {
                    proposed
                };
                let total = self.count_session_matches(&target, &effective);
                self.demand_single_thumbnail(view, &path);
                let reveal = self.column_previews.jump_select_and_reveal(
                    view,
                    path,
                    display_index,
                    viewport_h,
                    row_h,
                );
                let task = Task::batch([reveal, self.fill_thumbnails()]);
                (effective, hit.effective_query_collapsed, total, task)
            }
            _ => (proposed, false, 0, Task::none()),
        };
        self.record_jump_session(target, query, cycled, match_count, now);
        task
    }

    fn evaluate_all_boxes_jump(
        &mut self,
        proposed: String,
        force_cycle: bool,
        from_top: bool,
        now: Instant,
    ) -> Task<Message> {
        let views: Vec<ViewId> = self
            .application
            .browsable_box_views()
            .into_iter()
            .map(|(_, view)| view)
            .collect();
        let repeat_unit = if from_top {
            None
        } else {
            collapsed_repeat_unit(&proposed).map(str::to_owned)
        };
        let use_cycle = if force_cycle {
            true
        } else if repeat_unit.is_some() {
            !views.iter().any(|&view| {
                let candidates = self.application.jump_candidates(view);
                has_prefix_match(
                    candidates.iter().map(|cand| cand.entry.name.as_os_str()),
                    &proposed,
                )
            })
        } else {
            false
        };
        let unit_str = repeat_unit.as_deref().unwrap_or(&proposed);
        let mut decisions: Vec<(ViewId, Option<(PathBuf, usize)>)> =
            Vec::with_capacity(views.len());
        for &view in &views {
            let selected_now = self
                .views
                .get(&view)
                .and_then(|state| state.selected.as_deref());
            let candidates = self.application.jump_candidates(view);
            let current = if from_top {
                None
            } else {
                selected_now.and_then(|sel| {
                    candidates
                        .iter()
                        .position(|cand| cand.entry.path.as_path() == sel)
                })
            };
            let names = candidates.iter().map(|cand| cand.entry.name.as_os_str());
            let hit = if use_cycle {
                pick_cycled_match(names, unit_str, current)
            } else {
                pick_best_match(names, &proposed)
            };
            let chosen = hit.and_then(|(idx, _)| {
                candidates
                    .get(idx)
                    .map(|cand| (cand.entry.path.clone(), cand.display_index))
            });
            decisions.push((view, chosen));
        }

        let previous_matches = std::mem::take(&mut self.type_jump.all_boxes_matches);
        let mut new_matches = HashMap::new();
        let mut tasks = Vec::new();
        for (view, chosen) in decisions {
            if let Some((path, display_index)) = chosen {
                new_matches.insert(view, path.clone());
                tasks.push(self.jump_to_entry(view, path, display_index));
            } else if let Some(prev_path) = previous_matches.get(&view)
                && let Some(state) = self.views.get_mut(&view)
                && state.selected.as_ref() == Some(prev_path)
            {
                state.selected = None;
                state.selection.clear();
                state.selection_anchor = None;
                state.reveal_after_motion = false;
            }
        }
        let has_any_box = !new_matches.is_empty();
        self.type_jump.all_boxes_matches = new_matches;
        let (effective_query, cycled) = if use_cycle && has_any_box {
            (unit_str.to_owned(), true)
        } else {
            (proposed, false)
        };
        let match_count = if has_any_box {
            self.count_session_matches(&SessionTarget::AllBoxes, &effective_query)
        } else {
            0
        };
        self.record_jump_session(
            SessionTarget::AllBoxes,
            effective_query,
            cycled,
            match_count,
            now,
        );
        Task::batch(tasks)
    }

    fn clear_browsable_selections_for_all_boxes(&mut self) {
        self.type_jump.all_boxes_matches.clear();
        let views: Vec<ViewId> = self
            .application
            .browsable_box_views()
            .into_iter()
            .map(|(_, view)| view)
            .collect();
        for view in views {
            self.column_previews.clear_selection(view);
            if let Some(state) = self.views.get_mut(&view) {
                state.selected = None;
                state.selection.clear();
                state.selection_anchor = None;
                state.reveal_after_motion = false;
            }
        }
    }

    fn record_jump_session(
        &mut self,
        target: SessionTarget,
        query: String,
        cycled_single_char: bool,
        match_count: usize,
        now: Instant,
    ) {
        self.type_jump.session = Some(Session {
            target,
            query,
            cycled_single_char,
            match_count,
            fade_start: now + HOLD_DURATION,
            fade_started_at: None,
            fade: 1.0,
        });
    }

    fn reconcile_earlier_ancestors(&mut self) {
        let stale: Vec<ViewId> = self
            .views
            .iter()
            .filter_map(|(&view, state)| {
                (state.earlier_ancestors && !self.can_show_earlier_ancestors(view)).then_some(view)
            })
            .collect();
        for view in stale {
            if let Some(state) = self.views.get_mut(&view) {
                state.earlier_ancestors = false;
            }
        }
    }

    pub(crate) fn reconcile_type_jump(&mut self) {
        self.reconcile_earlier_ancestors();
        if self.application.workspace().mode() != crate::domain::Mode::Browse {
            self.type_jump.dismiss_session();
            self.clear_other_all_boxes_matches(None);
            self.clear_motion_reveals();
            return;
        }
        if !self.workspace_input_enabled() {
            self.clear_motion_reveals();
        }
        if let Some(active) = self.active_view
            && !self.type_jump.all_boxes_matches.is_empty()
        {
            self.clear_other_all_boxes_matches(Some(active));
        }
        let mut all_matches = std::mem::take(&mut self.type_jump.all_boxes_matches);
        all_matches.retain(|view, path| {
            self.application.workspace().can_browse(*view)
                && self
                    .views
                    .get(view)
                    .and_then(|state| state.selected.as_ref())
                    == Some(path)
                && self.visible_entry(*view, path).is_some()
        });
        self.type_jump.all_boxes_matches = all_matches;
        let refreshed_count = self
            .type_jump
            .session
            .as_ref()
            .map(|session| self.count_session_matches(&session.target, &session.query));
        if let (Some(session), Some(count)) = (&mut self.type_jump.session, refreshed_count) {
            session.match_count = count;
        }
        let target_invalid = self
            .type_jump
            .session
            .as_ref()
            .is_some_and(|session| !self.session_target_still_valid(&session.target));
        if target_invalid || !self.type_jump_allowed() {
            self.type_jump.dismiss_session();
        }
    }
}
