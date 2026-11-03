//! Same-parent exclusive native rename. Never fall back to replacing rename/move.
use rustix::{
    fs::{self, AtFlags, Mode, OFlags, RenameFlags},
    io::Errno,
};
use std::path::Path;

pub enum RenameError {
    Conflict,
    Failed(String),
}

pub fn rename(
    source: &Path,
    destination: &Path,
    parent: (u64, u64),
    original: (u64, u64),
) -> Result<(), RenameError> {
    let directory = source
        .parent()
        .filter(|directory| Some(*directory) == destination.parent())
        .ok_or_else(|| {
            RenameError::Failed(
                "Rename must stay in the same folder. Choose a filename, not a path.".into(),
            )
        })?;
    let from = source.file_name().ok_or_else(|| {
        RenameError::Failed("Rename source has no filename. Reload the folder.".into())
    })?;
    let to = destination.file_name().ok_or_else(|| {
        RenameError::Failed("Rename destination has no filename. Choose another name.".into())
    })?;
    let fd = fs::open(
        directory,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| failed("open parent for rename", error))?;
    let actual_parent = fs::fstat(&fd).map_err(|error| failed("inspect rename parent", error))?;
    let actual_source = fs::statat(&fd, from, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|error| failed("inspect rename source", error))?;
    if identity(&actual_parent) != parent || identity(&actual_source) != original {
        return Err(RenameError::Failed(
            "Rename source or parent was replaced. Reload the folder before retrying.".into(),
        ));
    }
    // On macOS rustix maps NOREPLACE to renameatx_np(..., RENAME_EXCL).
    // Both names are relative to one retained parent descriptor. A failed or
    // unsupported exclusive rename must never become copy/delete or plain rename.
    fs::renameat_with(&fd, from, &fd, to, RenameFlags::NOREPLACE).map_err(|error| {
        if error == Errno::EXIST || error == Errno::NOTEMPTY {
            RenameError::Conflict
        } else {
            failed("rename without replacement", error)
        }
    })?;
    let actual = fs::statat(&fd, to, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|error| failed("confirm rename", error))?;
    if identity(&actual) != original {
        return Err(RenameError::Failed("Rename completed but destination identity changed. Inspect both names before retrying.".into()));
    }
    Ok(())
}

fn identity(stat: &fs::Stat) -> (u64, u64) {
    (stat.st_dev as u64, stat.st_ino)
}

fn failed(operation: &str, error: Errno) -> RenameError {
    RenameError::Failed(format!(
        "Cannot {operation}: {error}. Inspect the original and new names before retrying. Exclusive rename may be unsupported on this filesystem; no replacement fallback was attempted."
    ))
}
