//! Descriptor-pinned source tree snapshotting, pre-delete verification, and bottom-up `unlinkat`.

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use rustix::{
    fd::OwnedFd,
    fs::{self, AtFlags, Dir, FileType, Mode, OFlags, Stat},
    io::Errno,
};

const MAX_DEPTH: usize = 64;
const MAX_ENTRIES: usize = 262_144;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Fingerprint {
    size: u64,
    mtime_sec: i64,
    mtime_nsec: i64,
}

#[derive(Debug)]
struct FileEntry {
    name: OsString,
    id: (u64, u64),
    fingerprint: Fingerprint,
}

#[derive(Debug)]
struct DirNode {
    name: OsString,
    id: (u64, u64),
    files: Vec<FileEntry>,
    dirs: Vec<DirNode>,
}

/// Verified snapshot of a directory tree prior to cross-volume copy.
#[derive(Debug)]
pub struct SourceTreeSnapshot {
    root_id: (u64, u64),
    files: Vec<FileEntry>,
    dirs: Vec<DirNode>,
}

pub(super) fn stat_fingerprint(stat: &Stat) -> (u64, i64, i64) {
    (stat.st_size as u64, stat.st_mtime, stat.st_mtime_nsec)
}

fn dir_flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
}

fn open_verified_root(
    source: &Path,
    source_parent: (u64, u64),
    expected_root: (u64, u64),
    action: &str,
) -> Result<(OwnedFd, OsString, OwnedFd), String> {
    let (Some(from_parent), Some(from)) = (source.parent(), source.file_name()) else {
        return Err("Cannot inspect or remove a filesystem root.".into());
    };
    let flags = dir_flags();
    let from_fd = fs::open(from_parent, flags, Mode::empty())
        .map_err(|error| format!("Cannot open source folder for {action}: {error}"))?;
    let from_stat = fs::fstat(&from_fd)
        .map_err(|error| format!("Cannot inspect source folder for {action}: {error}"))?;
    let root_fd = fs::openat(&from_fd, from, flags, Mode::empty())
        .map_err(|error| format!("Cannot open source folder entry for {action}: {error}"))?;
    let root_stat = fs::fstat(&root_fd)
        .map_err(|error| format!("Cannot inspect source folder entry for {action}: {error}"))?;
    if super::identity(&from_stat) != source_parent || super::identity(&root_stat) != expected_root
    {
        return Err(
            "Source or parent folder changed; both the original source and destination copy were retained."
                .into(),
        );
    }
    Ok((from_fd, from.to_os_string(), root_fd))
}

/// Snapshots every entry in `source` using `openat` + `Dir::read_from` + `statat` without following symlinks.
pub fn snapshot_source_tree(
    source: &Path,
    source_parent: (u64, u64),
    original: (u64, u64),
) -> Result<SourceTreeSnapshot, String> {
    let (_from_fd, _from, root_fd) =
        open_verified_root(source, source_parent, original, "cross-volume snapshot")?;
    let mut count = 0usize;
    let (files, dirs) = snapshot_dir(&root_fd, 0, &mut count)?;
    Ok(SourceTreeSnapshot {
        root_id: original,
        files,
        dirs,
    })
}

fn snapshot_dir(
    dir_fd: &OwnedFd,
    depth: usize,
    count: &mut usize,
) -> Result<(Vec<FileEntry>, Vec<DirNode>), String> {
    if depth >= MAX_DEPTH {
        return Err(
            "Source folder is nested too deeply for safe cross-volume verification. Use Finder."
                .into(),
        );
    }
    let names = read_dir_names(dir_fd)?;
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    for name in names {
        *count = count.saturating_add(1);
        if *count > MAX_ENTRIES {
            return Err(
                "Source folder has too many entries for safe cross-volume verification. Use Finder."
                    .into(),
            );
        }
        let stat = fs::statat(dir_fd, &name, AtFlags::SYMLINK_NOFOLLOW).map_err(|error| {
            format!("Cannot inspect source item before cross-volume move: {error}")
        })?;
        let id = super::identity(&stat);
        if FileType::from_raw_mode(stat.st_mode).is_dir() {
            let child_fd =
                fs::openat(dir_fd, &name, dir_flags(), Mode::empty()).map_err(|error| {
                    format!("Cannot open source subfolder before cross-volume move: {error}")
                })?;
            let child_stat = fs::fstat(&child_fd).map_err(|error| {
                format!("Cannot verify source subfolder before cross-volume move: {error}")
            })?;
            if super::identity(&child_stat) != id {
                return Err(
                    "Source folder changed while inspecting entries; cross-volume move aborted."
                        .into(),
                );
            }
            let (sub_files, sub_dirs) = snapshot_dir(&child_fd, depth + 1, count)?;
            dirs.push(DirNode {
                name,
                id,
                files: sub_files,
                dirs: sub_dirs,
            });
        } else {
            let (size, mtime_sec, mtime_nsec) = stat_fingerprint(&stat);
            files.push(FileEntry {
                name,
                id,
                fingerprint: Fingerprint {
                    size,
                    mtime_sec,
                    mtime_nsec,
                },
            });
        }
    }
    Ok((files, dirs))
}

fn read_dir_names(dir_fd: &OwnedFd) -> Result<Vec<OsString>, String> {
    let dir = Dir::read_from(dir_fd)
        .map_err(|error| format!("Cannot read source folder entries: {error}"))?;
    let mut names = Vec::new();
    for entry in dir {
        let entry = entry.map_err(|error| format!("Cannot read source folder entries: {error}"))?;
        let bytes = entry.file_name().to_bytes();
        if bytes.is_empty() || bytes == b"." || bytes == b".." {
            continue;
        }
        names.push(OsStr::from_bytes(bytes).to_os_string());
    }
    Ok(names)
}

/// Verifies the entire `snapshot` matches disk before deleting a single entry, then removes bottom-up.
pub fn remove_verified_tree(
    source: &Path,
    source_parent: (u64, u64),
    snapshot: &SourceTreeSnapshot,
) -> Result<(), String> {
    let (_from_fd, _from, root_fd) = open_verified_root(
        source,
        source_parent,
        snapshot.root_id,
        "post-copy verification",
    )?;
    verify_dir(&root_fd, &snapshot.files, &snapshot.dirs)?;
    drop(root_fd);

    let (from_fd, from, root_fd) =
        open_verified_root(source, source_parent, snapshot.root_id, "cleanup")?;
    delete_dir(&root_fd, &snapshot.files, &snapshot.dirs)?;
    drop(root_fd);
    fs::unlinkat(&from_fd, &from, AtFlags::REMOVEDIR).map_err(|error| {
        format!("Cannot remove original folder after cross-volume copy: {error}. Remaining source items and the destination copy were retained.")
    })?;
    match fs::statat(&from_fd, &from, AtFlags::SYMLINK_NOFOLLOW) {
        Err(Errno::NOENT) => Ok(()),
        Ok(_) => Err("Original folder still exists after cross-volume copy.".into()),
        Err(error) => Err(format!("Cannot confirm source folder removal: {error}")),
    }
}

fn verify_dir(dir_fd: &OwnedFd, files: &[FileEntry], dirs: &[DirNode]) -> Result<(), String> {
    let actual: HashSet<OsString> = read_dir_names(dir_fd)?.into_iter().collect();
    if actual.len() != files.len().saturating_add(dirs.len()) {
        return Err(changed_during_copy());
    }
    for file in files {
        if !actual.contains(&file.name) {
            return Err(changed_during_copy());
        }
        let stat = fs::statat(dir_fd, &file.name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|_| changed_during_copy())?;
        if FileType::from_raw_mode(stat.st_mode).is_dir()
            || super::identity(&stat) != file.id
            || !fingerprint_matches(&stat, file.fingerprint)
        {
            return Err(changed_during_copy());
        }
    }
    for dir in dirs {
        if !actual.contains(&dir.name) {
            return Err(changed_during_copy());
        }
        let stat = fs::statat(dir_fd, &dir.name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|_| changed_during_copy())?;
        if !FileType::from_raw_mode(stat.st_mode).is_dir() || super::identity(&stat) != dir.id {
            return Err(changed_during_copy());
        }
        let child_fd = fs::openat(dir_fd, &dir.name, dir_flags(), Mode::empty())
            .map_err(|_| changed_during_copy())?;
        let child_stat = fs::fstat(&child_fd).map_err(|_| changed_during_copy())?;
        if super::identity(&child_stat) != dir.id {
            return Err(changed_during_copy());
        }
        verify_dir(&child_fd, &dir.files, &dir.dirs)?;
    }
    Ok(())
}

fn delete_dir(dir_fd: &OwnedFd, files: &[FileEntry], dirs: &[DirNode]) -> Result<(), String> {
    for dir in dirs {
        let child_fd = fs::openat(dir_fd, &dir.name, dir_flags(), Mode::empty()).map_err(
            |error| format!("Cannot open source subfolder for cleanup: {error}. Remaining source items and the destination copy were retained."),
        )?;
        let child_stat = fs::fstat(&child_fd).map_err(|_| changed_during_copy())?;
        if super::identity(&child_stat) != dir.id {
            return Err(changed_during_copy());
        }
        delete_dir(&child_fd, &dir.files, &dir.dirs)?;
        drop(child_fd);
        fs::unlinkat(dir_fd, &dir.name, AtFlags::REMOVEDIR).map_err(|error| {
            format!("Cannot remove original subfolder after cross-volume copy: {error}. Remaining source items and the destination copy were retained.")
        })?;
    }
    for file in files {
        let stat = fs::statat(dir_fd, &file.name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|_| changed_during_copy())?;
        if FileType::from_raw_mode(stat.st_mode).is_dir()
            || super::identity(&stat) != file.id
            || !fingerprint_matches(&stat, file.fingerprint)
        {
            return Err(changed_during_copy());
        }
        fs::unlinkat(dir_fd, &file.name, AtFlags::empty()).map_err(|error| {
            format!("Cannot remove original file after cross-volume copy: {error}. Remaining source items and the destination copy were retained.")
        })?;
    }
    Ok(())
}

fn fingerprint_matches(stat: &Stat, expected: Fingerprint) -> bool {
    let (size, mtime_sec, mtime_nsec) = stat_fingerprint(stat);
    size == expected.size && mtime_sec == expected.mtime_sec && mtime_nsec == expected.mtime_nsec
}

fn changed_during_copy() -> String {
    "Source folder contents changed during cross-volume copy; both the original source and destination copy were retained."
        .into()
}
