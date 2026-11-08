//! Multi-runtime version managers (`mise`, `asdf`) and Nix shells (`flake.nix`, `shell.nix`).

use crate::scan::DirSnapshot;
use crate::{CapabilityItem, CapabilityKind};

pub fn detect(ancestors: &[DirSnapshot]) -> Option<CapabilityItem> {
    for dir in ancestors {
        if let Some(item) = inspect_directory(dir) {
            return Some(item);
        }
    }
    None
}

fn inspect_directory(dir: &DirSnapshot) -> Option<CapabilityItem> {
    if dir.has_file("flake.nix") {
        return Some(CapabilityItem::new(
            CapabilityKind::Languages,
            "nix",
            Some("flake".to_owned()),
            "Nix: flake.nix",
        ));
    }
    if dir.has_file("shell.nix") || dir.has_file("default.nix") {
        return Some(CapabilityItem::new(
            CapabilityKind::Languages,
            "nix",
            Some("shell".to_owned()),
            "Nix: shell.nix",
        ));
    }
    if dir.has_file("mise.toml") || dir.has_file(".mise.toml") {
        return Some(CapabilityItem::new(
            CapabilityKind::Languages,
            "mise",
            None,
            "mise: pinned toolchains (mise.toml)",
        ));
    }
    if let Some(content) = dir.read_bounded(".tool-versions") {
        let tools: Vec<&str> = content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .filter_map(|line| line.split_whitespace().next())
            .take(6)
            .collect();
        let value = (!tools.is_empty()).then(|| format!("{}", tools.len()));
        let detail = if tools.is_empty() {
            "Tool versions (.tool-versions)".to_owned()
        } else {
            format!("Tool versions (.tool-versions): {}", tools.join(", "))
        };
        return Some(CapabilityItem::new(
            CapabilityKind::Languages,
            "asdf",
            value,
            detail,
        ));
    }
    None
}
