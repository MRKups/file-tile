//! A retained operation makes partial creation and active-reference publication retryable.

use super::*;
use crate::application::contracts::storage::{
    ManagementRequest, ManagementResult, WorkspaceCommand,
};

pub(super) struct Pending {
    request: ManagementRequest,
    source_saved: bool,
    source_error: Option<StorageFailure>,
    created: Option<Vec<u8>>,
    preferences_changed: bool,
    index_updated: bool,
}

struct Target {
    document: WorkspaceDocument,
    bytes: Vec<u8>,
}

impl Store {
    pub(crate) fn manage(
        &mut self,
        request: &ManagementRequest,
    ) -> Result<ManagementResult, StorageFailure> {
        let mut pending = if let Some(pending) = self.management.take() {
            if pending.request.operation != request.operation
                || pending.request.command != request.command
                || pending.request.source.tag != request.source.tag
                || pending.request.next_session != request.next_session
                || request.attempt <= pending.request.attempt
            {
                self.management = Some(pending);
                return Err(StorageFailure::StaleRequest);
            }
            let mut pending = pending;
            pending.request.attempt = request.attempt;
            pending
        } else {
            if request.cancel
                || self.active != Some((request.source.tag.session, request.source.tag.workspace))
            {
                return Err(StorageFailure::StaleRequest);
            }
            Pending {
                request: request.clone(),
                source_saved: false,
                source_error: None,
                created: None,
                preferences_changed: false,
                index_updated: false,
            }
        };
        // Retain the operation before fallible path checks so a repaired store
        // can reconcile Cancel without losing the original request identity.
        let result = self.check_paths().and_then(|()| {
            if request.cancel {
                self.cancel_management(&pending)
            } else {
                self.advance_management(&mut pending)
            }
        });
        if !pending.source_saved
            && let Err(error) = &result
        {
            pending.source_error = Some(error.clone());
        }
        if result.is_err() {
            self.management = Some(pending);
        }
        result
    }

    fn loaded_index(&self) -> Result<WorkspaceIndex, StorageFailure> {
        let bytes = self
            .baselines
            .iter()
            .find(|(key, _)| *key == ConfigKey::WorkspaceIndex)
            .map(|(_, bytes)| bytes)
            .ok_or(StorageFailure::ReadOnly)?;
        let decoded =
            decode(bytes, ConfigKey::WorkspaceIndex).map_err(|error| invalid(error.reason()))?;
        if !decoded.can_save() {
            return Err(StorageFailure::ReadOnly);
        }
        match decoded.document {
            ConfigurationDocument::WorkspaceIndex(index) => Ok(index),
            _ => Err(StorageFailure::ReadOnly),
        }
    }

    fn verify_management_baselines(&mut self) -> Result<(), StorageFailure> {
        for (key, baseline) in &self.baselines {
            if read_bounded(&self.config.join(key.filename()), key.byte_limit())? != *baseline {
                self.writable = false;
                return Err(StorageFailure::ChangedExternally);
            }
        }
        Ok(())
    }

    fn cancel_management(&mut self, pending: &Pending) -> Result<ManagementResult, StorageFailure> {
        if pending.preferences_changed {
            // Rename may have happened before sync failed. Restore the source reference
            // through normal publication, never by deleting partially created records.
            self.verify_management_baselines()?;
            let bytes = encode(&ConfigurationDocument::Preferences(
                pending.request.source.preferences.clone(),
            ))
            .map_err(|error| invalid(error.reason()))?;
            self.publish_loaded(ConfigKey::Preferences, bytes)?;
        }
        Ok(ManagementResult {
            index: self.loaded_index()?,
            selected: None,
            source_saved: pending.source_saved,
            source_error: pending.source_error.clone(),
            cancelled: true,
        })
    }

    fn advance_management(
        &mut self,
        pending: &mut Pending,
    ) -> Result<ManagementResult, StorageFailure> {
        if !self.writable {
            return Err(StorageFailure::ReadOnly);
        }
        let command = pending.request.command.clone();
        let source_id = pending.request.source.tag.workspace;
        let mut index = self.loaded_index()?;
        self.validate_management_command(pending, &index, source_id)?;
        self.verify_management_baselines()?;
        if !pending.source_saved {
            let result = self.save(&pending.request.source);
            pending.source_error = result.as_ref().err().cloned();
            result?;
            pending.source_saved = true;
        }
        index = self.loaded_index()?;
        if matches!(
            command,
            WorkspaceCommand::Rename { .. } | WorkspaceCommand::Remove(_)
        ) {
            return self.apply_management_index(pending, index, command);
        }
        let target = match command {
            WorkspaceCommand::Select(id) => self.selected_management_target(id)?,
            WorkspaceCommand::Create { id, name } => {
                self.create_management_target(pending, &mut index, id, name)?
            }
            WorkspaceCommand::Rename { .. } | WorkspaceCommand::Remove(_) => {
                return Err(StorageFailure::StaleRequest);
            }
        };
        self.commit_management_target(pending, index, target)
    }

    fn apply_management_index(
        &mut self,
        pending: &mut Pending,
        mut index: WorkspaceIndex,
        command: WorkspaceCommand,
    ) -> Result<ManagementResult, StorageFailure> {
        match command {
            WorkspaceCommand::Remove(id) => index.workspaces.retain(|entry| entry.id != id),
            WorkspaceCommand::Rename { id, name } => {
                let entry = index
                    .workspaces
                    .iter_mut()
                    .find(|entry| entry.id == id)
                    .ok_or(StorageFailure::StaleRequest)?;
                entry.name = name;
            }
            WorkspaceCommand::Select(_) | WorkspaceCommand::Create { .. } => {
                return Err(StorageFailure::StaleRequest);
            }
        }
        let bytes = encode(&ConfigurationDocument::WorkspaceIndex(index.clone()))
            .map_err(|error| invalid(error.reason()))?;
        let result = self.publish_loaded(ConfigKey::WorkspaceIndex, bytes.clone());
        pending.index_updated = self
            .baselines
            .iter()
            .any(|(key, current)| *key == ConfigKey::WorkspaceIndex && *current == bytes);
        result?;
        self.check_stage(Stage::ManagementIndex)?;
        Ok(ManagementResult {
            index,
            selected: None,
            source_saved: true,
            source_error: None,
            cancelled: false,
        })
    }

    fn validate_management_command(
        &self,
        pending: &Pending,
        index: &WorkspaceIndex,
        source_id: WorkspaceId,
    ) -> Result<(), StorageFailure> {
        let command = &pending.request.command;
        match command {
            WorkspaceCommand::Select(id)
                if *id == source_id || !index.workspaces.iter().any(|entry| entry.id == *id) =>
            {
                return Err(StorageFailure::StaleRequest);
            }
            WorkspaceCommand::Create { id, name }
                if !WorkspaceIndex::valid_name(name)
                    || *id == source_id
                    || (pending.created.is_none()
                        && (index.workspaces.len() >= WorkspaceIndex::MAX_WORKSPACES
                            || index.workspaces.iter().any(|entry| entry.id == *id))) =>
            {
                return Err(invalid(
                    "Cannot create workspace: invalid name, identity, or workspace limit",
                ));
            }
            WorkspaceCommand::Rename { id, name }
                if *id == source_id
                    || !WorkspaceIndex::valid_name(name)
                    || !index.workspaces.iter().any(|entry| entry.id == *id) =>
            {
                return Err(StorageFailure::StaleRequest);
            }
            WorkspaceCommand::Remove(id)
                if *id == source_id
                    || (!pending.index_updated
                        && (index.workspaces.len() < 2
                            || !index.workspaces.iter().any(|entry| entry.id == *id))) =>
            {
                return Err(StorageFailure::StaleRequest);
            }
            _ => {}
        }
        Ok(())
    }

    fn selected_management_target(&self, id: WorkspaceId) -> Result<Target, StorageFailure> {
        let key = ConfigKey::Workspace(id);
        let bytes = read_bounded(&self.config.join(key.filename()), key.byte_limit())?;
        let decoded = decode(&bytes, key).map_err(|error| invalid(error.reason()))?;
        if !decoded.can_save() {
            return Err(invalid(
                "The selected workspace needs recovery. Preserve its file and backup",
            ));
        }
        let ConfigurationDocument::Workspace(document) = decoded.document else {
            return Err(StorageFailure::StaleRequest);
        };
        Ok(Target { document, bytes })
    }

    fn create_management_target(
        &mut self,
        pending: &mut Pending,
        index: &mut WorkspaceIndex,
        id: WorkspaceId,
        name: String,
    ) -> Result<Target, StorageFailure> {
        let key = ConfigKey::Workspace(id);
        let document = WorkspaceDocument {
            id,
            workspace: Workspace::default(),
        };
        let bytes = encode(&ConfigurationDocument::Workspace(document.clone()))
            .map_err(|error| invalid(error.reason()))?;
        if pending.created.is_none() {
            for filename in [key.filename(), format!("{}.bak", key.filename())] {
                match fs::symlink_metadata(self.config.join(filename)) {
                    Ok(_) => return Err(StorageFailure::ChangedExternally),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
        } else if read_bounded(&self.config.join(key.filename()), key.byte_limit())? != bytes {
            return Err(StorageFailure::ChangedExternally);
        }
        if !index.workspaces.iter().any(|entry| entry.id == id) {
            index.workspaces.push(WorkspaceSummary { id, name });
        }
        let index_bytes = encode(&ConfigurationDocument::WorkspaceIndex(index.clone()))
            .map_err(|error| invalid(error.reason()))?;
        let mut preferences = pending.request.source.preferences.clone();
        preferences.last_workspace_id = Some(id);
        encode(&ConfigurationDocument::Preferences(preferences))
            .map_err(|error| invalid(error.reason()))?;
        if pending.created.is_none() {
            let result = self.publish(key, &bytes, None);
            if read_bounded(&self.config.join(key.filename()), key.byte_limit())
                .is_ok_and(|current| current == bytes)
            {
                pending.created = Some(bytes.clone());
            }
            result?;
        } else {
            sync_directory(&self.config)?;
        }
        self.check_stage(Stage::ManagementWorkspace)?;
        self.publish_loaded(ConfigKey::WorkspaceIndex, index_bytes)?;
        self.check_stage(Stage::ManagementIndex)?;
        Ok(Target { document, bytes })
    }

    fn commit_management_target(
        &mut self,
        pending: &mut Pending,
        index: WorkspaceIndex,
        target: Target,
    ) -> Result<ManagementResult, StorageFailure> {
        let Target {
            document,
            bytes: target_bytes,
        } = target;
        let id = document.id;
        self.verify_management_baselines()?;
        if read_bounded(
            &self.config.join(ConfigKey::Workspace(id).filename()),
            ConfigKey::Workspace(id).byte_limit(),
        )? != target_bytes
        {
            return Err(StorageFailure::ChangedExternally);
        }
        let mut preferences = pending.request.source.preferences.clone();
        preferences.last_workspace_id = Some(id);
        let bytes = encode(&ConfigurationDocument::Preferences(preferences.clone()))
            .map_err(|error| invalid(error.reason()))?;
        let result = self.publish_loaded(ConfigKey::Preferences, bytes.clone());
        if self
            .baselines
            .iter()
            .any(|(key, current)| *key == ConfigKey::Preferences && *current == bytes)
        {
            pending.preferences_changed = true;
        }
        result?;
        self.check_stage(Stage::ManagementPreferences)?;
        self.baselines
            .retain(|(key, _)| !matches!(key, ConfigKey::Workspace(_)));
        self.baselines
            .push((ConfigKey::Workspace(id), target_bytes));
        self.active = Some((pending.request.next_session, id));
        Ok(ManagementResult {
            index: index.clone(),
            selected: Some(Box::new(LoadedConfiguration {
                preferences,
                index,
                active: document,
                user_themes: std::sync::Arc::clone(&self.user_themes),
                user_themes_notice: self.user_themes_notice.clone(),
                recovery_issues: vec![],
                backup: None,
                notice: None,
            })),
            source_saved: true,
            source_error: None,
            cancelled: false,
        })
    }
}
