use std::sync::Arc;

use crate::application::contracts::configuration::WorkspaceDocument;
use crate::application::contracts::storage::{
    CloseStage, PersistenceState, SaveOutcome, SaveRequest, SaveTag, StorageCompletion,
    StorageFailure,
};
use crate::application::{Application, Effect};
use crate::diagnostics::{Activity, Outcome, Subsystem, session as recorder};
use crate::domain::NavigationIntent;

impl Application {
    pub(in crate::application) fn close_without_saving(&self) -> Vec<Effect> {
        if self.close_failed() && !self.files_busy() {
            vec![Effect::Close]
        } else {
            vec![]
        }
    }
    pub fn persistence(&self) -> &PersistenceState {
        &self.persistence
    }

    pub fn configuration(
        &self,
    ) -> Option<&(
        crate::application::contracts::configuration::AppPreferences,
        crate::application::contracts::configuration::WorkspaceIndex,
    )> {
        self.configuration.as_ref()
    }

    pub fn accepts_edits(&self) -> bool {
        if self.management.is_some() || self.files_closing() {
            return false;
        }
        match self.persistence {
            PersistenceState::Loading(_) => false,
            PersistenceState::Ready { closing, .. } => closing == CloseStage::Open,
            PersistenceState::Recovery { closing, .. } => !closing,
            PersistenceState::Off => true,
        }
    }

    pub fn close_failed(&self) -> bool {
        if self.management_close_failed() {
            return true;
        }
        matches!(
            self.persistence,
            PersistenceState::Ready {
                closing: CloseStage::Failed,
                ..
            } | PersistenceState::Ready {
                closing: CloseStage::Flushing | CloseStage::SamplingWindow,
                error: Some(_),
                ..
            } | PersistenceState::Recovery { closing: true, .. }
        )
    }

    pub fn save_failed(&self) -> bool {
        matches!(
            self.persistence,
            PersistenceState::Ready { error: Some(_), .. }
        )
    }

    pub fn workspace_name(&self) -> Option<&str> {
        let (preferences, index) = self.configuration.as_ref()?;
        let id = preferences.last_workspace_id?;
        index
            .workspaces
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.name.as_str())
    }

    pub fn storage_detail(&self) -> Option<String> {
        if self.management_close_failed() {
            return self.workspace_management_error().map(|error| format!("Cannot finish the workspace transition before close: {error}\nRetry cancels the unfinished transition and saves the current workspace. Cancel close returns to workspace management."));
        }
        match &self.persistence {
            PersistenceState::Recovery { message, .. } => Some(message.clone()),
            PersistenceState::Ready {
                error: Some(error), ..
            } => Some(format!("Cannot save workspace: {error}")),
            PersistenceState::Ready {
                notice: Some(notice),
                ..
            } => Some(notice.clone()),
            _ => None,
        }
    }

    pub(in crate::application) fn storage_completed(
        &mut self,
        completion: StorageCompletion,
    ) -> Vec<Effect> {
        match completion {
            StorageCompletion::Managed { operation, result } => {
                self.management_completed(operation, result)
            }
            StorageCompletion::BudgetChecked { generation, result } => {
                if let Some(effects) = self.scope_budget_checked(generation, result.clone()) {
                    effects
                } else {
                    self.tab_budget_checked(generation, result)
                }
            }
            StorageCompletion::Loaded { session, result } => {
                if !matches!(self.persistence, PersistenceState::Loading(current) if current == session)
                {
                    recorder().record(
                        Subsystem::Storage,
                        Activity::Receipt,
                        Outcome::WorkspaceMismatch,
                        0,
                        0,
                        None,
                    );
                    return Vec::new();
                }
                match result {
                    Ok(loaded) => self.adopt_startup(*loaded, session),
                    Err(error) => {
                        self.startup_failed(error);
                        Vec::new()
                    }
                }
            }
            StorageCompletion::Saved { tag, result } => self.saved(tag, result),
        }
    }

    fn saved(&mut self, tag: SaveTag, result: Result<SaveOutcome, StorageFailure>) -> Vec<Effect> {
        let PersistenceState::Ready {
            session,
            id,
            revision,
            saved_revision,
            attempt,
            error,
            closing,
            ..
        } = &mut self.persistence
        else {
            return Vec::new();
        };
        if tag.session != *session
            || tag.workspace != *id
            || tag.attempt != *attempt
            || tag.revision != *revision
        {
            recorder().record(
                Subsystem::Storage,
                Activity::Receipt,
                Outcome::Stale,
                tag.attempt,
                tag.revision,
                None,
            );
            return Vec::new();
        }
        match result {
            Ok(SaveOutcome::Saved) => {
                *saved_revision = tag.revision;
                *error = None;
                self.user_themes_edited = false;
                if *closing == CloseStage::Flushing {
                    recorder().record(
                        Subsystem::Session,
                        Activity::Close,
                        Outcome::Complete,
                        tag.attempt,
                        tag.revision,
                        None,
                    );
                    return vec![Effect::Close];
                }
            }
            Ok(SaveOutcome::Superseded) => {}
            Err(failure) => {
                *error = Some(failure);
                if *closing == CloseStage::Flushing {
                    *closing = CloseStage::Failed;
                    recorder().record(
                        Subsystem::Session,
                        Activity::Close,
                        Outcome::Failed,
                        tag.attempt,
                        tag.revision,
                        None,
                    );
                }
            }
        }
        Vec::new()
    }

    pub(in crate::application) fn save_effect(&mut self, changed: bool) -> Vec<Effect> {
        if changed && let PersistenceState::Recovery { dirty, .. } = &mut self.persistence {
            *dirty = true;
        }
        let PersistenceState::Ready {
            session,
            id,
            revision,
            attempt,
            error,
            ..
        } = &mut self.persistence
        else {
            return Vec::new();
        };
        let Some((preferences, index)) = &self.configuration else {
            *error = Some(StorageFailure::Unavailable);
            return Vec::new();
        };
        let Some(summary) = index.workspaces.iter().find(|item| item.id == *id) else {
            *error = Some(StorageFailure::Unavailable);
            return Vec::new();
        };
        if changed {
            let Some(next) = revision.checked_add(1) else {
                *error = Some(StorageFailure::Unavailable);
                return Vec::new();
            };
            *revision = next;
        }
        let Some(next) = attempt.checked_add(1) else {
            *error = Some(StorageFailure::Unavailable);
            return Vec::new();
        };
        *attempt = next;
        // Do not clear a previous error until a successful receipt. This keeps
        // recovery guidance visible throughout retry and subsequent model edits.
        vec![Effect::SaveWorkspace(SaveRequest {
            tag: SaveTag {
                session: *session,
                workspace: *id,
                revision: *revision,
                attempt: *attempt,
            },
            document: Arc::new(WorkspaceDocument {
                id: *id,
                workspace: self.workspace.snapshot(),
            }),
            preferences: preferences.clone(),
            workspace_name: summary.name.clone(),
            user_themes: self
                .user_themes_edited
                .then(|| Arc::clone(&self.user_themes)),
        })]
    }

    pub(in crate::application) fn retry_save(&mut self) -> Vec<Effect> {
        if self.management.is_some() {
            return self.management_action(if self.workspace_management_close_waiting() {
                crate::application::Action::CancelWorkspaceManagement
            } else {
                crate::application::Action::RetryWorkspaceManagement
            });
        }
        if let PersistenceState::Ready {
            session,
            id,
            revision,
            attempt,
            closing,
            ..
        } = &mut self.persistence
        {
            match closing {
                CloseStage::SamplingWindow | CloseStage::Flushing => return Vec::new(),
                CloseStage::Failed => {
                    *closing = CloseStage::SamplingWindow;
                    return vec![Effect::CaptureWindowForClose(SaveTag {
                        session: *session,
                        workspace: *id,
                        revision: *revision,
                        attempt: *attempt,
                    })];
                }
                CloseStage::Open => {}
            }
        }
        self.save_effect(false)
    }

    pub(in crate::application) fn request_close(&mut self) -> Vec<Effect> {
        recorder().record(
            Subsystem::Session,
            Activity::Close,
            Outcome::Requested,
            0,
            0,
            None,
        );
        if self.files_busy() {
            if let Some(operation) = self.file_operation() {
                self.stop_files(operation.id);
            }
            if self.files_busy() {
                self.file_operations.close_requested = true;
                recorder().record(
                    Subsystem::Session,
                    Activity::Close,
                    Outcome::WaitingFiles,
                    0,
                    0,
                    None,
                );
                self.configuration_editor = None;
                return vec![];
            }
        }
        if let Some(pending) = &mut self.management {
            recorder().record(
                Subsystem::Session,
                Activity::Close,
                Outcome::WaitingTransition,
                0,
                0,
                None,
            );
            pending.close_requested = true;
            return Vec::new();
        }
        if !self.accepts_edits() {
            if matches!(self.persistence, PersistenceState::Loading(_)) {
                return vec![Effect::Close];
            }
            return Vec::new();
        }
        self.workspace.cancel_pointer();
        self.pending_tabs = None;
        let _ = self.cancel_folder_configuration();
        self.configuration_editor = None;
        if let PersistenceState::Ready {
            session,
            id,
            revision,
            attempt,
            closing,
            ..
        } = &mut self.persistence
        {
            *closing = CloseStage::SamplingWindow;
            recorder().record(
                Subsystem::Session,
                Activity::CaptureWindow,
                Outcome::Requested,
                *attempt,
                *revision,
                None,
            );
            vec![Effect::CaptureWindowForClose(SaveTag {
                session: *session,
                workspace: *id,
                revision: *revision,
                attempt: *attempt,
            })]
        } else if let PersistenceState::Recovery {
            dirty: true,
            closing,
            ..
        } = &mut self.persistence
        {
            *closing = true;
            Vec::new()
        } else {
            vec![Effect::Close]
        }
    }

    pub(in crate::application) fn cancel_close(&mut self) -> Vec<Effect> {
        recorder().record(
            Subsystem::Session,
            Activity::Close,
            Outcome::CancelRequested,
            0,
            0,
            None,
        );
        if self.files_closing() {
            self.file_operations.close_requested = false;
            return vec![];
        }
        if self.cancel_management_close() {
            return Vec::new();
        }
        if !self.close_failed() {
            return Vec::new();
        }
        if let PersistenceState::Ready { closing, .. } = &mut self.persistence {
            *closing = CloseStage::Open;
        }
        if let PersistenceState::Recovery { closing, .. } = &mut self.persistence {
            *closing = false;
        }
        self.restore_in_flight.clear();
        for load in self.loads.values_mut() {
            if let crate::application::LoadState::Loading { previous, .. } = load {
                *load = match previous.take() {
                    Some(previous) => crate::application::LoadState::Ready(previous),
                    None => crate::application::LoadState::Failed {
                        message: "Folder loading was cancelled during close. Reload folder listings to retry."
                            .into(),
                        previous: None,
                    },
                };
            }
        }
        self.fill_restore_reads()
    }

    pub(in crate::application) fn schedule_restore_reads(&mut self) -> Vec<Effect> {
        self.loads.clear();
        self.activation_reads.clear();
        self.restore_in_flight.clear();
        self.restore_queue = self
            .workspace
            .active_views()
            .map(|item| item.view)
            .collect();
        self.fill_restore_reads()
    }

    pub(in crate::application) fn fill_restore_reads(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        while self.restore_in_flight.len() < 2 {
            let Some(view) = self.restore_queue.pop_front() else {
                break;
            };
            if self.loads.contains_key(&view) {
                continue;
            }
            let Some(item) = self
                .workspace
                .view(view)
                .filter(|_| self.workspace.is_active(view))
            else {
                continue;
            };
            let path = item.navigation.current().to_path_buf();
            // Restoration never resets the configured starting folder/history.
            let intent = if self.activation_reads.remove(&view) {
                NavigationIntent::Activate
            } else {
                NavigationIntent::Restore
            };
            if let Some(effect) = self.request(view, path, intent) {
                self.restore_in_flight.insert(view);
                effects.push(effect);
            }
        }
        effects
    }
}
