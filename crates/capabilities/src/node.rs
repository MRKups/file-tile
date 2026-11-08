//! Node.js, fnm/nvm version files, Deno, and JS package manager detection.

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
    if dir.has_file("deno.json") || dir.has_file("deno.jsonc") {
        return Some(CapabilityItem::new(
            CapabilityKind::Languages,
            "deno",
            None,
            "Deno project (deno.json)",
        ));
    }

    let version = read_node_version(dir);
    let manager = detect_manager(dir);
    let has_package_json = dir.has_file("package.json");

    if version.is_none() && manager.is_none() && !has_package_json {
        return None;
    }

    let short_ver = version.as_deref().map(major_or_short);
    let value = match (short_ver.as_deref(), manager) {
        (Some(ver), Some(mgr)) => Some(format!("{ver} ({mgr})")),
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
    if dir.has_dir("node_modules") {
        details.push("node_modules".to_owned());
    }

    Some(CapabilityItem::new(
        CapabilityKind::Languages,
        "node",
        value,
        format!("Node.js: {}", details.join(" · ")),
    ))
}

fn read_node_version(dir: &DirSnapshot) -> Option<String> {
    for name in [".node-version", ".nvmrc", ".fnm-version"] {
        if let Some(line) = dir.read_first_line(name) {
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

fn detect_manager(dir: &DirSnapshot) -> Option<&'static str> {
    if dir.has_file("pnpm-lock.yaml") || dir.has_file("pnpm-workspace.yaml") {
        Some("pnpm")
    } else if dir.has_file("bun.lock") || dir.has_file("bun.lockb") {
        Some("bun")
    } else if dir.has_file("yarn.lock") {
        Some("yarn")
    } else if dir.has_file("package-lock.json") || dir.has_file("npm-shrinkwrap.json") {
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
