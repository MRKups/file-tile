//! Sequential schema migration boundary for configuration documents.
//!
//! All configuration files (`preferences.json`, `workspaces.json`,
//! `workspace-<UUID>.json`, and `user-themes.json`) share a single integer
//! `schema_version` in their JSON header.
//!
//! When `SCHEMA_VERSION` is incremented for a breaking schema change:
//! 1. Add a confined step module (for example `v1_to_v2.rs`) in this directory.
//! 2. Register a [`MigrationStep`] in [`STEPS`] that transforms the raw JSON
//!    payload map from version `N` to `N + 1` per [`DocumentKind`].
//! 3. Keep [`super::codec`] strict for the latest `SCHEMA_VERSION` only.

use serde_json::{Map, Value};

use super::{ConfigError, SCHEMA_VERSION};

/// Identifies which configuration document is being migrated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DocumentKind {
    Preferences,
    WorkspaceIndex,
    Workspace,
    UserThemes,
}

impl DocumentKind {
    fn parse(kind: &str) -> Option<Self> {
        match kind {
            "preferences" => Some(Self::Preferences),
            "workspace_index" => Some(Self::WorkspaceIndex),
            "workspace" => Some(Self::Workspace),
            "user_themes" => Some(Self::UserThemes),
            _ => None,
        }
    }
}

/// Pure single-version transformation step from `from_version` to `from_version + 1`.
pub(super) struct MigrationStep {
    pub from_version: u32,
    pub to_version: u32,
    pub apply: fn(DocumentKind, &mut Map<String, Value>) -> Result<(), ConfigError>,
}

/// Ordered registry of schema migrations from `v1` up to [`SCHEMA_VERSION`].
const STEPS: &[MigrationStep] = &[];

/// Upgrades a validated header payload from `version` to [`SCHEMA_VERSION`].
pub(super) fn upgrade(version: u64, kind: &str, payload: Value) -> Result<Value, ConfigError> {
    if version == 0 || version > u64::from(SCHEMA_VERSION) {
        return Err(ConfigError::UnsupportedVersion(version));
    }
    let mut current =
        u32::try_from(version).map_err(|_| ConfigError::UnsupportedVersion(version))?;
    if current == SCHEMA_VERSION {
        return Ok(payload);
    }
    let doc_kind = DocumentKind::parse(kind).ok_or(ConfigError::WrongDocument)?;
    let Value::Object(mut fields) = payload else {
        return Err(ConfigError::InvalidHeader);
    };
    while current < SCHEMA_VERSION {
        let step = STEPS
            .iter()
            .find(|step| step.from_version == current && step.to_version == current + 1)
            .ok_or(ConfigError::UnsupportedVersion(version))?;
        (step.apply)(doc_kind, &mut fields)?;
        current = step.to_version;
    }
    Ok(Value::Object(fields))
}
