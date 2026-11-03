//! Startup adoption and recovery decisions, without Iced or filesystem effects.

use crate::application::contracts::configuration::WorkspaceId;
use crate::application::contracts::storage::{
    BackupOffer, CloseStage, LoadedConfiguration, PersistenceState, SessionId, StartupOperation,
    StorageFailure,
};
use crate::application::{Application, Effect};

impl Application {
    pub fn with_storage(
        directory_override: Option<std::path::PathBuf>,
        home: std::path::PathBuf,
    ) -> (Self, Vec<Effect>) {
        let mut application = Self::new(home.clone(), home);
        application.directory_override = directory_override;
        let session = SessionId(WorkspaceId::random());
        application.persistence = PersistenceState::Loading(session);
        (application, vec![Effect::LoadConfiguration(session)])
    }
    pub fn backup_offer(&self) -> Option<&BackupOffer> {
        match &self.persistence {
            PersistenceState::Recovery {
                backup,
                closing: false,
                ..
            } => backup.as_ref(),
            _ => None,
        }
    }

    pub fn can_retry_startup(&self) -> bool {
        matches!(
            self.persistence,
            PersistenceState::Recovery { closing: false, .. }
        )
    }

    pub fn startup_has_unsaved_edits(&self) -> bool {
        matches!(
            self.persistence,
            PersistenceState::Recovery { dirty: true, .. }
        )
    }

    pub(in crate::application) fn restart_startup(
        &mut self,
        operation: StartupOperation,
        confirmed: bool,
    ) -> Vec<Effect> {
        if !self.can_retry_startup() || self.startup_has_unsaved_edits() && !confirmed {
            return Vec::new();
        }
        let session = SessionId(WorkspaceId::random());
        let effect = match operation {
            StartupOperation::Retry => Effect::LoadConfiguration(session),
            StartupOperation::RestoreBackup(token) => {
                if !confirmed
                    || !self
                        .backup_offer()
                        .is_some_and(|offer| offer.token == token)
                {
                    return Vec::new();
                }
                Effect::RestoreBackup { session, token }
            }
        };
        self.workspace.cancel_pointer();
        self.configuration_editor = None;
        self.persistence = PersistenceState::Loading(session);
        self.loads.clear();
        self.restore_in_flight.clear();
        self.restore_queue.clear();
        vec![effect]
    }

    pub(in crate::application) fn adopt_startup(
        &mut self,
        loaded: LoadedConfiguration,
        session: SessionId,
    ) -> Vec<Effect> {
        let id = loaded.active.id;
        let name = loaded
            .index
            .workspaces
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.name.clone());
        if loaded.preferences.last_workspace_id != Some(id) || name.is_none() {
            crate::diagnostics::session().record(
                crate::diagnostics::Subsystem::Storage,
                crate::diagnostics::Activity::CheckWorkspaceReference,
                crate::diagnostics::Outcome::InvalidConfiguration,
                0,
                0,
                None,
            );
            self.persistence = recovery("Cannot load workspace: the active reference does not match the index/workspace. Inspect the preserved configuration and backups, then retry.".into(), None);
            return Vec::new();
        }
        self.workspace = loaded.active.workspace.snapshot();
        self.configuration = Some((loaded.preferences, loaded.index));
        self.user_themes = loaded.user_themes;
        self.user_themes_edited = false;
        self.user_themes_notice = loaded.user_themes_notice;
        self.status = None;
        self.loads.clear();
        self.persistence = if loaded.recovery_issues.is_empty() {
            PersistenceState::Ready {
                session,
                id,
                revision: 0,
                saved_revision: 0,
                attempt: 0,
                error: None,
                closing: CloseStage::Open,
                notice: loaded.notice,
            }
        } else {
            let details = loaded
                .recovery_issues
                .iter()
                .take(8)
                .map(|issue| {
                    format!(
                        "{} record {}: {}",
                        issue.document,
                        issue.record_index + 1,
                        issue.reason
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            recovery(
                format!(
                    "Loaded a viewing copy with {} recovery issues. Autosave is blocked.\n\n{}\n\n{}Use a validated backup if offered, or inspect the preserved configuration before retrying.",
                    loaded.recovery_issues.len(),
                    details,
                    if loaded.recovery_issues.len() > 8 {
                        "Showing the first eight issues. "
                    } else {
                        ""
                    }
                ),
                loaded.backup,
            )
        };
        self.schedule_restore_reads()
    }

    pub(in crate::application) fn startup_failed(&mut self, error: StorageFailure) {
        let backup = match &error {
            StorageFailure::BackupAvailable { offer, .. } => Some(offer.clone()),
            _ => None,
        };
        self.persistence = recovery(format!("Cannot load workspace: {error}"), backup);
    }
}

fn recovery(message: String, backup: Option<BackupOffer>) -> PersistenceState {
    PersistenceState::Recovery {
        message,
        dirty: false,
        closing: false,
        backup,
    }
}
