//! Blocking disk boundary. Only the storage worker owns a production Store.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

use crate::application::contracts::configuration::{
    AppPreferences, ConfigurationDocument, WorkspaceDocument, WorkspaceId, WorkspaceIndex,
    WorkspaceSummary,
};
use crate::application::contracts::storage::{
    LoadedConfiguration, RecoveryNotice, SaveRequest, SessionId, StorageFailure,
};
use crate::application::preferences::appearance::UserTheme;
use crate::domain::Workspace;

use super::files::{
    Temporary, checked_file, directory, open_or_create, read_bounded, sync_directory,
};
use super::recovery::RecoveryPlan;
use super::{ConfigKey, Decoded, decode, encode};
mod management;

pub(super) struct Store {
    root: PathBuf,
    config: PathBuf,
    _lock: File,
    active: Option<(SessionId, WorkspaceId)>,
    writable: bool,
    baselines: Vec<(ConfigKey, Vec<u8>)>,
    user_themes: Arc<[UserTheme]>,
    user_themes_baseline: Option<Vec<u8>>,
    user_themes_notice: Option<String>,
    recovery: Option<RecoveryPlan>,
    management: Option<management::Pending>,
    #[cfg(test)]
    pub(super) fault: Option<Stage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage {
    TemporarySynced,
    BackupPublished,
    PrimaryPublished,
    WorkspaceInitialized,
    IndexInitialized,
    ManagementWorkspace,
    ManagementIndex,
    ManagementPreferences,
}

impl Store {
    #[cfg(test)]
    pub(super) fn active_owner(&self) -> Option<(SessionId, WorkspaceId)> {
        self.active
    }
    pub(super) fn open(root: &Path) -> Result<Self, StorageFailure> {
        if !root.is_absolute() {
            return Err(StorageFailure::UnsafePath);
        }
        directory(root)?;
        let config = root.join("config");
        directory(&config)?;
        let lock_path = config.join(".writer.lock");
        let lock = open_or_create(&lock_path)?;
        lock.try_lock().map_err(|error| match error {
            fs::TryLockError::WouldBlock => StorageFailure::Busy,
            fs::TryLockError::Error(error) => error.into(),
        })?;
        Ok(Self {
            root: root.into(),
            config,
            _lock: lock,
            active: None,
            writable: false,
            baselines: Vec::new(),
            user_themes: Arc::from([]),
            user_themes_baseline: None,
            user_themes_notice: None,
            recovery: None,
            management: None,
            #[cfg(test)]
            fault: None,
        })
    }

    pub(super) fn load(
        &mut self,
        session: SessionId,
    ) -> Result<LoadedConfiguration, StorageFailure> {
        self.check_paths()?;
        self.active = None;
        self.writable = false;
        self.baselines.clear();
        self.recovery = None;
        let mut result = self.load_primary(session);
        if !result
            .as_ref()
            .is_ok_and(|loaded| loaded.recovery_issues.is_empty())
        {
            self.recovery = RecoveryPlan::inspect(&self.config).ok().flatten();
            if let Some(plan) = &self.recovery {
                match &mut result {
                    Ok(loaded) => loaded.backup = Some(plan.offer()),
                    Err(error) => {
                        *error = StorageFailure::BackupAvailable {
                            cause: Box::new(error.clone()),
                            offer: plan.offer(),
                        }
                    }
                }
            }
        }
        result
    }

    fn load_primary(&mut self, session: SessionId) -> Result<LoadedConfiguration, StorageFailure> {
        let fresh = fs::read_dir(&self.config)?.try_fold(true, |empty, entry| {
            let entry = entry?;
            Ok::<_, std::io::Error>(empty && entry.file_name() == ".writer.lock")
        })?;
        if fresh {
            self.initialize()?;
        }
        let preferences = self.read(ConfigKey::Preferences)?;
        let index = self.read(ConfigKey::WorkspaceIndex)?;
        let mut recovery_issues = notices(preferences.issues, ConfigKey::Preferences);
        recovery_issues.extend(notices(index.issues, ConfigKey::WorkspaceIndex));
        let ConfigurationDocument::Preferences(preferences) = preferences.document else {
            return Err(invalid("Expected preferences."));
        };
        let ConfigurationDocument::WorkspaceIndex(index) = index.document else {
            return Err(invalid("Expected a workspace index."));
        };
        let id = preferences
            .last_workspace_id
            .ok_or_else(|| invalid("The last-active workspace reference is missing."))?;
        if !index.workspaces.iter().any(|item| item.id == id) {
            return Err(invalid("The last-active workspace is not in the index."));
        }
        let active = self.read(ConfigKey::Workspace(id))?;
        recovery_issues.extend(notices(active.issues, ConfigKey::Workspace(id)));
        let ConfigurationDocument::Workspace(active) = active.document else {
            return Err(invalid("Expected a workspace."));
        };
        self.active = Some((session, id));
        self.writable = recovery_issues.is_empty();
        let (user_themes, baseline, user_themes_notice) =
            super::user_themes::load_optional(&self.config);
        self.user_themes = Arc::clone(&user_themes);
        self.user_themes_baseline = baseline;
        self.user_themes_notice.clone_from(&user_themes_notice);
        Ok(LoadedConfiguration {
            preferences,
            index,
            active,
            user_themes,
            user_themes_notice,
            recovery_issues,
            backup: None,
            notice: None,
        })
    }

    pub(super) fn restore(
        &mut self,
        session: SessionId,
        token: WorkspaceId,
    ) -> Result<LoadedConfiguration, StorageFailure> {
        self.check_paths()?;
        if !self
            .recovery
            .as_ref()
            .is_some_and(|plan| plan.token == token)
        {
            return Err(StorageFailure::StaleRequest);
        }
        let plan = self.recovery.take().ok_or(StorageFailure::StaleRequest)?;
        self.writable = false;
        let count = plan.restore(&self.config)?;
        let mut loaded = self.load(session)?;
        loaded.notice = Some(format!(
            "Restored {count} configuration document(s) from validated backups. Previous files are preserved beside the configuration as .recovery files. Backup files are unchanged."
        ));
        Ok(loaded)
    }

    fn initialize(&mut self) -> Result<(), StorageFailure> {
        let id = WorkspaceId::random();
        // A footprint from any interrupted publication makes the next startup
        // recovery-only. Never roll back/delete it to pretend setup was fresh.
        for document in [
            ConfigurationDocument::Workspace(WorkspaceDocument {
                id,
                workspace: Workspace::default(),
            }),
            ConfigurationDocument::WorkspaceIndex(WorkspaceIndex {
                workspaces: vec![WorkspaceSummary {
                    id,
                    name: "Main".into(),
                }],
            }),
            ConfigurationDocument::Preferences(AppPreferences {
                last_workspace_id: Some(id),
                window: None,
                new_box_folder: None,
                appearance: Default::default(),
                typography: Default::default(),
                safety: Default::default(),
            }),
        ] {
            let key = match &document {
                ConfigurationDocument::Workspace(_) => ConfigKey::Workspace(id),
                ConfigurationDocument::WorkspaceIndex(_) => ConfigKey::WorkspaceIndex,
                ConfigurationDocument::Preferences(_) => ConfigKey::Preferences,
            };
            let bytes = encode(&document).map_err(|error| invalid(error.reason()))?;
            self.publish(key, &bytes, None)?;
            match key {
                ConfigKey::Workspace(_) => self.check_stage(Stage::WorkspaceInitialized)?,
                ConfigKey::WorkspaceIndex => self.check_stage(Stage::IndexInitialized)?,
                ConfigKey::Preferences => {}
            }
        }
        Ok(())
    }

    fn read(&mut self, key: ConfigKey) -> Result<Decoded, StorageFailure> {
        let bytes = read_bounded(&self.config.join(key.filename()), key.byte_limit())?;
        let decoded = decode(&bytes, key)
            .map_err(|error| invalid(&format!("{}: {}.", key.filename(), error.reason())))?;
        self.baselines.push((key, bytes));
        Ok(decoded)
    }

    pub(super) fn save(&mut self, request: &SaveRequest) -> Result<(), StorageFailure> {
        if self.management.is_some() {
            return Err(StorageFailure::StaleRequest);
        }
        self.check_paths()?;
        if self.active != Some((request.tag.session, request.tag.workspace))
            || request.document.id != request.tag.workspace
            || request.preferences.last_workspace_id != Some(request.tag.workspace)
        {
            return Err(StorageFailure::StaleRequest);
        }
        if !self.writable {
            return Err(StorageFailure::ReadOnly);
        }
        let workspace_bytes = encode(&ConfigurationDocument::Workspace(
            (*request.document).clone(),
        ))
        .map_err(|error| invalid(error.reason()))?;
        let preferences_bytes = encode(&ConfigurationDocument::Preferences(
            request.preferences.clone(),
        ))
        .map_err(|error| invalid(error.reason()))?;
        // Naming is the only index mutation this save port permits. Derive it
        // from the loaded baseline, never a UI-supplied replacement index.
        let index_baseline = self
            .baselines
            .iter()
            .find(|(key, _)| *key == ConfigKey::WorkspaceIndex)
            .map(|(_, bytes)| bytes)
            .ok_or(StorageFailure::ReadOnly)?;
        let ConfigurationDocument::WorkspaceIndex(mut index) =
            decode(index_baseline, ConfigKey::WorkspaceIndex)
                .map_err(|error| invalid(error.reason()))?
                .document
        else {
            return Err(StorageFailure::ReadOnly);
        };
        if !WorkspaceIndex::valid_name(&request.workspace_name) {
            return Err(invalid(
                "Workspace name must contain 1 to 32 letters, numbers, single spaces, hyphens, or underscores without leading or trailing spaces.",
            ));
        }
        let summary = index
            .workspaces
            .iter_mut()
            .find(|item| item.id == request.tag.workspace)
            .ok_or(StorageFailure::StaleRequest)?;
        let index_bytes = if summary.name != request.workspace_name {
            summary.name.clone_from(&request.workspace_name);
            Some(
                encode(&ConfigurationDocument::WorkspaceIndex(index))
                    .map_err(|error| invalid(error.reason()))?,
            )
        } else {
            None
        };
        for (key, baseline) in &self.baselines {
            let current = read_bounded(&self.config.join(key.filename()), key.byte_limit())?;
            if &current != baseline {
                self.writable = false;
                return Err(StorageFailure::ChangedExternally);
            }
        }
        self.publish_loaded(ConfigKey::Workspace(request.tag.workspace), workspace_bytes)?;
        if let Some(bytes) = index_bytes {
            self.publish_loaded(ConfigKey::WorkspaceIndex, bytes)?;
        }
        if let Some(themes) = &request.user_themes {
            self.user_themes =
                super::user_themes::publish(&self.config, themes, &mut self.user_themes_baseline)?;
            self.user_themes_notice = None;
        }
        self.publish_loaded(ConfigKey::Preferences, preferences_bytes)
    }

    fn publish_loaded(&mut self, key: ConfigKey, bytes: Vec<u8>) -> Result<(), StorageFailure> {
        let baseline = self
            .baselines
            .iter()
            .find(|(old, _)| *old == key)
            .map(|(_, bytes)| bytes.clone())
            .ok_or(StorageFailure::ReadOnly)?;
        let result = if bytes == baseline {
            // A close barrier for already published state must not replace its
            // useful previous backup with another copy of the current document.
            checked_file(&self.config.join(key.filename()), false)?.sync_all()?;
            sync_directory(&self.config)
        } else {
            self.publish(key, &bytes, Some(&baseline))
        };
        // Rename can succeed before directory synchronization fails. Track the
        // bytes we published, but do not acknowledge durability on that error.
        if read_bounded(&self.config.join(key.filename()), key.byte_limit())
            .is_ok_and(|current| current == bytes)
            && let Some((_, baseline)) = self.baselines.iter_mut().find(|(old, _)| *old == key)
        {
            *baseline = bytes;
        }
        result
    }

    fn publish(
        &self,
        key: ConfigKey,
        bytes: &[u8],
        previous: Option<&[u8]>,
    ) -> Result<(), StorageFailure> {
        self.check_paths()?;
        let target = self.config.join(key.filename());
        let temporary = Temporary::write(&self.config, bytes)?;
        self.check_stage(Stage::TemporarySynced)?;
        if let Some(previous) = previous {
            let backup = self.config.join(format!("{}.bak", key.filename()));
            if fs::symlink_metadata(&backup).is_ok() {
                let old = read_bounded(&backup, key.byte_limit())?;
                let decoded = decode(&old, key).map_err(|error| {
                    invalid(&format!("{}.bak: {}.", key.filename(), error.reason()))
                })?;
                if !decoded.can_save() {
                    return Err(StorageFailure::ReadOnly);
                }
            } else if backup.try_exists()? {
                return Err(StorageFailure::UnsafePath);
            }
            let backup_temporary = Temporary::write(&self.config, previous)?;
            fs::rename(&backup_temporary.path, &backup)?;
            sync_directory(&self.config)?;
            self.check_stage(Stage::BackupPublished)?;
        } else if fs::symlink_metadata(&target).is_ok() {
            return Err(StorageFailure::ChangedExternally);
        }
        if let Some(previous) = previous
            && read_bounded(&target, key.byte_limit())? != previous
        {
            return Err(StorageFailure::ChangedExternally);
        }
        fs::rename(&temporary.path, &target)?;
        self.check_stage(Stage::PrimaryPublished)?;
        sync_directory(&self.config)
    }

    fn check_stage(&self, stage: Stage) -> Result<(), StorageFailure> {
        use crate::diagnostics::{Activity, Outcome, Subsystem, session};
        let activity = match stage {
            Stage::TemporarySynced => Activity::TemporarySynced,
            Stage::BackupPublished => Activity::BackupPublished,
            Stage::PrimaryPublished => Activity::PrimaryPublished,
            Stage::WorkspaceInitialized => Activity::WorkspaceInitialized,
            Stage::IndexInitialized => Activity::IndexInitialized,
            Stage::ManagementWorkspace => Activity::ManagementWorkspace,
            Stage::ManagementIndex => Activity::ManagementIndex,
            Stage::ManagementPreferences => Activity::ManagementPreferences,
        };
        session().record(Subsystem::Storage, activity, Outcome::Complete, 0, 0, None);
        #[cfg(test)]
        if self.fault == Some(stage) {
            return Err(StorageFailure::Io(std::io::ErrorKind::Other));
        }
        if !fs::symlink_metadata(&self.config)?.is_dir() {
            return Err(StorageFailure::UnsafePath);
        }
        Ok(())
    }

    fn check_paths(&self) -> Result<(), StorageFailure> {
        for path in [&self.root, &self.config] {
            if !fs::symlink_metadata(path)?.is_dir() {
                return Err(StorageFailure::UnsafePath);
            }
        }
        let lock = checked_file(&self.config.join(".writer.lock"), false)?;
        #[cfg(unix)]
        {
            let held = self._lock.metadata()?;
            let current = lock.metadata()?;
            if held.dev() != current.dev() || held.ino() != current.ino() {
                return Err(StorageFailure::ChangedExternally);
            }
        }
        drop(lock);
        Ok(())
    }
}

fn invalid(reason: &str) -> StorageFailure {
    StorageFailure::InvalidConfiguration(reason.into())
}

fn notices(issues: Vec<super::RecoveryIssue>, key: ConfigKey) -> Vec<RecoveryNotice> {
    issues
        .into_iter()
        .map(|issue| RecoveryNotice {
            document: key.kind(),
            record_index: issue.record_index,
            reason: issue.reason,
        })
        .collect()
}
