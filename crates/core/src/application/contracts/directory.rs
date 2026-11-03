//! The directory port and records owned by the application, not by an engine or UI.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::domain::ViewId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryKind {
    Directory,
    File,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryEntry {
    pub name: OsString,
    pub path: PathBuf,
    pub kind: EntryKind,
    pub hidden: bool,
    pub symlink: bool,
    pub inode: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryListing {
    pub directory: PathBuf,
    pub origin: ListingOrigin,
    entries: Vec<DirectoryEntry>,
    visible_indices: Vec<usize>,
    path_indices: Arc<[usize]>,
    /// Nonempty means incomplete, even when no usable entries remain.
    pub issues: Vec<DirectoryIssue>,
    resolved_links: std::collections::HashMap<PathBuf, PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListingOrigin {
    Local,
    Controlled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryIssue {
    pub error: DirectoryError,
    pub entry_omitted: bool,
}

impl DirectoryListing {
    /// Application-owned ordering/filter projection. Adapters call this off-thread.
    pub fn new(
        directory: PathBuf,
        origin: ListingOrigin,
        mut entries: Vec<DirectoryEntry>,
        issues: Vec<DirectoryIssue>,
    ) -> Self {
        entries.sort_by(|a, b| {
            (a.kind != EntryKind::Directory)
                .cmp(&(b.kind != EntryKind::Directory))
                .then_with(|| a.name.cmp(&b.name))
        });
        let visible_indices = entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| (!entry.hidden).then_some(index))
            .collect();
        // Index the final ordering without duplicating native paths. Cloned
        // listings share this immutable projection and keep the same positions.
        let mut path_indices: Vec<_> = (0..entries.len()).collect();
        path_indices.sort_unstable_by(|a, b| {
            entries[*a]
                .path
                .cmp(&entries[*b].path)
                .then_with(|| a.cmp(b))
        });
        Self {
            directory,
            origin,
            entries,
            visible_indices,
            path_indices: path_indices.into(),
            issues,
            resolved_links: std::collections::HashMap::new(),
        }
    }

    pub fn entries(&self) -> &[DirectoryEntry] {
        &self.entries
    }
    /// Native-path lookup, including hidden entries. Duplicate paths return the
    /// first entry in listing order; visibility and scope remain caller rules.
    pub fn entry(&self, path: &Path) -> Option<&DirectoryEntry> {
        let position = self
            .path_indices
            .partition_point(|index| self.entries[*index].path.as_path() < path);
        self.path_indices
            .get(position)
            .and_then(|index| self.entries.get(*index))
            .filter(|entry| entry.path == path)
    }
    pub fn with_resolved_links(
        mut self,
        links: std::collections::HashMap<PathBuf, PathBuf>,
    ) -> Self {
        self.resolved_links = links;
        self
    }
    pub fn entry_target<'a>(&'a self, entry: &'a DirectoryEntry) -> Option<&'a Path> {
        if entry.symlink {
            self.resolved_links.get(&entry.path).map(PathBuf::as_path)
        } else {
            Some(&entry.path)
        }
    }
    pub fn visible_len(&self) -> usize {
        self.visible_indices.len()
    }
    pub fn display_len(&self, show_hidden: bool) -> usize {
        if show_hidden {
            self.entries.len()
        } else {
            self.visible_len()
        }
    }
    pub fn display_entry(&self, index: usize, show_hidden: bool) -> Option<&DirectoryEntry> {
        if show_hidden {
            self.entries.get(index)
        } else {
            self.visible_entry(index)
        }
    }
    pub fn visible_entry(&self, index: usize) -> Option<&DirectoryEntry> {
        self.visible_indices
            .get(index)
            .and_then(|index| self.entries.get(*index))
    }
    pub fn omitted_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|issue| issue.entry_omitted)
            .count()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryErrorKind {
    NotFound,
    NotDirectory,
    PermissionDenied,
    Unavailable,
    Other,
    Cancelled,
    Busy,
    OutsideScope,
}
impl DirectoryErrorKind {
    pub(crate) fn diagnostic_outcome(self) -> crate::diagnostics::Outcome {
        use crate::diagnostics::Outcome;
        match self {
            Self::NotFound => Outcome::NotFound,
            Self::NotDirectory => Outcome::NotDirectory,
            Self::PermissionDenied => Outcome::PermissionDenied,
            Self::Unavailable => Outcome::Unavailable,
            Self::Other => Outcome::Failed,
            Self::Cancelled => Outcome::Cancelled,
            Self::Busy => Outcome::Busy,
            Self::OutsideScope => Outcome::OutsideScope,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryError {
    pub path: PathBuf,
    pub kind: DirectoryErrorKind,
    pub detail: Option<String>,
}

impl DirectoryError {
    pub fn describe(&self, operation: &str) -> String {
        let reason = match self.kind {
            DirectoryErrorKind::NotFound => "the folder was not found",
            DirectoryErrorKind::NotDirectory => "the path is not a folder",
            DirectoryErrorKind::PermissionDenied => "access was denied",
            DirectoryErrorKind::Unavailable => "the location is unavailable",
            DirectoryErrorKind::Other => "the directory read failed",
            DirectoryErrorKind::Cancelled => "the request was cancelled",
            DirectoryErrorKind::Busy => "directory workers are busy",
            DirectoryErrorKind::OutsideScope => {
                "the resolved folder is outside this view's navigation scope"
            }
        };
        format!(
            "Cannot {operation} {}: {reason}{}. Choose another folder or retry.",
            self.path.display(),
            self.detail
                .as_ref()
                .map(|detail| format!(" ({detail})"))
                .unwrap_or_default()
        )
    }
    pub fn new(path: PathBuf, kind: DirectoryErrorKind) -> Self {
        Self {
            path,
            kind,
            detail: None,
        }
    }

    pub fn from_io(path: PathBuf, error: std::io::Error) -> Self {
        let kind = match error.kind() {
            std::io::ErrorKind::NotFound => DirectoryErrorKind::NotFound,
            std::io::ErrorKind::NotADirectory => DirectoryErrorKind::NotDirectory,
            std::io::ErrorKind::PermissionDenied => DirectoryErrorKind::PermissionDenied,
            _ => DirectoryErrorKind::Other,
        };
        Self {
            path,
            kind,
            detail: Some(error.to_string()),
        }
    }
}

impl std::fmt::Display for DirectoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.describe("list"))
    }
}

impl std::error::Error for DirectoryError {}

/// Immediate children only. Real implementations must be called by owned blocking
/// workers, never from rendering or the UI thread. The controlled adapter is memory-only.
pub trait DirectoryReader {
    fn metadata(
        &self,
        _path: &Path,
        _cancellation: &Cancellation,
    ) -> Result<
        crate::application::files::metadata::FileMetadata,
        crate::application::files::metadata::MetadataError,
    > {
        Err(crate::application::files::metadata::MetadataError::Unsupported)
    }
    fn list(&self, directory: &Path) -> Result<DirectoryListing, DirectoryError>;

    fn list_cancellable(
        &self,
        directory: &Path,
        cancellation: &Cancellation,
    ) -> Result<DirectoryListing, DirectoryError> {
        cancellation.check(directory)?;
        let result = self.list(directory);
        cancellation.check(directory)?;
        result
    }

    fn resolve_directory(
        &self,
        directory: &Path,
        cancellation: &Cancellation,
    ) -> Result<PathBuf, DirectoryError> {
        self.list_cancellable(directory, cancellation)
            .map(|listing| listing.directory)
    }
    fn list_scoped(
        &self,
        directory: &Path,
        scope: &crate::domain::Scope,
        cancellation: &Cancellation,
    ) -> Result<DirectoryListing, DirectoryError> {
        for boundary in scope.boundary_paths() {
            let resolved = self.resolve_directory(&boundary, cancellation)?;
            if resolved != boundary {
                return Err(DirectoryError::new(
                    boundary,
                    DirectoryErrorKind::OutsideScope,
                ));
            }
        }
        let listing = self.list_cancellable(directory, cancellation)?;
        if !scope.allows(&listing.directory) {
            return Err(DirectoryError::new(
                directory.into(),
                DirectoryErrorKind::OutsideScope,
            ));
        }
        Ok(listing)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Cancellation {
    cancelled: Arc<AtomicBool>,
    shutdown: Option<Arc<AtomicBool>>,
}

impl Cancellation {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
            || self
                .shutdown
                .as_ref()
                .is_some_and(|flag| flag.load(Ordering::Acquire))
    }
    pub fn child(&self) -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            shutdown: Some(Arc::clone(&self.cancelled)),
        }
    }
    pub fn check(&self, path: &Path) -> Result<(), DirectoryError> {
        if self.is_cancelled() {
            Err(DirectoryError::new(
                path.to_path_buf(),
                DirectoryErrorKind::Cancelled,
            ))
        } else {
            Ok(())
        }
    }
}

/// Globally increasing tags are never reused across view lifetimes or refreshes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListingRequest {
    pub view: ViewId,
    pub generation: u64,
    pub directory: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ListingCompletion {
    pub request: ListingRequest,
    pub result: Result<DirectoryListing, DirectoryError>,
}
