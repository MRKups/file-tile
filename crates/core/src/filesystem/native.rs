use std::fs;
use std::path::Path;

use crate::application::contracts::directory::{
    Cancellation, DirectoryEntry, DirectoryError, DirectoryIssue, DirectoryListing,
    DirectoryReader, EntryKind, ListingOrigin,
};

/// std::fs immediate-child reads. Production callers use the blocking worker service.
#[derive(Debug, Default)]
pub struct NativeDirectory;

impl DirectoryReader for NativeDirectory {
    fn metadata(
        &self,
        path: &Path,
        cancellation: &Cancellation,
    ) -> Result<
        crate::application::files::metadata::FileMetadata,
        crate::application::files::metadata::MetadataError,
    > {
        use crate::application::files::metadata::{FileMetadata, MetadataError};
        if cancellation.is_cancelled() {
            return Err(MetadataError::Cancelled);
        }
        let metadata = fs::metadata(path).map_err(MetadataError::from_io)?;
        if cancellation.is_cancelled() {
            return Err(MetadataError::Cancelled);
        }
        Ok(FileMetadata {
            bytes: metadata.is_file().then_some(metadata.len()),
            modified: metadata.modified().map_err(MetadataError::from_io),
        })
    }
    fn resolve_directory(
        &self,
        directory: &Path,
        cancellation: &Cancellation,
    ) -> Result<std::path::PathBuf, DirectoryError> {
        cancellation.check(directory)?;
        let resolved = fs::canonicalize(directory)
            .map_err(|error| DirectoryError::from_io(directory.into(), error))?;
        let metadata = fs::metadata(&resolved)
            .map_err(|error| DirectoryError::from_io(directory.into(), error))?;
        if !metadata.is_dir() {
            return Err(DirectoryError::new(
                directory.into(),
                crate::application::contracts::directory::DirectoryErrorKind::NotDirectory,
            ));
        }
        cancellation.check(directory)?;
        Ok(resolved)
    }
    fn list(&self, directory: &Path) -> Result<DirectoryListing, DirectoryError> {
        self.list_cancellable(directory, &Cancellation::default())
    }

    fn list_cancellable(
        &self,
        directory: &Path,
        cancellation: &Cancellation,
    ) -> Result<DirectoryListing, DirectoryError> {
        cancellation.check(directory)?;
        let resolved = fs::canonicalize(directory)
            .map_err(|error| DirectoryError::from_io(directory.to_path_buf(), error))?;
        cancellation.check(directory)?;
        let children = fs::read_dir(&resolved)
            .map_err(|error| DirectoryError::from_io(directory.to_path_buf(), error))?;
        let mut entries = Vec::new();
        let mut issues = Vec::new();
        let mut links = std::collections::HashMap::new();
        for child in children {
            cancellation.check(directory)?;
            match child {
                Ok(child) => {
                    let name = child.file_name();
                    let path = child.path();
                    let hidden = name.as_encoded_bytes().first() == Some(&b'.');
                    let (kind, symlink) = classify(&child, &mut issues);
                    if symlink && kind == EntryKind::Directory {
                        match fs::canonicalize(&path) {
                            Ok(target) => {
                                links.insert(path.clone(), target);
                            }
                            Err(error) => issues.push(DirectoryIssue {
                                error: DirectoryError::from_io(path.clone(), error),
                                entry_omitted: false,
                            }),
                        }
                    }
                    #[cfg(unix)]
                    let inode = {
                        use std::os::unix::fs::DirEntryExt;
                        Some(child.ino())
                    };
                    #[cfg(not(unix))]
                    let inode = None;
                    entries.push(DirectoryEntry {
                        name,
                        path,
                        kind,
                        hidden,
                        symlink,
                        inode,
                    });
                }
                Err(error) => issues.push(DirectoryIssue {
                    error: DirectoryError::from_io(resolved.clone(), error),
                    entry_omitted: true,
                }),
            }
        }
        cancellation.check(directory)?;
        let listing = DirectoryListing::new(resolved, ListingOrigin::Local, entries, issues)
            .with_resolved_links(links);
        cancellation.check(directory)?;
        Ok(listing)
    }
}

fn classify(entry: &fs::DirEntry, issues: &mut Vec<DirectoryIssue>) -> (EntryKind, bool) {
    match entry.file_type() {
        Ok(kind) if kind.is_symlink() => match fs::metadata(entry.path()) {
            Ok(metadata) => (entry_kind(metadata.file_type()), true),
            Err(error) => {
                issues.push(DirectoryIssue {
                    error: DirectoryError::from_io(entry.path(), error),
                    entry_omitted: false,
                });
                (EntryKind::Other, true)
            }
        },
        Ok(kind) => (entry_kind(kind), false),
        Err(error) => {
            issues.push(DirectoryIssue {
                error: DirectoryError::from_io(entry.path(), error),
                entry_omitted: false,
            });
            (EntryKind::Other, false)
        }
    }
}

fn entry_kind(kind: fs::FileType) -> EntryKind {
    if kind.is_dir() {
        EntryKind::Directory
    } else if kind.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    }
}
