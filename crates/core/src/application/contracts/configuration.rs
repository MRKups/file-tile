//! Application-owned configuration data, independent of JSON and storage paths.

use std::fmt;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::domain::Workspace;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkspaceId(Uuid);

impl WorkspaceId {
    pub fn random() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn parse(value: &str) -> Result<Self, InvalidWorkspaceId> {
        let id = Uuid::parse_str(value).map_err(|_| InvalidWorkspaceId)?;
        if id.is_nil() || id.hyphenated().to_string() != value {
            return Err(InvalidWorkspaceId);
        }
        Ok(Self(id))
    }
}

impl fmt::Display for WorkspaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.hyphenated().fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidWorkspaceId;

impl fmt::Display for InvalidWorkspaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Use a canonical lowercase, hyphenated, non-nil workspace UUID")
    }
}

impl std::error::Error for InvalidWorkspaceId {}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppPreferences {
    pub last_workspace_id: Option<WorkspaceId>,
    pub window: Option<crate::domain::WindowPlacement>,
    pub new_box_folder: Option<PathBuf>,
    pub pinned_folders: Option<Vec<PathBuf>>,
    pub appearance: crate::application::preferences::appearance::AppearancePreferences,
    pub typography: crate::application::preferences::typography::TypographyPreferences,
    pub safety: crate::domain::SafetyPreferences,
    pub capabilities: crate::domain::CapabilityPreferences,
}

pub const MAX_PATH_BYTES: usize = 4096;
pub const MAX_PINNED_FOLDERS: usize = 128;

/// Returns the default app-wide pinned sidebar folders (`Home`, `Desktop`,
/// `Documents`, `Downloads`) rooted at `home`.
pub fn default_pinned_folders(home: &Path) -> Vec<PathBuf> {
    [
        home.to_path_buf(),
        home.join("Desktop"),
        home.join("Documents"),
        home.join("Downloads"),
    ]
    .into_iter()
    .filter(|path| validate_saved_path(path).is_ok())
    .collect()
}

/// Shared saved-path structure, without accessing or normalizing the filesystem.
pub fn validate_saved_path(path: &Path) -> Result<(), &'static str> {
    if !path.is_absolute() {
        return Err("saved paths must be absolute");
    }
    let bytes = path.as_os_str().as_encoded_bytes();
    if bytes.len() > MAX_PATH_BYTES || bytes.contains(&0) {
        return Err("path exceeds its byte limit or contains a null byte");
    }
    Ok(())
}

pub fn parse_new_box_folder(input: &str, home: &Path) -> Result<Option<PathBuf>, String> {
    if input.contains(['\n', '\r', '\0']) {
        return Err(
            "Folder paths cannot contain line breaks or null characters. Edit the path and retry."
                .into(),
        );
    }
    let input = input.trim();
    if input.is_empty() {
        return Ok(None);
    }
    if !input.starts_with('~') && !Path::new(input).is_absolute() {
        return Err("Enter an absolute folder path or ~/path, or leave blank for Home.".into());
    }
    let path = crate::domain::expand_path(input, home, home).map_err(|error| error.to_string())?;
    validate_saved_path(&path).map_err(|_| "Cannot apply starting folder: the path is invalid or exceeds 4,096 bytes. Shorten or edit it and retry.".to_owned())?;
    Ok(Some(path))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSummary {
    pub id: WorkspaceId,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceIndex {
    pub workspaces: Vec<WorkspaceSummary>,
}

impl WorkspaceIndex {
    pub const MAX_WORKSPACES: usize = 128;
    pub const MAX_NAME_CHARACTERS: usize = crate::domain::validation::MAX_CONFIG_NAME_CHARS;

    pub fn valid_name(name: &str) -> bool {
        crate::domain::validation::is_valid_config_name(name)
    }
}

#[derive(Debug, Clone)]
pub struct WorkspaceDocument {
    pub id: WorkspaceId,
    pub workspace: Workspace,
}

#[derive(Debug, Clone)]
pub enum ConfigurationDocument {
    Preferences(AppPreferences),
    WorkspaceIndex(WorkspaceIndex),
    Workspace(WorkspaceDocument),
}
