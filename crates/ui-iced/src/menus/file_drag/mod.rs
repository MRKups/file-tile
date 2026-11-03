//! Internal file drag-and-drop and inbound native Finder drops.

mod geometry;
mod preview;

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use iced::{Point, Rectangle, Task};

use crate::application::Action;
use crate::domain::{BoxId, LauncherConfig, LauncherIcon, LauncherTarget, Mode, ViewId};
use crate::{Desktop, Message};

pub(crate) use preview::view;

const DRAG_THRESHOLD: f32 = 6.0;
const OUTBOUND_EDGE_MARGIN: f32 = 8.0;
const LIFT_DURATION: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum DropTarget {
    FolderBox { view: ViewId, rect: Rectangle },
    Launcher { id: BoxId, rect: Rectangle },
}

impl DropTarget {
    fn rect(self) -> Rectangle {
        match self {
            Self::FolderBox { rect, .. } | Self::Launcher { rect, .. } => rect,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClickRelease {
    Keep,
    NarrowMultiSelection,
    DeselectPrimary,
    DeselectColumnPreview,
}

pub(crate) struct InternalDrag {
    pub source_view: ViewId,
    pub pressed_path: PathBuf,
    pub release: ClickRelease,
    pub paths: Vec<PathBuf>,
    pub path_set: HashSet<PathBuf>,
    layout: geometry::SourceLayout,
    pub origin: Point,
    pub pointer: Point,
    pub active: bool,
    pub activated_at: Option<Instant>,
    pub now: Instant,
    pub target: Option<DropTarget>,
}

impl InternalDrag {
    fn lift_progress(&self) -> f32 {
        let Some(started) = self.activated_at else {
            return 0.0;
        };
        let elapsed = self.now.saturating_duration_since(started);
        if elapsed >= LIFT_DURATION {
            1.0
        } else {
            let u = (elapsed.as_secs_f32() / LIFT_DURATION.as_secs_f32()).clamp(0.0, 1.0);
            1.0 - (1.0 - u).powi(3)
        }
    }
}

#[derive(Default)]
pub(crate) struct State {
    pub internal: Option<InternalDrag>,
    pub outbound_session: Option<(u64, ViewId)>,
    pub external_paths: Vec<PathBuf>,
    pub external_pointer: Option<Point>,
    pub external_target: Option<DropTarget>,
    pub dropped_paths: Vec<PathBuf>,
}

impl State {
    pub(crate) fn is_active(&self) -> bool {
        self.internal.as_ref().is_some_and(|drag| drag.active)
    }

    pub(crate) fn is_animating(&self) -> bool {
        self.internal.as_ref().is_some_and(|drag| {
            drag.active
                && drag.activated_at.is_some_and(|started| {
                    drag.now.saturating_duration_since(started) < LIFT_DURATION
                })
        })
    }

    pub(crate) fn tick(&mut self, now: Instant) {
        if let Some(drag) = &mut self.internal {
            drag.now = now;
        }
    }

    pub(crate) fn dragged_set(&self, view: ViewId) -> Option<&HashSet<PathBuf>> {
        let drag = self.internal.as_ref().filter(|drag| drag.active)?;
        (drag.source_view == view).then_some(&drag.path_set)
    }

    pub(crate) fn has_pending_or_active(&self) -> bool {
        self.internal.is_some()
    }

    pub(crate) fn clear(&mut self) {
        self.internal = None;
        self.outbound_session = None;
        self.clear_external();
        self.dropped_paths.clear();
    }

    pub(crate) fn clear_external(&mut self) {
        self.external_paths.clear();
        self.external_target = None;
    }
}

impl Desktop {
    pub(crate) fn cancel_file_drag_for(&mut self, action: &Action) -> bool {
        if matches!(action, Action::Escape | Action::CancelPointer) && self.file_drag.is_active() {
            self.file_drag.clear();
            return true;
        }
        if matches!(
            action,
            Action::SelectMode(_)
                | Action::Escape
                | Action::CancelPointer
                | Action::Undo
                | Action::RemoveBox(_)
                | Action::RemoveSelected
                | Action::ChangeTabs(..)
                | Action::StartFileOperation { .. }
        ) {
            self.file_drag.clear();
        }
        false
    }

    pub(crate) fn begin_file_drag(
        &mut self,
        view: ViewId,
        pressed_path: PathBuf,
        release: ClickRelease,
    ) {
        #[cfg(target_os = "macos")]
        crate::platform::drag::reset_stale_session();
        self.file_drag.internal = None;
        if !self.workspace_input_enabled()
            || self.application.workspace().mode() != Mode::Browse
            || self.application.files_busy()
            || self.modifiers.command()
            || self.modifiers.shift()
        {
            return;
        }
        let Some(origin) = self.pointer.or_else(|| self.live_cursor_position()) else {
            self.apply_click_release(view, pressed_path, release);
            return;
        };
        self.pointer = Some(origin);
        let paths: Vec<_> = self
            .views
            .get(&view)
            .filter(|state| {
                state.selection.contains(&pressed_path)
                    || state.selected.as_ref() == Some(&pressed_path)
            })
            .map(|state| {
                self.application
                    .ordered_entries(view)
                    .filter(|entry| {
                        state.selection.contains(&entry.path)
                            || state.selected.as_ref() == Some(&entry.path)
                    })
                    .map(|entry| entry.path.clone())
                    .collect()
            })
            .unwrap_or_default();
        let paths = if paths.is_empty() {
            vec![pressed_path.clone()]
        } else {
            paths
        };
        let layout = geometry::measure_source(self, view, &pressed_path, &paths, origin);
        let path_set = paths.iter().cloned().collect();
        let now = Instant::now();
        self.file_drag.internal = Some(InternalDrag {
            source_view: view,
            pressed_path,
            release,
            paths,
            path_set,
            layout,
            origin,
            pointer: origin,
            active: false,
            activated_at: None,
            now,
            target: None,
        });
    }

    pub(crate) fn move_file_drag(&mut self, point: Point) -> bool {
        if !self.file_drag.external_paths.is_empty() {
            self.file_drag.external_pointer = Some(point);
            let count = self.file_drag.external_paths.len();
            self.file_drag.external_target = self.drop_target_at(point, None, count);
        }
        let now = Instant::now();
        let Some((source_view, count, active)) = self.file_drag.internal.as_mut().map(|drag| {
            drag.pointer = point;
            drag.now = now;
            if !drag.active && point.distance(drag.origin) >= DRAG_THRESHOLD {
                drag.active = true;
                drag.activated_at = Some(now);
            }
            (drag.source_view, drag.paths.len(), drag.active)
        }) else {
            return false;
        };
        if !active {
            return false;
        }
        if self.near_window_edge(point) {
            self.hand_off_outbound_drag();
            return true;
        }
        let target = self.drop_target_at(point, Some(source_view), count);
        if let Some(drag) = &mut self.file_drag.internal {
            drag.target = target;
        }
        true
    }

    pub(crate) fn hand_off_outbound_drag(&mut self) {
        let Some(drag) = self.file_drag.internal.take() else {
            return;
        };
        #[cfg(target_os = "macos")]
        {
            let allow_move = !self.modifiers.alt()
                && drag
                    .paths
                    .iter()
                    .all(|path| self.application.move_guidance(path).is_ok());
            match crate::platform::drag::start_outbound_drag(&drag.paths, allow_move) {
                Ok(session_id) => {
                    self.file_drag.outbound_session = Some((session_id, drag.source_view));
                }
                Err(error) => {
                    crate::platform::drag::reset_stale_session();
                    self.views.entry(drag.source_view).or_default().hint = Some(error);
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = drag;
    }

    pub(crate) fn reconcile_outbound_drag(&mut self) -> Task<Message> {
        #[cfg(target_os = "macos")]
        if let Some(completion) = crate::platform::drag::take_completed_drag()
            && let Some((session_id, source_view)) = self.file_drag.outbound_session.take()
            && session_id == completion.session_id
            && completion.moved
            && self.application.workspace().can_browse(source_view)
        {
            return self.dispatch(Action::Navigate(
                source_view,
                crate::application::Navigate::Refresh,
            ));
        }
        Task::none()
    }

    pub(crate) fn window_file_drag_event(&mut self, event: &iced::Event) -> Option<Task<Message>> {
        match event {
            iced::Event::Window(iced::window::Event::FileHovered(path)) => {
                self.hover_external_file(path.clone());
                Some(Task::none())
            }
            iced::Event::Window(iced::window::Event::FilesHoveredLeft) => {
                self.clear_external_hover();
                Some(Task::none())
            }
            iced::Event::Window(iced::window::Event::FileDropped(path)) => {
                Some(self.drop_external_file(path.clone()))
            }
            iced::Event::Mouse(iced::mouse::Event::CursorLeft) if self.file_drag.is_active() => {
                self.hand_off_outbound_drag();
                Some(Task::none())
            }
            _ => None,
        }
    }

    fn near_window_edge(&self, point: Point) -> bool {
        point.x <= OUTBOUND_EDGE_MARGIN
            || point.y <= OUTBOUND_EDGE_MARGIN
            || point.x >= (self.size.width - OUTBOUND_EDGE_MARGIN).max(OUTBOUND_EDGE_MARGIN)
            || point.y >= (self.size.height - OUTBOUND_EDGE_MARGIN).max(OUTBOUND_EDGE_MARGIN)
    }

    pub(crate) fn finish_file_drag(&mut self) -> Task<Message> {
        let Some(drag) = self.file_drag.internal.take() else {
            return Task::none();
        };
        if !drag.active {
            self.apply_click_release(drag.source_view, drag.pressed_path, drag.release);
            return Task::none();
        }
        let moving = !self.modifiers.alt();
        self.apply_drop(drag.target, drag.pointer, drag.paths, moving)
    }

    fn apply_click_release(&mut self, view: ViewId, pressed_path: PathBuf, release: ClickRelease) {
        match release {
            ClickRelease::Keep => {}
            ClickRelease::NarrowMultiSelection => {
                if let Some(presentation) = self.views.get_mut(&view) {
                    presentation.selection.clear();
                    presentation.selection.insert(pressed_path.clone());
                    presentation.selection_anchor = Some(pressed_path.clone());
                    presentation.selected = Some(pressed_path);
                }
            }
            ClickRelease::DeselectPrimary => {
                if let Some(presentation) = self.views.get_mut(&view) {
                    presentation.selection.clear();
                    presentation.selection_anchor = None;
                    presentation.selected = None;
                }
                self.column_previews.clear_selection(view);
            }
            ClickRelease::DeselectColumnPreview => {
                self.column_previews.clear_selection(view);
            }
        }
    }

    pub(crate) fn hover_external_file(&mut self, path: PathBuf) {
        let point = self.live_cursor_position();
        self.file_drag.external_pointer = point;
        if !self.file_drag.external_paths.contains(&path) {
            self.file_drag.external_paths.push(path);
        }
        let count = self.file_drag.external_paths.len();
        self.file_drag.external_target = point.and_then(|pt| self.drop_target_at(pt, None, count));
    }

    pub(crate) fn clear_external_hover(&mut self) {
        self.file_drag.clear_external();
    }

    pub(crate) fn drop_external_file(&mut self, path: PathBuf) -> Task<Message> {
        if let Some(point) = self.live_cursor_position() {
            self.file_drag.external_pointer = Some(point);
        }
        self.file_drag.clear_external();
        if !self.file_drag.dropped_paths.contains(&path) {
            self.file_drag.dropped_paths.push(path);
        }
        if self.file_drag.dropped_paths.len() == 1 {
            Task::perform(std::future::ready(()), |()| Message::FlushExternalDrop)
        } else {
            Task::none()
        }
    }

    pub(crate) fn flush_external_drop(&mut self) -> Task<Message> {
        let paths = std::mem::take(&mut self.file_drag.dropped_paths);
        let point = self
            .file_drag
            .external_pointer
            .take()
            .or_else(|| self.live_cursor_position());
        if paths.is_empty() {
            return Task::none();
        }
        let Some(point) = point else {
            return Task::none();
        };
        let target = self.drop_target_at(point, None, paths.len());
        let moving = self.modifiers.command();
        self.apply_drop(target, point, paths, moving)
    }

    fn apply_drop(
        &mut self,
        target: Option<DropTarget>,
        point: Point,
        paths: Vec<PathBuf>,
        moving: bool,
    ) -> Task<Message> {
        match target {
            Some(DropTarget::FolderBox { view, .. }) => {
                self.active_view = Some(view);
                self.anchor_file_prompt(view, point);
                self.transfer_paths_into(view, paths, moving)
            }
            Some(DropTarget::Launcher { id, .. }) => {
                paths.into_iter().next().map_or_else(Task::none, |path| {
                    self.configure_launcher_from_drop(id, path)
                })
            }
            None => Task::none(),
        }
    }

    fn configure_launcher_from_drop(&mut self, id: BoxId, path: PathBuf) -> Task<Message> {
        let Some(tile) = self
            .application
            .workspace()
            .launcher(id)
            .filter(|tile| tile.config.target.is_none())
        else {
            return Task::none();
        };
        let name = if tile.config.name == "Shortcut" {
            shortcut_name_for(&path).unwrap_or_else(|| tile.config.name.clone())
        } else {
            tile.config.name.clone()
        };
        let config = LauncherConfig {
            name,
            target: Some(LauncherTarget::Path(path)),
            icon: LauncherIcon::Automatic,
        };
        if config.validate().is_err() {
            return Task::none();
        }
        self.dispatch(Action::ConfigureLauncher(id, config))
    }

    fn live_cursor_position(&self) -> Option<Point> {
        #[cfg(target_os = "macos")]
        if let Some((x, y)) = crate::platform::window_cursor_position() {
            return Some(Point::new(x, y));
        }
        self.pointer
    }

    fn drop_target_at(
        &self,
        point: Point,
        source_view: Option<ViewId>,
        item_count: usize,
    ) -> Option<DropTarget> {
        if !self.workspace_input_enabled()
            || self.application.workspace().mode() != Mode::Browse
            || self.application.files_busy()
            || item_count == 0
        {
            return None;
        }
        let geometry = self.geometry();
        if !geometry.viewport.contains(point) {
            return None;
        }
        for item in self.application.workspace().boxes() {
            let rect = geometry.rect(item.rect);
            let bounds = Rectangle {
                x: rect.x + geometry.viewport.x - geometry.offset.x,
                y: rect.y + geometry.viewport.y - geometry.offset.y,
                ..rect
            };
            let view = item.active_view().view;
            if Some(view) != source_view
                && bounds.contains(point)
                && self.application.workspace().can_browse(view)
            {
                return Some(DropTarget::FolderBox {
                    view,
                    rect: bounds.intersection(&geometry.viewport).unwrap_or(bounds),
                });
            }
        }
        if item_count == 1 {
            for item in self.application.workspace().launchers() {
                let rect = geometry.rect(item.rect);
                let bounds = Rectangle {
                    x: rect.x + geometry.viewport.x - geometry.offset.x,
                    y: rect.y + geometry.viewport.y - geometry.offset.y,
                    ..rect
                };
                if item.config.target.is_none() && bounds.contains(point) {
                    return Some(DropTarget::Launcher {
                        id: item.id,
                        rect: bounds.intersection(&geometry.viewport).unwrap_or(bounds),
                    });
                }
            }
        }
        None
    }
}

fn shortcut_name_for(path: &std::path::Path) -> Option<String> {
    let raw = path.file_name()?.to_str()?;
    let trimmed = raw
        .strip_suffix(".app")
        .filter(|stem| !stem.is_empty())
        .unwrap_or(raw);
    let cleaned: String = trimmed
        .chars()
        .filter(|ch| !ch.is_control())
        .take(128)
        .collect();
    LauncherConfig::validate_name(&cleaned)
        .ok()
        .map(|()| cleaned)
}
