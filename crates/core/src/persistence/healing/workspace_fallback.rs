//! Scenarios:
//! - `ConfigHealingScenario::MissingActiveWorkspaceFallback`
//! - `ConfigHealingScenario::EmptyWorkspaceRecreation`
//!
//! Handles the case where `preferences.json` and `workspaces.json` are valid,
//! but the active `workspace-<UUID>.json` file was deleted from `config/` and
//! has no `.bak` backup file to restore from.

use std::path::Path;

use crate::application::contracts::configuration::{
    ConfigurationDocument, WorkspaceDocument, WorkspaceId, WorkspaceIndex,
};
use crate::application::contracts::storage::StorageFailure;
use crate::domain::Workspace;

use super::ConfigHealingScenario;
use crate::persistence::files::read_bounded;
use crate::persistence::{ConfigKey, Decoded, decode, encode};

pub(in crate::persistence) struct HealedWorkspace {
    pub scenario: ConfigHealingScenario,
    pub id: WorkspaceId,
    pub bytes: Vec<u8>,
    pub document: WorkspaceDocument,
    pub notice: String,
}

/// Attempts to heal a missing (`NotFound`) active workspace file when no `.bak`
/// exists for that workspace.
pub(in crate::persistence) fn heal_missing_active_workspace(
    config_dir: &Path,
    missing_id: WorkspaceId,
    index: &WorkspaceIndex,
) -> Result<Option<HealedWorkspace>, StorageFailure> {
    let backup_name = format!("{}.bak", ConfigKey::Workspace(missing_id).filename());
    if config_dir.join(backup_name).exists() {
        // Defer to RecoveryPlan when a backup of the missing workspace exists.
        return Ok(None);
    }

    // Scenario 1: Another indexed workspace has a clean file on disk.
    for summary in &index.workspaces {
        if summary.id == missing_id {
            continue;
        }
        let key = ConfigKey::Workspace(summary.id);
        let path = config_dir.join(key.filename());
        let Ok(bytes) = read_bounded(&path, key.byte_limit()) else {
            continue;
        };
        let Ok(Decoded { document, issues }) = decode(&bytes, key) else {
            continue;
        };
        if !issues.is_empty() {
            continue;
        }
        if let ConfigurationDocument::Workspace(document) = document {
            return Ok(Some(HealedWorkspace {
                scenario: ConfigHealingScenario::MissingActiveWorkspaceFallback,
                id: summary.id,
                bytes,
                document,
                notice: format!(
                    "Active workspace file {} was missing. Switched to workspace \"{}\".",
                    ConfigKey::Workspace(missing_id).filename(),
                    summary.name
                ),
            }));
        }
    }

    // Scenario 2: No other valid workspace exists; recreate an empty workspace
    // for the missing active UUID so startup succeeds cleanly.
    let document = WorkspaceDocument {
        id: missing_id,
        workspace: Workspace::default(),
    };
    let bytes = encode(&ConfigurationDocument::Workspace(document.clone()))
        .map_err(|error| StorageFailure::InvalidConfiguration(error.reason().into()))?;
    Ok(Some(HealedWorkspace {
        scenario: ConfigHealingScenario::EmptyWorkspaceRecreation,
        id: missing_id,
        bytes,
        document,
        notice: format!(
            "Active workspace file {} was missing and had no backup. Recreated an empty workspace.",
            ConfigKey::Workspace(missing_id).filename()
        ),
    }))
}
