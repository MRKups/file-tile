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
        let dataless = is_dataless(&metadata);
        let inspect_contents =
            !dataless && crate::platform::volumes::is_local_measurable_mount(path);
        Ok(FileMetadata {
            bytes: metadata.is_file().then_some(metadata.len()),
            modified: metadata.modified().map_err(MetadataError::from_io),
            created: metadata.created().ok(),
            item_count: (metadata.is_dir() && inspect_contents)
                .then(|| read_folder_item_count(path, cancellation))
                .flatten(),
            dimensions: (metadata.is_file() && inspect_contents)
                .then(|| read_image_dimensions(path))
                .flatten(),
        })
    }
    fn sort_metadata(
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
            created: metadata.created().ok(),
            item_count: None,
            dimensions: None,
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
        let mount = mount_origin(&resolved);
        read_resolved_directory(directory, resolved, mount, cancellation)
    }

    fn list_permitted(
        &self,
        directory: &Path,
        scope: &crate::domain::Scope,
        safety: &crate::domain::SafetyPolicy,
        cancellation: &Cancellation,
    ) -> Result<DirectoryListing, DirectoryError> {
        if let Err(reason) = safety.check_read_directory(directory) {
            return Err(DirectoryError::access_disabled(
                directory.to_path_buf(),
                reason,
            ));
        }
        for boundary in scope.boundary_paths() {
            let resolved = self.resolve_directory(&boundary, cancellation)?;
            if resolved != boundary {
                return Err(DirectoryError::new(
                    boundary,
                    crate::application::contracts::directory::DirectoryErrorKind::OutsideScope,
                ));
            }
        }
        cancellation.check(directory)?;
        let resolved = fs::canonicalize(directory)
            .map_err(|error| DirectoryError::from_io(directory.to_path_buf(), error))?;
        cancellation.check(directory)?;
        if !scope.allows(&resolved) {
            return Err(DirectoryError::new(
                directory.into(),
                crate::application::contracts::directory::DirectoryErrorKind::OutsideScope,
            ));
        }
        let mount = mount_origin(&resolved);
        if let Some(reason) = safety.check_read_directory(&resolved).err().or_else(|| {
            crate::application::contracts::directory::check_mount_origin_access(
                &safety.preferences,
                &mount,
            )
            .err()
        }) {
            return Err(DirectoryError::access_disabled(
                directory.to_path_buf(),
                reason,
            ));
        }
        read_resolved_directory(directory, resolved, mount, cancellation)
    }
}

fn mount_origin(resolved: &Path) -> crate::capabilities::MountOrigin {
    match crate::platform::volumes::classify_mount(resolved) {
        crate::platform::volumes::VolumeMount::Local => crate::capabilities::MountOrigin::Local,
        crate::platform::volumes::VolumeMount::External { name } => {
            crate::capabilities::MountOrigin::External { name }
        }
        crate::platform::volumes::VolumeMount::Network { fstype, name } => {
            crate::capabilities::MountOrigin::Network { fstype, name }
        }
    }
}

fn read_resolved_directory(
    directory: &Path,
    resolved: std::path::PathBuf,
    mount: crate::capabilities::MountOrigin,
    cancellation: &Cancellation,
) -> Result<DirectoryListing, DirectoryError> {
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
                let (kind, symlink, dataless) = classify(&child, &mut issues);
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
                    dataless,
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
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let local_mount = crate::platform::volumes::is_local_measurable_mount(&resolved);
    let dir_dataless = fs::metadata(&resolved)
        .ok()
        .is_some_and(|m| is_dataless(&m));
    let capabilities = crate::capabilities::detect_in_context_measurable(
        directory,
        &resolved,
        home.as_deref(),
        mount.clone(),
        local_mount && !dir_dataless,
    );
    let measurable = local_mount && !dir_dataless && !entries.iter().any(|e| e.dataless);
    let listing = DirectoryListing::new(resolved, ListingOrigin::Local, entries, issues)
        .with_measurable(measurable)
        .with_resolved_links(links)
        .with_capabilities(capabilities)
        .with_mount(mount);
    cancellation.check(directory)?;
    Ok(listing)
}

fn classify(entry: &fs::DirEntry, issues: &mut Vec<DirectoryIssue>) -> (EntryKind, bool, bool) {
    match entry.file_type() {
        Ok(kind) if kind.is_symlink() => match fs::metadata(entry.path()) {
            Ok(metadata) => (
                entry_kind(metadata.file_type()),
                true,
                is_dataless(&metadata),
            ),
            Err(error) => {
                issues.push(DirectoryIssue {
                    error: DirectoryError::from_io(entry.path(), error),
                    entry_omitted: false,
                });
                (EntryKind::Other, true, false)
            }
        },
        Ok(kind) => {
            let dataless = entry.metadata().ok().is_some_and(|m| is_dataless(&m));
            (entry_kind(kind), false, dataless)
        }
        Err(error) => {
            issues.push(DirectoryIssue {
                error: DirectoryError::from_io(entry.path(), error),
                entry_omitted: false,
            });
            (EntryKind::Other, false, false)
        }
    }
}

fn is_dataless(metadata: &fs::Metadata) -> bool {
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        (metadata.st_flags() & 0x4000_0000) != 0
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = metadata;
        false
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

const FOLDER_ITEM_COUNT_LIMIT: u64 = 10_000;

fn read_folder_item_count(path: &Path, cancellation: &Cancellation) -> Option<u64> {
    if path.file_name().and_then(|name| name.to_str()) == Some("Library") {
        return None;
    }
    if crate::platform::permissions::check_path_access(path)
        != crate::platform::permissions::PermissionStatus::Granted
    {
        return None;
    }
    let entries = fs::read_dir(path).ok()?;
    let mut count = 0_u64;
    for entry in entries {
        if cancellation.is_cancelled() {
            return None;
        }
        let Ok(entry) = entry else {
            continue;
        };
        if entry.file_name() == ".DS_Store" {
            continue;
        }
        count = count.saturating_add(1);
        if count >= FOLDER_ITEM_COUNT_LIMIT {
            break;
        }
    }
    Some(count)
}

fn read_image_dimensions(path: &Path) -> Option<(u32, u32)> {
    if !crate::application::contracts::preview::image_candidate(path) {
        return None;
    }
    let reader = image::ImageReader::open(path).ok()?;
    let (width, height) = reader.into_dimensions().ok()?;
    (width > 0 && height > 0).then_some((width, height))
}
