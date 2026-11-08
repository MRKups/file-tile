//! Bounded ancestor traversal, in-memory directory snapshots, and small file
//! inspection helpers.
//!
//! `DirSnapshot` reads each ancestor directory's entry names once so all
//! capability modules can probe for marker files, directories, and extensions
//! in memory without repeated `stat` or `open` syscalls.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_ANCESTOR_DEPTH: usize = 16;
const MAX_DIR_ENTRIES: usize = 512;
const MAX_FILE_SIZE: u64 = 256 * 1024;
const MAX_READ_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone)]
pub struct DirSnapshot {
    path: PathBuf,
    files: Vec<String>,
    dirs: Vec<String>,
}

impl DirSnapshot {
    pub fn capture(path: PathBuf) -> Self {
        let mut files = Vec::new();
        let mut dirs = Vec::new();
        if let Ok(entries) = fs::read_dir(&path) {
            for entry in entries.take(MAX_DIR_ENTRIES).flatten() {
                let raw_name = entry.file_name();
                let Some(name) = raw_name.to_str() else {
                    continue;
                };
                let Ok(ft) = entry.file_type() else {
                    continue;
                };
                if ft.is_file() {
                    files.push(name.to_owned());
                } else if ft.is_dir() {
                    dirs.push(name.to_owned());
                } else if ft.is_symlink() {
                    if let Ok(meta) = fs::metadata(entry.path()) {
                        if meta.is_dir() {
                            dirs.push(name.to_owned());
                        } else {
                            files.push(name.to_owned());
                        }
                    } else {
                        files.push(name.to_owned());
                    }
                }
            }
        }
        files.sort_unstable();
        dirs.sort_unstable();
        Self { path, files, dirs }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn files(&self) -> &[String] {
        &self.files
    }

    pub fn has_file(&self, name: &str) -> bool {
        self.files
            .binary_search_by(|probe| probe.as_str().cmp(name))
            .is_ok()
    }

    pub fn has_dir(&self, name: &str) -> bool {
        self.dirs
            .binary_search_by(|probe| probe.as_str().cmp(name))
            .is_ok()
    }

    pub fn has_subfile(&self, subdir: &str, name: &str) -> bool {
        self.has_dir(subdir) && has_file_on_disk(&self.path.join(subdir), name)
    }

    pub fn first_file_with_ext(&self, ext: &str) -> Option<&str> {
        self.files
            .iter()
            .map(String::as_str)
            .find(|name| has_ext(name, ext))
    }

    pub fn has_file_ext(&self, ext: &str) -> bool {
        self.first_file_with_ext(ext).is_some()
    }

    pub fn has_dir_ext(&self, ext: &str) -> bool {
        self.dirs.iter().any(|name| has_ext(name, ext))
    }

    pub fn read_bounded(&self, rel: &str) -> Option<String> {
        if let Some((top, _)) = rel.split_once('/') {
            if !self.has_dir(top) {
                return None;
            }
        } else if !self.has_file(rel) {
            return None;
        }
        read_bounded_path(&self.path.join(rel))
    }

    pub fn read_first_line(&self, rel: &str) -> Option<String> {
        let content = self.read_bounded(rel)?;
        first_non_comment_line(&content)
    }
}

fn has_ext(name: &str, expected: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(expected))
}

/// Collect `directory` and its parent folders up to a safe boundary as in-memory snapshots.
pub fn ancestors(directory: &Path) -> Vec<DirSnapshot> {
    let mut chain = Vec::new();
    for ancestor in directory.ancestors().take(MAX_ANCESTOR_DEPTH) {
        if is_boundary(ancestor) {
            break;
        }
        chain.push(DirSnapshot::capture(ancestor.to_path_buf()));
    }
    chain
}

fn is_boundary(path: &Path) -> bool {
    let Some(raw) = path.to_str() else {
        return false;
    };
    matches!(
        raw,
        "/" | "/Users" | "/home" | "/Volumes" | "/System" | "/Library" | "/private" | "/var"
    )
}

pub fn has_file_on_disk(directory: &Path, name: &str) -> bool {
    fs::metadata(directory.join(name)).is_ok_and(|meta| meta.is_file())
}

pub fn has_dir_on_disk(directory: &Path, name: &str) -> bool {
    fs::metadata(directory.join(name)).is_ok_and(|meta| meta.is_dir())
}

pub fn read_bounded_path(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_SIZE {
        return None;
    }
    let file = fs::File::open(path).ok()?;
    let mut buffer = Vec::new();
    file.take(MAX_READ_BYTES as u64)
        .read_to_end(&mut buffer)
        .ok()?;
    String::from_utf8(buffer).ok()
}

pub fn read_first_line_path(path: &Path) -> Option<String> {
    let content = read_bounded_path(path)?;
    first_non_comment_line(&content)
}

fn first_non_comment_line(content: &str) -> Option<String> {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
}

pub fn clean_version(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches(|c: char| {
        matches!(
            c,
            '"' | '\'' | '^' | '~' | '=' | '>' | '<' | 'v' | 'V' | ',' | ';' | ' '
        )
    });
    let token: String = trimmed
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .take(20)
        .collect();
    (!token.is_empty() && token.chars().any(|c| c.is_ascii_digit())).then_some(token)
}

pub fn short_major_minor(version: &str) -> String {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() >= 2 && parts[0].chars().all(|c| c.is_ascii_digit()) {
        format!("{}.{}", parts[0], parts[1])
    } else {
        version.to_owned()
    }
}
