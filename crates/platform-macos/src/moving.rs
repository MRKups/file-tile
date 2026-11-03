//! Native exclusive move between retained parent directories, with verified cross-volume source cleanup.
use objc2_foundation::NSFileManager;
use rustix::{
    fs::{self, AtFlags, Mode, OFlags, RenameFlags},
    io::Errno,
};
use std::path::Path;

pub enum MoveError {
    Conflict,
    CrossVolume,
    Failed(String),
}

pub fn move_entry(
    source: &Path,
    destination: &Path,
    source_parent: (u64, u64),
    destination_parent: (u64, u64),
    original: (u64, u64),
) -> Result<(), MoveError> {
    let (Some(from_parent), Some(to_parent), Some(from), Some(to)) = (
        source.parent(),
        destination.parent(),
        source.file_name(),
        destination.file_name(),
    ) else {
        return Err(MoveError::Failed(
            "Cannot move a filesystem root. Choose a file or folder inside it.".into(),
        ));
    };
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let from_fd = fs::open(from_parent, flags, Mode::empty())
        .map_err(|error| failed("open source folder", error))?;
    let to_fd = fs::open(to_parent, flags, Mode::empty())
        .map_err(|error| failed("open destination folder", error))?;
    let from_stat = fs::fstat(&from_fd).map_err(|error| failed("inspect source folder", error))?;
    let to_stat = fs::fstat(&to_fd).map_err(|error| failed("inspect destination folder", error))?;
    let entry = fs::statat(&from_fd, from, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|error| failed("inspect move source", error))?;
    if identity(&from_stat) != source_parent
        || identity(&to_stat) != destination_parent
        || identity(&entry) != original
    {
        return Err(MoveError::Failed(
            "Move source or parent folder was replaced. Inspect both folders before retrying."
                .into(),
        ));
    }
    // On macOS this is renameatx_np(..., RENAME_EXCL), an OS operation which
    // cannot overwrite a concurrently published destination. Never downgrade it.
    fs::renameat_with(&from_fd, from, &to_fd, to, RenameFlags::NOREPLACE).map_err(|error| {
        match error {
            Errno::EXIST | Errno::NOTEMPTY => MoveError::Conflict,
            Errno::XDEV => MoveError::CrossVolume,
            error => failed("move without replacement", error),
        }
    })?;
    let output = fs::statat(&to_fd, to, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|error| failed("confirm moved entry", error))?;
    if identity(&output) != original {
        return Err(MoveError::Failed("Move completed but destination identity changed. Inspect both folders before retrying.".into()));
    }
    Ok(())
}

pub fn remove_verified_source(
    source: &Path,
    source_parent: (u64, u64),
    original: (u64, u64),
    is_dir: bool,
) -> Result<(), String> {
    let (Some(from_parent), Some(from)) = (source.parent(), source.file_name()) else {
        return Err("Cannot remove a filesystem root.".into());
    };
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let from_fd = fs::open(from_parent, flags, Mode::empty())
        .map_err(|error| format!("Cannot open source folder for cleanup: {error}"))?;
    let from_stat = fs::fstat(&from_fd)
        .map_err(|error| format!("Cannot inspect source folder for cleanup: {error}"))?;
    let entry = fs::statat(&from_fd, from, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|error| format!("Cannot inspect source entry for cleanup: {error}"))?;
    if identity(&from_stat) != source_parent || identity(&entry) != original {
        return Err("Source or parent folder changed before cleanup; both the copy and original were retained.".into());
    }
    if is_dir {
        objc2::rc::autoreleasepool(|_| {
            let url = super::file_urls::encode_url(
                source,
                "Cannot encode source folder URL for cleanup.",
            )?;
            NSFileManager::new()
                .removeItemAtURL_error(&url)
                .map_err(|error| {
                    format!(
                        "Cannot remove original folder after cross-volume copy: {}",
                        error.localizedDescription()
                    )
                })
        })?;
    } else {
        fs::unlinkat(&from_fd, from, AtFlags::empty()).map_err(|error| {
            format!("Cannot remove original file after cross-volume copy: {error}")
        })?;
    }
    match fs::statat(&from_fd, from, AtFlags::SYMLINK_NOFOLLOW) {
        Err(Errno::NOENT) => Ok(()),
        Ok(_) => Err("Original source still exists after cross-volume copy.".into()),
        Err(error) => Err(format!("Cannot confirm source removal: {error}")),
    }
}

fn identity(stat: &fs::Stat) -> (u64, u64) {
    (stat.st_dev as u64, stat.st_ino)
}

fn failed(operation: &str, error: Errno) -> MoveError {
    MoveError::Failed(format!(
        "Cannot {operation}: {error}. Inspect the source and destination before retrying; no copy/delete fallback was attempted."
    ))
}
