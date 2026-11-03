//! Tab admission and active-only directory lifetimes, without JSON or filesystem work.

use std::sync::Arc;

use crate::application::contracts::configuration::{WorkspaceDocument, WorkspaceId};
use crate::application::contracts::storage::{BudgetRequest, StorageFailure};
use crate::application::{Application, Effect};
use crate::diagnostics::{Change, configuration_changed};
use crate::domain::{BoxId, TabCommand, Workspace};

#[derive(Debug)]
pub(in crate::application) struct PendingTabs {
    request: BudgetRequest,
    baseline: Workspace,
    pub(in crate::application) id: BoxId,
    command: TabCommand,
}

impl Application {
    pub(in crate::application) fn supersede_tab_gesture(
        &mut self,
        action: &crate::application::Action,
    ) {
        if self.workspace.mode() == crate::domain::Mode::Layout
            && matches!(
                action,
                crate::application::Action::BeginDraw(..)
                    | crate::application::Action::BeginLayout { .. }
                    | crate::application::Action::SelectBox(_)
            )
        {
            self.pending_tabs = None;
        }
    }
    pub fn tab_error(&self, id: BoxId) -> Option<&str> {
        self.tab_error
            .as_ref()
            .filter(|(box_id, _)| *box_id == id)
            .map(|(_, message)| message.as_str())
    }
    pub fn tab_pending(&self, id: BoxId) -> bool {
        self.pending_tabs
            .as_ref()
            .is_some_and(|pending| pending.id == id)
    }
    pub fn pending_tab_budget(&self) -> Option<BudgetRequest> {
        self.pending_tabs
            .as_ref()
            .map(|pending| pending.request.clone())
    }

    pub(in crate::application) fn change_tabs(
        &mut self,
        id: BoxId,
        command: TabCommand,
    ) -> Vec<Effect> {
        if !self.workspace.can_change_tabs(id, command) {
            return Vec::new();
        }
        let mut effects = self.cancel_folder_configuration();
        self.box_configuration_error = None;
        self.pending_tabs = None;
        self.tab_error = None;
        match self.workspace.propose_tabs(id, command) {
            Ok(Some(workspace)) => {
                let Some(generation) = self.next_tab_generation.checked_add(1) else {
                    self.tab_error = Some((id, "Cannot change tabs: request identities are exhausted. Keep browsing existing views and collect application output.".into()));
                    return effects;
                };
                self.next_tab_generation = generation;
                let workspace_id = self
                    .configuration
                    .as_ref()
                    .and_then(|(preferences, _)| preferences.last_workspace_id)
                    .unwrap_or_else(WorkspaceId::random);
                let request = BudgetRequest {
                    generation,
                    document: Arc::new(WorkspaceDocument {
                        id: workspace_id,
                        workspace,
                    }),
                };
                self.pending_tabs = Some(PendingTabs {
                    request: request.clone(),
                    baseline: self.workspace.snapshot(),
                    id,
                    command,
                });
                effects.push(Effect::CheckWorkspaceBudget(request));
            }
            Ok(None) => {}
            Err(reason) => {
                self.tab_error = Some((
                    id,
                    format!(
                        "Cannot change tabs: {reason}. Keep existing tabs, or close a tab or box and retry."
                    ),
                ))
            }
        }
        effects
    }

    pub(in crate::application) fn tab_budget_checked(
        &mut self,
        generation: u64,
        result: Result<(), StorageFailure>,
    ) -> Vec<Effect> {
        if !self
            .pending_tabs
            .as_ref()
            .is_some_and(|pending| pending.request.generation == generation)
        {
            return Vec::new();
        }
        let Some(pending) = self.pending_tabs.take() else {
            return Vec::new();
        };
        if !self.accepts_workspace_input()
            || !self.workspace.can_change_tabs(pending.id, pending.command)
        {
            return Vec::new();
        }
        if !self.workspace.same_durable_state(&pending.baseline) {
            self.tab_error = Some((
                pending.id,
                "Tab change cancelled because the workspace changed. Try again.".into(),
            ));
            return Vec::new();
        }
        if let Err(error) = result {
            let message = if matches!(error, StorageFailure::InvalidConfiguration(_)) {
                "Cannot change tabs: the proposed workspace is invalid or exceeds its saved-size limit. Keep existing tabs, or close a tab or box and retry."
            } else {
                "Cannot check tab changes: the storage worker is unavailable. Keep existing tabs and collect application output."
            };
            self.tab_error = Some((pending.id, message.into()));
            return Vec::new();
        }
        self.workspace
            .accept_tab_proposal(pending.request.document.workspace.snapshot());
        if !matches!(pending.command, TabCommand::Activate(_)) {
            configuration_changed(Change::BoxTabs);
        }
        self.retain_current_views();
        for view in self.workspace.active_views() {
            if !self.loads.contains_key(&view.view) && !self.restore_queue.contains(&view.view) {
                self.restore_queue.push_back(view.view);
                self.activation_reads.insert(view.view);
            }
        }
        self.tab_error = None;
        let mut effects = self.fill_restore_reads();
        effects.extend(self.save_effect(true));
        effects
    }
}
