//! Python virtual environment, version, and package manager detection.

use crate::scan::DirSnapshot;
use crate::{CapabilityItem, CapabilityKind, scan};

pub fn detect(ancestors: &[DirSnapshot]) -> Option<CapabilityItem> {
    for dir in ancestors {
        if let Some(item) = inspect_directory(dir) {
            return Some(item);
        }
    }
    None
}

fn inspect_directory(dir: &DirSnapshot) -> Option<CapabilityItem> {
    let venv_version = read_pyvenv(dir, ".venv").or_else(|| read_pyvenv(dir, "venv"));
    let has_venv = venv_version.is_some()
        || dir.has_subfile(".venv", "pyvenv.cfg")
        || dir.has_subfile("venv", "pyvenv.cfg");
    let dot_version = dir
        .read_first_line(".python-version")
        .and_then(|raw| scan::clean_version(&raw));
    let manager = detect_manager(dir);

    if !has_venv && dot_version.is_none() && manager.is_none() {
        return None;
    }

    let full_version = venv_version.or(dot_version);
    let short_version = full_version.as_deref().map(scan::short_major_minor);

    let value = match (short_version.as_deref(), manager) {
        (Some(ver), Some(mgr)) => Some(format!("{ver} ({mgr})")),
        (Some(ver), None) if has_venv => Some(format!("{ver} (.venv)")),
        (Some(ver), None) => Some(ver.to_owned()),
        (None, Some(mgr)) => Some(mgr.to_owned()),
        (None, None) => Some(".venv".to_owned()),
    };

    let mut details = Vec::new();
    if let Some(ver) = &full_version {
        details.push(format!("v{ver}"));
    }
    if has_venv {
        details.push("virtualenv".to_owned());
    }
    if let Some(mgr) = manager {
        details.push(mgr.to_owned());
    }
    let detail = format!("Python: {}", details.join(" · "));

    Some(CapabilityItem::new(
        CapabilityKind::Languages,
        "py",
        value,
        detail,
    ))
}

fn read_pyvenv(dir: &DirSnapshot, folder: &str) -> Option<String> {
    let cfg = dir.read_bounded(&format!("{folder}/pyvenv.cfg"))?;
    for line in cfg.lines() {
        let Some((key, val)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if matches!(key, "version" | "version_info")
            && let Some(cleaned) = scan::clean_version(val)
        {
            return Some(cleaned);
        }
    }
    None
}

fn detect_manager(dir: &DirSnapshot) -> Option<&'static str> {
    if dir.has_file("uv.lock") {
        Some("uv")
    } else if dir.has_file("poetry.lock") {
        Some("poetry")
    } else if dir.has_file("Pipfile.lock") || dir.has_file("Pipfile") {
        Some("pipenv")
    } else if dir.has_file("environment.yml") || dir.has_file("conda-lock.yml") {
        Some("conda")
    } else if dir.has_file("pyproject.toml") {
        Some("pyproject")
    } else if dir.has_file("requirements.txt") {
        Some("pip")
    } else {
        None
    }
}
