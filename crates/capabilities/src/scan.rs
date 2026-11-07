//! Bounded ancestor traversal and small file inspection helpers.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_ANCESTOR_DEPTH: usize = 16;
const MAX_FILE_SIZE: u64 = 256 * 1024;
const MAX_READ_BYTES: usize = 16 * 1024;

/// Collect `directory` and its parent folders up to a safe boundary.
/// Stops at `/`, `/Users`, `/home`, or `/Volumes` so scans stay inside user trees.
pub fn ancestors(directory: &Path) -> Vec<PathBuf> {
    let mut chain = Vec::new();
    for ancestor in directory.ancestors().take(MAX_ANCESTOR_DEPTH) {
        if is_boundary(ancestor) {
            break;
        }
        chain.push(ancestor.to_path_buf());
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

pub fn has_file(directory: &Path, name: &str) -> bool {
    fs::metadata(directory.join(name)).is_ok_and(|meta| meta.is_file())
}

pub fn has_dir(directory: &Path, name: &str) -> bool {
    fs::metadata(directory.join(name)).is_ok_and(|meta| meta.is_dir())
}

pub fn read_bounded(path: &Path) -> Option<String> {
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

pub fn read_first_line(path: &Path) -> Option<String> {
    let content = read_bounded(path)?;
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
}

pub fn clean_version(raw: &str) -> Option<String> {
    let trimmed = raw
        .trim()
        .trim_matches(|c: char| matches!(c, '"' | '\'' | '^' | '~' | '=' | '>' | '<' | 'v' | 'V'));
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

pub fn has_extension(directory: &Path, ext: &str) -> bool {
    let Ok(entries) = fs::read_dir(directory) else {
        return false;
    };
    entries.take(128).flatten().any(|entry| {
        entry
            .path()
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case(ext))
    })
}
