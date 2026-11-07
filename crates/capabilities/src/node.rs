//! Node.js, fnm/nvm version files, Deno, and JS package manager detection.

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
    if scan::has_file(dir, "deno.json") || scan::has_file(dir, "deno.jsonc") {
        return Some(CapabilityItem::new(
            CapabilityKind::Node,
            "deno",
            None,
            "Deno project (deno.json)",
        ));
    }

    let version = read_node_version(dir);
    let manager = detect_manager(dir);
    let has_package_json = scan::has_file(dir, "package.json");

    if version.is_none() && manager.is_none() && !has_package_json {
        return None;
    }

    let short_ver = version.as_deref().map(major_or_short);
    let value = match (short_ver.as_deref(), manager) {
        (Some(ver), Some(mgr)) => Some(format!("{ver} · {mgr}")),
        (Some(ver), None) => Some(ver.to_owned()),
        (None, Some(mgr)) => Some(mgr.to_owned()),
        (None, None) => Some("pkg".to_owned()),
    };

    let mut details = Vec::new();
    if let Some(ver) = &version {
        details.push(format!("v{ver}"));
    }
    if let Some(mgr) = manager {
        details.push(mgr.to_owned());
    } else if has_package_json {
        details.push("package.json".to_owned());
    }
    if scan::has_dir(dir, "node_modules") {
        details.push("node_modules".to_owned());
    }

    Some(CapabilityItem::new(
        CapabilityKind::Node,
        "node",
        value,
        format!("Node.js: {}", details.join(" · ")),
    ))
}

fn read_node_version(dir: &Path) -> Option<String> {
    for name in [".node-version", ".nvmrc", ".fnm-version"] {
        if let Some(line) = scan::read_first_line(&dir.join(name)) {
            if let Some(cleaned) = scan::clean_version(&line) {
                return Some(cleaned);
            }
            let trimmed = line.trim().trim_start_matches("lts/");
            if !trimmed.is_empty() && trimmed.len() <= 16 {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

fn detect_manager(dir: &Path) -> Option<&'static str> {
    if scan::has_file(dir, "pnpm-lock.yaml") || scan::has_file(dir, "pnpm-workspace.yaml") {
        Some("pnpm")
    } else if scan::has_file(dir, "bun.lock") || scan::has_file(dir, "bun.lockb") {
        Some("bun")
    } else if scan::has_file(dir, "yarn.lock") {
        Some("yarn")
    } else if scan::has_file(dir, "package-lock.json") || scan::has_file(dir, "npm-shrinkwrap.json")
    {
        Some("npm")
    } else {
        None
    }
}

fn major_or_short(version: &str) -> String {
    let mut parts = version.split('.');
    let Some(first) = parts.next() else {
        return version.to_owned();
    };
    if first.chars().all(|c| c.is_ascii_digit()) && parts.next().is_some() {
        first.to_owned()
    } else {
        version.to_owned()
    }
}
