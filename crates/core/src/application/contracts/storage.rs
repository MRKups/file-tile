//! Application-owned persistence messages. No JSON, filesystem, or Iced types.

use std::fmt;
use std::io::ErrorKind;
use std::sync::Arc;

use crate::application::contracts::configuration::{
    AppPreferences, WorkspaceDocument, WorkspaceId, WorkspaceIndex,
};
use crate::application::preferences::appearance::UserTheme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionId(pub WorkspaceId);

#[derive(Debug, Clone)]
pub struct LoadedConfiguration {
    pub preferences: AppPreferences,
    pub index: WorkspaceIndex,
    pub active: WorkspaceDocument,
    pub user_themes: Arc<[UserTheme]>,
    pub user_themes_notice: Option<String>,
    pub recovery_issues: Vec<RecoveryNotice>,
    pub backup: Option<BackupOffer>,
    pub notice: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupOffer {
    pub token: WorkspaceId,
    pub documents: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum StartupOperation {
    Retry,
    RestoreBackup(WorkspaceId),
}

#[derive(Debug, Clone)]
pub struct RecoveryNotice {
    pub document: &'static str,
    pub record_index: usize,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveTag {
    pub session: SessionId,
    pub workspace: WorkspaceId,
    pub revision: u64,
    pub attempt: u64,
}

#[derive(Debug, Clone)]
pub struct SaveRequest {
    pub tag: SaveTag,
    pub document: Arc<WorkspaceDocument>,
    pub preferences: AppPreferences,
    pub workspace_name: String,
    pub user_themes: Option<Arc<[UserTheme]>>,
}

#[derive(Debug, Clone)]
pub struct BudgetRequest {
    pub generation: u64,
    pub document: Arc<WorkspaceDocument>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceCommand {
    Select(WorkspaceId),
    Create { id: WorkspaceId, name: String },
    Rename { id: WorkspaceId, name: String },
    Remove(WorkspaceId),
}

#[derive(Debug, Clone)]
pub struct ManagementRequest {
    pub operation: WorkspaceId,
    pub attempt: u64,
    pub next_session: SessionId,
    pub source: SaveRequest,
    pub command: WorkspaceCommand,
    pub cancel: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManagementTag {
    pub operation: WorkspaceId,
    pub attempt: u64,
}

impl ManagementRequest {
    pub fn receipt_tag(&self) -> ManagementTag {
        ManagementTag {
            operation: self.operation,
            attempt: self.attempt,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ManagementResult {
    pub index: WorkspaceIndex,
    pub selected: Option<Box<LoadedConfiguration>>,
    pub source_saved: bool,
    pub source_error: Option<StorageFailure>,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveOutcome {
    Saved,
    Superseded,
}

#[derive(Debug, Clone)]
pub enum StorageCompletion {
    Managed {
        operation: ManagementTag,
        result: Result<ManagementResult, StorageFailure>,
    },
    BudgetChecked {
        generation: u64,
        result: Result<(), StorageFailure>,
    },
    Loaded {
        session: SessionId,
        result: Result<Box<LoadedConfiguration>, StorageFailure>,
    },
    Saved {
        tag: SaveTag,
        result: Result<SaveOutcome, StorageFailure>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageFailure {
    Busy,
    UnsafePath,
    Io(ErrorKind),
    InvalidConfiguration(String),
    ChangedExternally,
    ReadOnly,
    StaleRequest,
    Unavailable,
    BackupAvailable {
        cause: Box<StorageFailure>,
        offer: BackupOffer,
    },
}

impl StorageFailure {
    pub(crate) fn diagnostic_outcome(&self) -> crate::diagnostics::Outcome {
        use crate::diagnostics::Outcome;
        match self {
            Self::Busy => Outcome::Busy,
            Self::UnsafePath => Outcome::UnsafePath,
            Self::Io(kind) => Outcome::Io(*kind),
            Self::InvalidConfiguration(_) => Outcome::InvalidConfiguration,
            Self::ChangedExternally => Outcome::ExternalChange,
            Self::ReadOnly => Outcome::ReadOnly,
            Self::StaleRequest => Outcome::Stale,
            Self::Unavailable => Outcome::Unavailable,
            Self::BackupAvailable { .. } => Outcome::BackupAvailable,
        }
    }
    pub fn category(&self) -> &'static str {
        match self {
            Self::Busy => "writer_busy",
            Self::UnsafePath => "unsafe_path",
            Self::Io(_) => "io",
            Self::InvalidConfiguration(_) => "invalid_configuration",
            Self::ChangedExternally => "external_change",
            Self::ReadOnly => "read_only",
            Self::StaleRequest => "stale_request",
            Self::Unavailable => "worker_unavailable",
            Self::BackupAvailable { .. } => "backup_available",
        }
    }
}

impl fmt::Display for StorageFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => f.write_str("Another file-tile session owns this state directory. Close that session, then choose Retry."),
            Self::UnsafePath => f.write_str("A managed storage path is symlinked or is not a regular file/directory. Preserve it and inspect the state directory."),
            Self::Io(kind) => write!(f, "Storage I/O failed ({kind:?}). Check space and access to the state directory, then retry."),
            Self::InvalidConfiguration(reason) => write!(f, "{}. Autosave is blocked. Inspect the preserved configuration and backups, then retry after repairing the files.", reason.trim_end_matches('.')),
            Self::ChangedExternally => f.write_str("Configuration changed outside this session. Autosave is blocked. Preserve the files and relaunch to inspect the new configuration."),
            Self::ReadOnly => f.write_str("Recovery is read-only. Use a validated backup if offered, or inspect the preserved configuration before retrying."),
            Self::StaleRequest => f.write_str("The storage request no longer belongs to the current workspace revision. Retry from the current session."),
            Self::Unavailable => f.write_str("The storage worker is unavailable. Keep this session open and collect application error output before relaunching."),
            Self::BackupAvailable { cause, .. } => cause.fmt(f),
        }
    }
}

impl std::error::Error for StorageFailure {}

impl From<std::io::Error> for StorageFailure {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.kind())
    }
}

#[derive(Debug)]
pub enum PersistenceState {
    Off,
    Loading(SessionId),
    Ready {
        session: SessionId,
        id: WorkspaceId,
        revision: u64,
        saved_revision: u64,
        attempt: u64,
        error: Option<StorageFailure>,
        closing: CloseStage,
        notice: Option<String>,
    },
    Recovery {
        message: String,
        dirty: bool,
        closing: bool,
        backup: Option<BackupOffer>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseStage {
    Open,
    SamplingWindow,
    Flushing,
    Failed,
}
