use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::application::contracts::directory::{
    DirectoryEntry, DirectoryError, DirectoryErrorKind, DirectoryListing, DirectoryReader,
    EntryKind, ListingOrigin,
};
use crate::domain::normalize_path;

/// A small fictitious tree. Listing this adapter never reads a host path.
#[derive(Debug)]
pub struct ControlledDirectory {
    directories: HashMap<PathBuf, Vec<DirectoryEntry>>,
}

impl Default for ControlledDirectory {
    fn default() -> Self {
        let mut directories = HashMap::new();
        for &(parent, folders, files) in TREE {
            let entries = folders
                .iter()
                .copied()
                .map(|name| (name, EntryKind::Directory))
                .chain(files.iter().copied().map(|name| (name, EntryKind::File)))
                .map(|(name, kind)| DirectoryEntry {
                    name: name.into(),
                    path: Path::new(parent).join(name),
                    kind,
                    hidden: name.starts_with('.'),
                    symlink: false,
                    inode: None,
                })
                .collect();
            directories.insert(parent.into(), entries);
        }
        Self { directories }
    }
}

const TREE: &[(&str, &[&str], &[&str])] = &[
    ("/", &["Users", "Volumes"], &[]),
    ("/Users", &["demo"], &[]),
    (
        "/Users/demo",
        &[
            "Documents",
            "Projects",
            "Downloads",
            "Pictures",
            "Desktop",
            "Music",
            ".config",
        ],
        &[],
    ),
    (
        "/Users/demo/Documents",
        &["Notes", "Invoices"],
        &["Brief.pdf", "Readme.md", ".DS_Store"],
    ),
    (
        "/Users/demo/Documents/Notes",
        &[],
        &["Ideas.md", "Meeting notes.txt"],
    ),
    ("/Users/demo/Documents/Invoices", &[], &["October.pdf"]),
    ("/Users/demo/Projects", &["App", "Website", "Archive"], &[]),
    (
        "/Users/demo/Projects/App",
        &["Assets", "src"],
        &["README.md"],
    ),
    ("/Users/demo/Projects/App/Assets", &["Icons"], &[]),
    (
        "/Users/demo/Projects/App/Assets/Icons",
        &[],
        &["Folder.svg"],
    ),
    (
        "/Users/demo/Projects/App/src",
        &["components"],
        &["main.rs"],
    ),
    (
        "/Users/demo/Projects/App/src/components",
        &[],
        &["workspace.rs"],
    ),
    (
        "/Users/demo/Projects/Website",
        &["Content", "Design"],
        &["index.html"],
    ),
    ("/Users/demo/Projects/Website/Content", &["Posts"], &[]),
    (
        "/Users/demo/Projects/Website/Content/Posts",
        &[],
        &["Welcome.md"],
    ),
    ("/Users/demo/Projects/Website/Design", &[], &["Layout.svg"]),
    (
        "/Users/demo/Projects/Archive",
        &["2025"],
        &["Old notes.txt"],
    ),
    ("/Users/demo/Projects/Archive/2025", &[], &["Prototype.zip"]),
    (
        "/Users/demo/Downloads",
        &[],
        &["Reference.pdf", "Sample.zip", "Screenshot.png"],
    ),
    ("/Users/demo/Pictures", &["Screenshots", "Travel"], &[]),
    ("/Users/demo/Pictures/Screenshots", &[], &["Workspace.png"]),
    ("/Users/demo/Pictures/Travel", &[], &["Coast.jpg"]),
    ("/Users/demo/Desktop", &[], &["Workspace notes.md"]),
    ("/Users/demo/Music", &[], &[]),
    ("/Users/demo/.config", &[], &["preferences.json"]),
    ("/Volumes", &["Work"], &[]),
    ("/Volumes/Work", &["Shared"], &[]),
    ("/Volumes/Work/Shared", &[], &["Team notes.md"]),
];

impl DirectoryReader for ControlledDirectory {
    fn metadata(
        &self,
        path: &Path,
        cancellation: &crate::application::contracts::directory::Cancellation,
    ) -> Result<
        crate::application::files::metadata::FileMetadata,
        crate::application::files::metadata::MetadataError,
    > {
        use crate::application::files::metadata::{FileMetadata, MetadataError};
        if cancellation.is_cancelled() {
            return Err(MetadataError::Cancelled);
        }
        let entry = self
            .directories
            .values()
            .flatten()
            .find(|entry| entry.path == path)
            .ok_or(MetadataError::Missing)?;
        Ok(FileMetadata {
            bytes: (entry.kind == EntryKind::File).then_some(1234),
            modified: Ok(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000)),
            created: Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_690_000_000)),
            item_count: (entry.kind == EntryKind::Directory).then_some(4),
            dimensions: None,
        })
    }
    fn list(&self, directory: &Path) -> Result<DirectoryListing, DirectoryError> {
        let normalized = normalize_path(directory);
        let directory = normalized.as_path();
        if let Some(entries) = self.directories.get(directory) {
            return Ok(DirectoryListing::new(
                normalized,
                ListingOrigin::Controlled,
                entries.clone(),
                vec![],
            ));
        }
        let is_file = self
            .directories
            .values()
            .flatten()
            .any(|entry| entry.path == directory && entry.kind == EntryKind::File);
        Err(DirectoryError::new(
            directory.to_path_buf(),
            if is_file {
                DirectoryErrorKind::NotDirectory
            } else {
                DirectoryErrorKind::NotFound
            },
        ))
    }
}
