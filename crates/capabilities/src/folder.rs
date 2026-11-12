//! Symlinked folder target, read-only permission, cloud storage, and drive mount detection.

use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::CapabilityItem;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum MountOrigin {
    #[default]
    Local,
    External {
        name: Option<String>,
    },
    Network {
        fstype: String,
        name: Option<String>,
    },
}

pub fn detect(directory: &Path, resolved: &Path) -> Vec<CapabilityItem> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    detect_with_context(directory, resolved, home.as_deref(), &MountOrigin::Local)
}

pub fn detect_with_context(
    directory: &Path,
    resolved: &Path,
    home: Option<&Path>,
    mount: &MountOrigin,
) -> Vec<CapabilityItem> {
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
    if let Some((slug, detail)) = detect_cloud(directory, resolved, home) {
        items.push(CapabilityItem::always(
            "cloud",
            Some(slug.to_owned()),
            detail,
        ));
    }
    if let Some(drive_item) = detect_mount(mount) {
        items.push(drive_item);
    }
    items
}

fn detect_cloud(
    directory: &Path,
    resolved: &Path,
    home: Option<&Path>,
) -> Option<(&'static str, String)> {
    let home = home.filter(|path| has_usable_home(path))?;
    let from_dir = detect_cloud_path(directory, home);
    if from_dir.as_ref().is_some_and(|(slug, _)| *slug != "cloud") || directory == resolved {
        return from_dir;
    }
    detect_cloud_path(resolved, home).or(from_dir)
}

fn has_usable_home(home: &Path) -> bool {
    home.is_absolute() && !home.as_os_str().is_empty() && home != Path::new("/")
}

fn strip_home<'a>(path: &'a Path, home: &Path) -> Option<&'a Path> {
    if let Ok(rel) = path.strip_prefix(home) {
        return Some(rel);
    }
    if let Ok(without_private) = path.strip_prefix("/private")
        && let Ok(home_rel) = home.strip_prefix("/")
        && let Ok(rel) = without_private.strip_prefix(home_rel)
    {
        return Some(rel);
    }
    if let Ok(home_without_private) = home.strip_prefix("/private")
        && let Ok(path_rel) = path.strip_prefix("/")
        && let Ok(rel) = path_rel.strip_prefix(home_without_private)
    {
        return Some(rel);
    }
    None
}

fn detect_cloud_path(path: &Path, home: &Path) -> Option<(&'static str, String)> {
    let rel = strip_home(path, home)?;
    if rel.starts_with("Library/Mobile Documents/com~apple~CloudDocs") {
        return Some(("icloud", "Cloud storage: iCloud Drive".to_owned()));
    }
    if let Ok(after_cs) = rel.strip_prefix("Library/CloudStorage") {
        let Some(Component::Normal(raw_domain)) = after_cs.components().next() else {
            return None;
        };
        let domain = raw_domain
            .to_str()
            .filter(|name| !name.is_empty() && !name.starts_with('.'))?;
        return Some(classify_cloud_domain(domain));
    }
    let Some(Component::Normal(raw_child)) = rel.components().next() else {
        return None;
    };
    let child = raw_child.to_str()?;
    classify_home_cloud_child(child).map(|(slug, detail)| (slug, detail.to_owned()))
}

fn classify_cloud_domain(domain: &str) -> (&'static str, String) {
    let lower = domain.to_ascii_lowercase();
    let known = if lower.starts_with("onedrive") || has_word_prefix(&lower, "one drive") {
        Some(("onedrive", "Cloud storage: OneDrive"))
    } else if lower.starts_with("dropbox") {
        Some(("dropbox", "Cloud storage: Dropbox"))
    } else if lower.starts_with("protondrive") || lower.starts_with("proton drive") {
        Some(("proton", "Cloud storage: Proton Drive"))
    } else if lower.starts_with("box") {
        Some(("box", "Cloud storage: Box"))
    } else if lower.starts_with("googledrive") || lower.starts_with("google drive") {
        Some(("gdrive", "Cloud storage: Google Drive"))
    } else if lower.starts_with("nextcloud") {
        Some(("nextcloud", "Cloud storage: Nextcloud"))
    } else if lower.starts_with("pcloud") {
        Some(("pcloud", "Cloud storage: pCloud"))
    } else if lower.starts_with("owncloud") {
        Some(("owncloud", "Cloud storage: OwnCloud"))
    } else if lower.starts_with("synologydrive") || lower.starts_with("synology drive") {
        Some(("synology", "Cloud storage: Synology Drive"))
    } else if lower.starts_with("mega") {
        Some(("mega", "Cloud storage: MEGA"))
    } else {
        None
    };
    match known {
        Some((slug, detail)) => (slug, detail.to_owned()),
        None => ("cloud", format!("Cloud storage: {domain}")),
    }
}

fn classify_home_cloud_child(name: &str) -> Option<(&'static str, &'static str)> {
    let lower = name.to_ascii_lowercase();
    if has_word_prefix(&lower, "onedrive") || has_word_prefix(&lower, "one drive") {
        Some(("onedrive", "Cloud storage: OneDrive"))
    } else if has_word_prefix(&lower, "dropbox") {
        Some(("dropbox", "Cloud storage: Dropbox"))
    } else if has_word_prefix(&lower, "protondrive") || has_word_prefix(&lower, "proton drive") {
        Some(("proton", "Cloud storage: Proton Drive"))
    } else if has_word_prefix(&lower, "box") {
        Some(("box", "Cloud storage: Box"))
    } else if has_word_prefix(&lower, "googledrive") || has_word_prefix(&lower, "google drive") {
        Some(("gdrive", "Cloud storage: Google Drive"))
    } else if has_word_prefix(&lower, "nextcloud") {
        Some(("nextcloud", "Cloud storage: Nextcloud"))
    } else if has_word_prefix(&lower, "pcloud") {
        Some(("pcloud", "Cloud storage: pCloud"))
    } else if has_word_prefix(&lower, "owncloud") {
        Some(("owncloud", "Cloud storage: OwnCloud"))
    } else if has_word_prefix(&lower, "synologydrive") || has_word_prefix(&lower, "synology drive")
    {
        Some(("synology", "Cloud storage: Synology Drive"))
    } else if has_word_prefix(&lower, "mega")
        || has_word_prefix(&lower, "megasync")
        || has_word_prefix(&lower, "megadrive")
    {
        Some(("mega", "Cloud storage: MEGA"))
    } else {
        None
    }
}

fn has_word_prefix(text: &str, prefix: &str) -> bool {
    let Some(rest) = text.strip_prefix(prefix) else {
        return false;
    };
    rest.is_empty() || rest.starts_with([' ', '-', '_', '('])
}

fn detect_mount(mount: &MountOrigin) -> Option<CapabilityItem> {
    match mount {
        MountOrigin::Local => None,
        MountOrigin::External { name } => {
            let detail = match name.as_deref().filter(|n| !n.is_empty()) {
                Some(n) => format!("External drive: {n}"),
                None => "External drive".to_owned(),
            };
            Some(CapabilityItem::always(
                "drive",
                Some("external".to_owned()),
                detail,
            ))
        }
        MountOrigin::Network { fstype, name } => {
            let fs_label = if fstype.is_empty() {
                "network"
            } else {
                fstype.as_str()
            };
            let detail = match name.as_deref().filter(|n| !n.is_empty()) {
                Some(n) => format!("Network share: {n} ({fs_label})"),
                None => format!("Network share ({fs_label})"),
            };
            Some(CapabilityItem::always(
                "drive",
                Some(fs_label.to_owned()),
                detail,
            ))
        }
    }
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
