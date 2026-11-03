//! Restrictive, bounded filesystem primitives shared by publication and recovery.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};

use crate::application::contracts::configuration::WorkspaceId;
use crate::application::contracts::storage::StorageFailure;

pub(super) fn directory(path: &Path) -> Result<(), StorageFailure> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    builder.mode(0o700);
    builder.create(path)?;
    if !fs::symlink_metadata(path)?.is_dir() {
        return Err(StorageFailure::UnsafePath);
    }
    Ok(())
}

fn options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    options
}

pub(super) fn open_or_create(path: &Path) -> Result<File, StorageFailure> {
    match options().create_new(true).open(path) {
        Ok(file) => Ok(file),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => checked_file(path, true),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn checked_file(path: &Path, writable: bool) -> Result<File, StorageFailure> {
    let before = fs::symlink_metadata(path)?;
    if !before.is_file() {
        return Err(StorageFailure::UnsafePath);
    }
    let file = OpenOptions::new().read(true).write(writable).open(path)?;
    let after = fs::symlink_metadata(path)?;
    if !after.is_file() {
        return Err(StorageFailure::UnsafePath);
    }
    #[cfg(unix)]
    {
        let opened = file.metadata()?;
        if before.dev() != opened.dev()
            || before.ino() != opened.ino()
            || after.dev() != opened.dev()
            || after.ino() != opened.ino()
        {
            return Err(StorageFailure::ChangedExternally);
        }
    }
    Ok(file)
}

pub(super) fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, StorageFailure> {
    let file = checked_file(path, false)?;
    if file.metadata()?.len() > limit as u64 {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(too_large());
    }
    Ok(bytes)
}

pub(super) fn read_optional(path: &Path, limit: usize) -> Result<Option<Vec<u8>>, StorageFailure> {
    match read_bounded(path, limit) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(StorageFailure::Io(std::io::ErrorKind::NotFound)) => Ok(None),
        Err(error) => Err(error),
    }
}

fn too_large() -> StorageFailure {
    StorageFailure::InvalidConfiguration("Configuration exceeds its byte limit.".into())
}

pub(super) fn sync_directory(path: &Path) -> Result<(), StorageFailure> {
    File::open(path)?.sync_all().map_err(Into::into)
}

pub(super) struct Temporary {
    pub(super) path: PathBuf,
}

impl Temporary {
    pub(super) fn write(directory: &Path, bytes: &[u8]) -> Result<Self, StorageFailure> {
        let path = directory.join(format!(".file-tile-{}.tmp", WorkspaceId::random()));
        let mut file = options().create_new(true).open(&path)?;
        let temporary = Self { path };
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(temporary)
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!(
                "Cannot remove an owned persistence temporary file ({:?}). Inspect the state directory.",
                error.kind()
            );
        }
    }
}
