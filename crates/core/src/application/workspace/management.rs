//! Workspace transitions retain the source until an ordered storage receipt.

use crate::application::contracts::configuration::{WorkspaceId, WorkspaceIndex};
use crate::application::contracts::storage::{
    ManagementRequest, ManagementResult, ManagementTag, PersistenceState, SessionId,
    StorageFailure, WorkspaceCommand,
};
use crate::application::preferences::settings::ConfigurationEditor;
use crate::application::{Action, Application, Effect};
use crate::diagnostics::{Change, configuration_changed};

#[derive(Debug)]
pub(in crate::application) struct PendingManagement {
    request: ManagementRequest,
    error: Option<StorageFailure>,
    running: bool,
    pub close_requested: bool,
}

impl Application {
    pub fn management_request(&self) -> Option<ManagementRequest> {
        self.management
            .as_ref()
            .map(|pending| pending.request.clone())
    }
    pub fn workspace_epoch(&self) -> Option<SessionId> {
        match self.persistence {
            PersistenceState::Ready { session, .. } | PersistenceState::Loading(session) => {
                Some(session)
            }
            _ => None,
        }
    }
    pub fn workspace_management_open(&self) -> bool {
        self.configuration_editor == Some(ConfigurationEditor::Workspaces)
    }
    pub fn can_manage_workspaces(&self) -> bool {
        self.can_configure_application()
            && !matches!(
                &self.persistence,
                PersistenceState::Ready {
                    error: Some(
                        StorageFailure::ReadOnly
                            | StorageFailure::ChangedExternally
                            | StorageFailure::InvalidConfiguration(_)
                            | StorageFailure::BackupAvailable { .. }
                    ),
                    ..
                }
            )
    }
    pub fn workspace_management_pending(&self) -> bool {
        self.management.is_some()
    }
    pub fn workspace_management_running(&self) -> bool {
        self.management
            .as_ref()
            .is_some_and(|pending| pending.running)
    }
    pub fn workspace_management_error(&self) -> Option<&StorageFailure> {
        self.management
            .as_ref()
            .and_then(|pending| pending.error.as_ref())
    }
    pub fn workspace_management_close_waiting(&self) -> bool {
        self.management
            .as_ref()
            .is_some_and(|pending| pending.close_requested)
    }
    pub(in crate::application) fn management_close_failed(&self) -> bool {
        self.management.as_ref().is_some_and(|pending| {
            pending.close_requested && !pending.running && pending.error.is_some()
        })
    }
    pub(in crate::application) fn cancel_management_close(&mut self) -> bool {
        if self.management_close_failed()
            && let Some(pending) = &mut self.management
        {
            pending.close_requested = false;
            return true;
        }
        false
    }

    pub(in crate::application) fn management_action(&mut self, action: Action) -> Vec<Effect> {
        if matches!(action, Action::OpenWorkspaceManagement) {
            if self.can_manage_workspaces()
                && self.configuration_editor.is_none()
                && self.workspace.mode() != crate::domain::Mode::Browse
            {
                let effects = self.suspend_box_configuration();
                self.workspace.cancel_pointer();
                self.configuration_editor = Some(ConfigurationEditor::Workspaces);
                return effects;
            }
            return vec![];
        }
        if !self.workspace_management_open() {
            return vec![];
        }
        if matches!(
            action,
            Action::RetryWorkspaceManagement | Action::CancelWorkspaceManagement
        ) {
            if let Some(pending) = &mut self.management {
                if pending.running {
                    return vec![];
                }
                let Some(attempt) = pending.request.attempt.checked_add(1) else {
                    pending.error = Some(StorageFailure::Unavailable);
                    return vec![];
                };
                pending.request.attempt = attempt;
                if matches!(action, Action::CancelWorkspaceManagement) {
                    pending.request.cancel = true;
                }
                pending.running = true;
                pending.error = None;
                return vec![Effect::ManageWorkspace(pending.request.clone())];
            }
            if matches!(action, Action::CancelWorkspaceManagement) {
                self.configuration_editor = None;
            }
            return vec![];
        }
        if !self.can_manage_workspaces() {
            return vec![];
        }
        let Some((preferences, index)) = &self.configuration else {
            return vec![];
        };
        if let Action::RenameWorkspace { id, name } = &action
            && preferences.last_workspace_id == Some(*id)
        {
            return self.rename_active_workspace(*id, name.clone());
        }
        let command = match action {
            Action::SelectWorkspace(id)
                if preferences.last_workspace_id != Some(id)
                    && index.workspaces.iter().any(|entry| entry.id == id) =>
            {
                WorkspaceCommand::Select(id)
            }
            Action::CreateWorkspace(name)
                if WorkspaceIndex::valid_name(&name)
                    && index.workspaces.len() < WorkspaceIndex::MAX_WORKSPACES =>
            {
                WorkspaceCommand::Create {
                    id: WorkspaceId::random(),
                    name,
                }
            }
            Action::RenameWorkspace { id, name }
                if preferences.last_workspace_id != Some(id)
                    && WorkspaceIndex::valid_name(&name)
                    && index
                        .workspaces
                        .iter()
                        .any(|entry| entry.id == id && entry.name != name) =>
            {
                WorkspaceCommand::Rename { id, name }
            }
            Action::RemoveWorkspace {
                id,
                confirmed: true,
            } if preferences.last_workspace_id != Some(id)
                && index.workspaces.len() > 1
                && index.workspaces.iter().any(|entry| entry.id == id) =>
            {
                WorkspaceCommand::Remove(id)
            }
            _ => return vec![],
        };
        self.begin_workspace_management(command)
    }

    fn rename_active_workspace(&mut self, active_id: WorkspaceId, value: String) -> Vec<Effect> {
        if !WorkspaceIndex::valid_name(&value) {
            return Vec::new();
        }
        let PersistenceState::Ready { id, .. } = self.persistence else {
            return Vec::new();
        };
        if id != active_id {
            return Vec::new();
        }
        let Some((_, index)) = &mut self.configuration else {
            return Vec::new();
        };
        let Some(summary) = index.workspaces.iter_mut().find(|item| item.id == id) else {
            return Vec::new();
        };
        if summary.name == value {
            return Vec::new();
        }
        summary.name = value;
        configuration_changed(Change::WorkspaceRenamed);
        self.save_effect(true)
    }

    fn begin_workspace_management(&mut self, command: WorkspaceCommand) -> Vec<Effect> {
        let mut effects = self.suspend_box_configuration();
        self.workspace.cancel_pointer();
        let Some(source) = self
            .save_effect(false)
            .into_iter()
            .find_map(|effect| match effect {
                Effect::SaveWorkspace(request) => Some(request),
                _ => None,
            })
        else {
            return effects;
        };
        let request = ManagementRequest {
            operation: WorkspaceId::random(),
            attempt: 1,
            next_session: SessionId(WorkspaceId::random()),
            source,
            command,
            cancel: false,
        };
        self.management = Some(PendingManagement {
            request: request.clone(),
            error: None,
            running: true,
            close_requested: false,
        });
        effects.push(Effect::ManageWorkspace(request));
        effects
    }

    pub(in crate::application) fn management_completed(
        &mut self,
        operation: ManagementTag,
        result: Result<ManagementResult, StorageFailure>,
    ) -> Vec<Effect> {
        if !self
            .management
            .as_ref()
            .is_some_and(|pending| pending.running && pending.request.receipt_tag() == operation)
        {
            return vec![];
        }
        let Some(mut pending) = self.management.take() else {
            return vec![];
        };
        let result = result.and_then(|result| {
            let selection = !pending.request.cancel
                && matches!(
                    pending.request.command,
                    WorkspaceCommand::Select(_) | WorkspaceCommand::Create { .. }
                );
            if result.cancelled != pending.request.cancel || result.selected.is_some() != selection
            {
                return Err(StorageFailure::StaleRequest);
            }
            if let Some(loaded) = &result.selected {
                let expected = match pending.request.command {
                    WorkspaceCommand::Select(id) | WorkspaceCommand::Create { id, .. } => Some(id),
                    WorkspaceCommand::Rename { .. } | WorkspaceCommand::Remove(_) => None,
                };
                if expected != Some(loaded.active.id)
                    || loaded.preferences.last_workspace_id != Some(loaded.active.id)
                    || !loaded
                        .index
                        .workspaces
                        .iter()
                        .any(|entry| entry.id == loaded.active.id)
                    || !loaded.recovery_issues.is_empty()
                {
                    return Err(StorageFailure::StaleRequest);
                }
            }
            Ok(result)
        });
        let mut result = match result {
            Ok(result) => result,
            Err(error) => {
                pending.running = false;
                pending.error = Some(error);
                self.management = Some(pending);
                return vec![];
            }
        };
        if pending.close_requested
            || result.cancelled
            || !matches!(
                pending.request.command,
                WorkspaceCommand::Select(_) | WorkspaceCommand::Rename { .. }
            )
        {
            self.configuration_editor = None;
        }
        if !result.cancelled {
            configuration_changed(match pending.request.command {
                WorkspaceCommand::Select(_) => Change::WorkspaceSelected,
                WorkspaceCommand::Create { .. } => Change::WorkspaceCreated,
                WorkspaceCommand::Rename { .. } => Change::WorkspaceRenamed,
                WorkspaceCommand::Remove(_) => Change::WorkspaceRemoved,
            });
        }
        let mut effects = if let Some(loaded) = result.selected.take() {
            self.adopt_managed_workspace(*loaded, pending.request.next_session)
        } else {
            self.resume_management_source(result, &pending.request)
        };
        if pending.close_requested {
            effects.retain(|effect| !matches!(effect, Effect::List(_)));
            effects.extend(self.request_close());
        }
        effects
    }

    fn adopt_managed_workspace(
        &mut self,
        loaded: crate::application::contracts::storage::LoadedConfiguration,
        session: SessionId,
    ) -> Vec<Effect> {
        self.pending_configuration = None;
        self.pending_tabs = None;
        self.pending_scope = None;
        self.folder_tree = None;
        self.box_configuration_error = None;
        self.tab_error = None;
        self.scope_error = None;
        self.opening.current = None;
        self.loads.clear();
        self.restore_queue.clear();
        self.restore_in_flight.clear();
        self.activation_reads.clear();
        self.invalidate_workspace_observations();
        self.sync_metadata();
        self.adopt_startup(loaded, session)
    }

    fn resume_management_source(
        &mut self,
        result: ManagementResult,
        request: &ManagementRequest,
    ) -> Vec<Effect> {
        if let Some((_, index)) = &mut self.configuration {
            *index = result.index;
            if let Some(source) = index
                .workspaces
                .iter_mut()
                .find(|entry| entry.id == request.source.tag.workspace)
            {
                source.name.clone_from(&request.source.workspace_name);
            }
        }
        if let PersistenceState::Ready {
            saved_revision,
            revision,
            error,
            ..
        } = &mut self.persistence
        {
            if result.source_saved {
                *saved_revision = *revision;
                *error = None;
            } else if result.source_error.is_some() {
                *error = result.source_error;
            }
        }
        let loading: Vec<_> = self
            .loads
            .iter()
            .filter_map(|(view, load)| {
                matches!(load, crate::application::LoadState::Loading { .. }).then_some(*view)
            })
            .collect();
        for view in loading {
            if let Some(crate::application::LoadState::Loading { previous, .. }) =
                self.loads.remove(&view)
            {
                self.restore_in_flight.remove(&view);
                if let Some(listing) = previous {
                    self.loads
                        .insert(view, crate::application::LoadState::Ready(listing));
                } else if !self.restore_queue.contains(&view) {
                    self.restore_queue.push_back(view);
                }
            }
        }
        self.fill_restore_reads()
    }
}
