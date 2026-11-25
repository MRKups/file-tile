//! Versioned JSON, ordered disk storage, and explicit backup recovery.

mod appearance;
mod codec;
mod files;
pub mod healing;
mod launchers;
mod migrations;
mod path;
mod recovery;
mod scope;
mod sorting;
mod store;
#[cfg(test)]
mod tests;
mod typography;
mod unique_value;
mod user_themes;
mod worker;

pub use codec::encode;
pub use worker::StorageWorker;

use std::fmt;

use crate::application::contracts::configuration::{ConfigurationDocument, WorkspaceId};
use codec::decode;

const FORMAT: &str = "file-tile.config";
const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigKey {
    Preferences,
    WorkspaceIndex,
    Workspace(WorkspaceId),
}

impl ConfigKey {
    fn filename(self) -> String {
        match self {
            Self::Preferences => "preferences.json".into(),
            Self::WorkspaceIndex => "workspaces.json".into(),
            Self::Workspace(id) => format!("workspace-{id}.json"),
        }
    }

    fn byte_limit(self) -> usize {
        match self {
            Self::Preferences => 64 * 1024,
            Self::WorkspaceIndex => 256 * 1024,
            Self::Workspace(_) => 8 * 1024 * 1024,
        }
    }

    fn kind(self) -> &'static str {
        match self {
            Self::Preferences => "preferences",
            Self::WorkspaceIndex => "workspace_index",
            Self::Workspace(_) => "workspace",
        }
    }
}

#[derive(Debug)]
struct Decoded {
    document: ConfigurationDocument,
    issues: Vec<RecoveryIssue>,
}

impl Decoded {
    fn can_save(&self) -> bool {
        self.issues.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecoveryIssue {
    record_index: usize,
    reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    TooLarge,
    InvalidJson,
    InvalidHeader,
    UnsupportedFormat,
    UnsupportedVersion(u64),
    WrongDocument,
    InvalidPayload,
    LimitExceeded,
    InvalidState(String),
}

impl ConfigError {
    pub fn reason(&self) -> &str {
        match self {
            Self::TooLarge => "configuration exceeds its byte limit",
            Self::InvalidJson => "JSON is malformed, too deeply nested, or has duplicate keys",
            Self::InvalidHeader => "the shared schema header is missing or invalid",
            Self::UnsupportedFormat => "the format is unsupported",
            Self::UnsupportedVersion(_) => "the schema version is unsupported",
            Self::WrongDocument => "the document kind or workspace UUID does not match its target",
            Self::InvalidPayload => "required fields are invalid or unknown fields are present",
            Self::LimitExceeded => "a configuration collection exceeds its limit",
            Self::InvalidState(reason) => reason,
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Cannot process configuration: {}. Preserve the original; inspect the configuration or use a compatible application version.",
            self.reason()
        )
    }
}

impl std::error::Error for ConfigError {}
