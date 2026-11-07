//! Python virtual environment, version, and package manager detection.

use std::path::{Path, PathBuf};

use crate::{CapabilityItem, CapabilityKind, scan};

pub fn detect(ancestors: &[PathBuf]) -> Option<CapabilityItem> {
    for dir in ancestors {
        if let Some(item) = inspect_directory(dir) {
            return Some(item);
        }
    }
    None
}

fn inspect_directory(dir: &Path) -> Option<CapabilityItem> {
    let venv_version = read_pyvenv(dir, ".venv").or_else(|| read_pyvenv(dir, "venv"));
    let has_venv = venv_version.is_some()
        || scan::has_file(&dir.join(".venv"), "pyvenv.cfg")
        || scan::has_file(&dir.join("venv"), "pyvenv.cfg");
    let dot_version = scan::read_first_line(&dir.join(".python-version"))
        .and_then(|raw| scan::clean_version(&raw));
    let manager = detect_manager(dir);

    if !has_venv && dot_version.is_none() && manager.is_none() {
        return None;
    }

    let full_version = venv_version.or(dot_version);
    let short_version = full_version.as_deref().map(scan::short_major_minor);

    let value = match (short_version.as_deref(), manager) {
        (Some(ver), Some(mgr)) => Some(format!("{ver} · {mgr}")),
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
        CapabilityKind::Python,
        "py",
        value,
        detail,
    ))
}

fn read_pyvenv(dir: &Path, folder: &str) -> Option<String> {
    let cfg = scan::read_bounded(&dir.join(folder).join("pyvenv.cfg"))?;
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

fn detect_manager(dir: &Path) -> Option<&'static str> {
    if scan::has_file(dir, "uv.lock") {
        Some("uv")
    } else if scan::has_file(dir, "poetry.lock") {
        Some("poetry")
    } else if scan::has_file(dir, "Pipfile.lock") || scan::has_file(dir, "Pipfile") {
        Some("pipenv")
    } else if scan::has_file(dir, "environment.yml") || scan::has_file(dir, "conda-lock.yml") {
        Some("conda")
    } else if scan::has_file(dir, "pyproject.toml") {
        Some("pyproject")
    } else if scan::has_file(dir, "requirements.txt") {
        Some("pip")
    } else {
        None
    }
}
