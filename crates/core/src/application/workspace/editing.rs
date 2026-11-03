//! Layout commits, Undo, and affected directory lifetimes, without platform calls.

use crate::application::{Application, Effect};
use crate::diagnostics::{Change, configuration_changed};
use crate::domain::{BoxId, LayoutOutcome, UndoOutcome, ViewId};

impl Application {
    pub(in crate::application) fn editing_action(
        &mut self,
        action: crate::application::Action,
    ) -> Vec<Effect> {
        match action {
            crate::application::Action::BeginLayout { id, kind, extent } => {
                self.workspace.begin_layout(id, kind, extent);
                self.status = None;
            }
            crate::application::Action::LayoutTo(delta) => self.workspace.layout_to(delta),
            crate::application::Action::FinishLayout => return self.finish_layout(),
            crate::application::Action::RemoveBox(id) => return self.remove_box(id),
            crate::application::Action::RemoveSelected => {
                return self
                    .workspace
                    .selected()
                    .map_or_else(Vec::new, |id| self.remove_box(id));
            }
            crate::application::Action::Undo => return self.undo_workspace(),
            _ => {}
        }
        vec![]
    }
    fn finish_layout(&mut self) -> Vec<Effect> {
        let launcher = self
            .workspace
            .layout_preview()
            .is_some_and(|preview| self.workspace.launcher(preview.id).is_some());
        match self.workspace.finish_layout() {
            LayoutOutcome::Changed => {
                configuration_changed(if launcher {
                    Change::LauncherLayout
                } else {
                    Change::BoxLayout
                });
                self.save_effect(true)
            }
            LayoutOutcome::Invalid => {
                self.status = Some("Cannot place this item here. Keep items within the canvas and clear of each other.".into());
                Vec::new()
            }
            LayoutOutcome::None | LayoutOutcome::Unchanged => Vec::new(),
        }
    }

    fn remove_box(&mut self, id: BoxId) -> Vec<Effect> {
        let launcher = self.workspace.launcher(id).is_some();
        if self.pending_configuration.as_ref().is_some_and(|pending| {
            self.workspace
                .view(pending.view)
                .is_some_and(|item| item.id == id)
        }) && self.workspace.configuring(id)
        {
            self.pending_configuration = None;
        }
        if !self.workspace.remove_box(id) {
            return Vec::new();
        }
        configuration_changed(if launcher {
            Change::LauncherRemoved
        } else {
            Change::BoxRemoved
        });
        if self
            .pending_tabs
            .as_ref()
            .is_some_and(|pending| pending.id == id)
        {
            self.pending_tabs = None;
        }
        self.retain_current_views();
        self.status = None;
        let mut effects = self.fill_restore_reads();
        effects.extend(self.save_effect(true));
        effects
    }

    fn undo_workspace(&mut self) -> Vec<Effect> {
        self.pending_tabs = None;
        let mut effects = self.cancel_folder_configuration();
        self.box_configuration_error = None;
        let before: std::collections::HashMap<_, _> = self
            .workspace
            .all_views()
            .map(|item| (item.view, item.navigation.clone()))
            .collect();
        self.status = None;
        if self.workspace.undo() != UndoOutcome::Restored {
            return effects;
        }
        configuration_changed(Change::WorkspaceUndo);
        self.retain_current_views();
        // Changed and restored navigation gets a fresh generation. Unaffected
        // listings and reads survive a geometry-only Undo.
        let reloads: Vec<_> = self
            .workspace
            .active_views()
            .filter_map(|item| {
                let changed = before.get(&item.view) != Some(&item.navigation);
                (changed || !self.loads.contains_key(&item.view)).then_some((item.view, !changed))
            })
            .collect();
        for (view, activation) in reloads {
            self.loads.remove(&view);
            self.restore_in_flight.remove(&view);
            self.restore_queue.retain(|queued| *queued != view);
            self.restore_queue.push_back(view);
            if activation {
                self.activation_reads.insert(view);
            } else {
                self.activation_reads.remove(&view);
            }
        }
        effects.extend(self.fill_restore_reads());
        effects.extend(self.save_effect(true));
        effects
    }

    pub(in crate::application) fn retain_current_views(&mut self) {
        let current: std::collections::HashSet<ViewId> = self
            .workspace
            .active_views()
            .map(|item| item.view)
            .collect();
        self.loads.retain(|view, _| current.contains(view));
        self.restore_in_flight.retain(|view| current.contains(view));
        self.restore_queue.retain(|view| current.contains(view));
        self.activation_reads.retain(|view| current.contains(view));
    }
}
