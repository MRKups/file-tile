//! Symlinked folder target and read-only folder permission detection.

use std::fs;
use std::path::Path;

use crate::CapabilityItem;

pub fn detect(directory: &Path, resolved: &Path) -> Vec<CapabilityItem> {
    let mut items = Vec::new();
    if directory != resolved && !is_system_private_link(directory, resolved) {
        let target_name = resolved
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .map(str::to_owned);
        items.push(CapabilityItem::always(
            "link",
            target_name,
            format!("Symlink target: {}", resolved.display()),
        ));
    }
    if is_read_only(resolved) {
        items.push(CapabilityItem::always(
            "read-only",
            None,
            "Read-only folder",
        ));
    }
    items
}

fn is_system_private_link(directory: &Path, resolved: &Path) -> bool {
    resolved
        .strip_prefix("/private")
        .is_ok_and(|tail| Path::new("/").join(tail) == directory)
}

fn is_read_only(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if meta.permissions().readonly() {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let mode = meta.mode();
        if mode & 0o200 == 0 {
            return true;
        }
        if meta.uid() == 0
            && (mode & 0o002 == 0)
            && !std::env::var_os("USER").is_some_and(|user| user == "root")
        {
            return true;
        }
    }
    false
}
