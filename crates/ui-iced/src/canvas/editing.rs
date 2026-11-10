//! Pointer anchors and presentation reconciliation, not another workspace model.

use crate::application::Action;
use crate::domain::{BoxId, GridDelta, LayoutKind, ViewId};
use crate::{Desktop, Message, boxes::file_list, canvas::geometry::PITCH, canvas::layout_controls};
use iced::{Point, Task, widget::operation};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Copy)]
pub(crate) struct Anchor {
    id: BoxId,
    kind: LayoutKind,
    position: Point,
}

impl Desktop {
    pub(crate) fn prepare_action_focus(&mut self, action: &Action) -> bool {
        self.observe_file_action_presentation(action);
        if matches!(action, Action::ActivateLauncher(_)) {
            self.deactivate_all_views();
        } else if let Action::Navigate(view, nav) = action
            && !matches!(nav, crate::application::Navigate::Refresh)
            && self.application.workspace().mode() == crate::domain::Mode::Browse
            && self.application.workspace().can_browse(*view)
        {
            self.type_jump.dismiss_session();
            self.activate_view(*view);
        }
        matches!(
            action,
            Action::Undo
                | Action::RemoveBox(_)
                | Action::RemoveSelected
                | Action::ActivateLauncher(_)
        )
    }
    pub(crate) fn focus_canvas(&mut self) -> Task<Message> {
        self.focused_path = None;
        self.focus_version = self.focus_version.saturating_add(1);
        iced::widget::operation::focus("workspace-canvas")
    }
    pub(crate) fn view_folders(&self) -> HashMap<ViewId, PathBuf> {
        self.application
            .workspace()
            .all_views()
            .map(|item| (item.view, item.navigation.current().to_path_buf()))
            .collect()
    }

    pub(crate) fn reset_restored_views(&mut self) {
        self.pending_reads.clear();
        self.views.clear();
        self.tab_offsets.clear();
        self.active_view = None;
        self.type_jump = crate::runtime::type_jump::State::default();
        self.focused_path = None;
        self.focus_version = self.focus_version.saturating_add(1);
        self.canvas_offset = iced::Vector::ZERO;
    }
    pub(crate) fn editing_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::LayoutPressed { id, kind, position } => {
                if !self.workspace_input_enabled() {
                    return Task::none();
                }
                let task = self.dispatch(Action::BeginLayout {
                    id,
                    kind,
                    extent: self.geometry().extent,
                });
                if self
                    .application
                    .workspace()
                    .layout_preview()
                    .is_some_and(|preview| preview.id == id && preview.kind == kind)
                {
                    self.edit_anchor = Some(Anchor { id, kind, position });
                    self.focused_path = None;
                    self.focus_version = self.focus_version.saturating_add(1);
                }
                task
            }
            Message::LayoutPointerMoved(position) => {
                let Some(anchor) = self.edit_anchor else {
                    return Task::none();
                };
                if !self
                    .application
                    .workspace()
                    .layout_preview()
                    .is_some_and(|preview| preview.id == anchor.id && preview.kind == anchor.kind)
                {
                    return Task::none();
                }
                let delta = match anchor.kind {
                    LayoutKind::Move => {
                        let first = self.geometry().cell(anchor.position);
                        let last = self.geometry().cell(position);
                        GridDelta {
                            columns: i32::from(last.column) - i32::from(first.column),
                            rows: i32::from(last.row) - i32::from(first.row),
                        }
                    }
                    LayoutKind::Resize(_) => GridDelta {
                        columns: ((position.x - anchor.position.x) / PITCH).round() as i32,
                        rows: ((position.y - anchor.position.y) / PITCH).round() as i32,
                    },
                };
                self.dispatch(Action::LayoutTo(delta))
            }
            Message::FinishLayout => {
                self.edit_anchor = None;
                self.dispatch(Action::FinishLayout)
            }
            _ => Task::none(),
        }
    }

    pub(crate) fn editing_press(&mut self, point: Point) -> Task<Message> {
        let geometry = self.geometry();
        let frame = iced::Rectangle {
            x: geometry.viewport.x - crate::canvas::geometry::GUTTER,
            y: geometry.viewport.y - crate::canvas::geometry::GUTTER,
            width: geometry.viewport.width + 2.0 * crate::canvas::geometry::GUTTER,
            height: geometry.viewport.height + 2.0 * crate::canvas::geometry::GUTTER,
        };
        if !frame.contains(point) {
            return Task::none();
        }
        match layout_controls::hit_with_offsets(
            self.application.workspace(),
            self.geometry(),
            point,
            &self.tab_offsets,
        ) {
            Some(layout_controls::Target::Tab(id, view)) => self.begin_tab_drag(id, view, point),
            Some(layout_controls::Target::Resize(id, handle)) => {
                self.editing_message(Message::LayoutPressed {
                    id,
                    kind: LayoutKind::Resize(handle),
                    position: point,
                })
            }
            Some(layout_controls::Target::Body(id)) => {
                self.editing_message(Message::LayoutPressed {
                    id,
                    kind: LayoutKind::Move,
                    position: point,
                })
            }
            Some(layout_controls::Target::Remove(id)) => self.dispatch(Action::RemoveBox(id)),
            None if geometry.can_start(point) => {
                self.dispatch(Action::BeginItemDraw(geometry.cell(point), geometry.extent))
            }
            _ => self.dispatch(Action::SelectBox(None)),
        }
    }

    pub(crate) fn sync_editing_views(
        &mut self,
        before: &HashMap<ViewId, PathBuf>,
        skip_reset: Option<ViewId>,
        full_reset: bool,
    ) -> Task<Message> {
        self.views
            .retain(|view, _| self.application.workspace().view(*view).is_some());
        self.active_view = self
            .active_view
            .filter(|view| self.application.workspace().is_active(*view));
        self.focused_path = self
            .focused_path
            .filter(|view| self.application.workspace().view(*view).is_some());
        for (view, presentation) in &mut self.views {
            if !self.application.workspace().is_active(*view) {
                presentation.hovered = None;
            }
        }
        if self.application.workspace().layout_preview().is_none() {
            self.edit_anchor = None;
        }
        if full_reset {
            return Task::batch(self.application.workspace().active_views().map(|item| {
                operation::scroll_to(
                    file_list::scroll_id(item.view),
                    operation::AbsoluteOffset { x: 0.0, y: 0.0 },
                )
            }));
        }
        let mut resets = Vec::new();
        for item in self.application.workspace().all_views() {
            if Some(item.view) != skip_reset
                && before
                    .get(&item.view)
                    .is_some_and(|folder| folder != item.navigation.current())
            {
                self.views.remove(&item.view);
                if self.application.workspace().is_active(item.view) {
                    resets.push(operation::scroll_to(
                        file_list::scroll_id(item.view),
                        operation::AbsoluteOffset { x: 0.0, y: 0.0 },
                    ));
                }
            }
        }
        Task::batch(resets)
    }
}
