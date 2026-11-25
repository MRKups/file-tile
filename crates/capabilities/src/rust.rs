//! Rust / Cargo package, workspace, edition, and toolchain detection.

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
    let cargo_toml = dir.read_bounded("Cargo.toml");
    let toolchain = read_toolchain(dir);

    if cargo_toml.is_none() && toolchain.is_none() {
        return None;
    }

    let edition = cargo_toml.as_deref().and_then(parse_edition);
    let is_workspace = cargo_toml
        .as_deref()
        .is_some_and(|text| text.lines().any(|line| line.trim() == "[workspace]"));

    let value = match (edition.as_deref(), toolchain.as_deref()) {
        (Some(ed), Some(tc)) => Some(format!("{ed} ({tc})")),
        (Some(ed), None) => Some(ed.to_owned()),
        (None, Some(tc)) => Some(tc.to_owned()),
        (None, None) if is_workspace => Some("workspace".to_owned()),
        (None, None) => Some("cargo".to_owned()),
    };

    let mut details = Vec::new();
    if is_workspace {
        details.push("Cargo workspace".to_owned());
    } else if cargo_toml.is_some() {
        details.push("Cargo crate".to_owned());
    }
    if let Some(ed) = edition {
        details.push(format!("edition {ed}"));
    }
    if let Some(tc) = toolchain {
        details.push(format!("toolchain {tc}"));
    }

    Some(CapabilityItem::new(
        CapabilityKind::Languages,
        "rust",
        value,
        format!("Rust: {}", details.join(" · ")),
    ))
}

fn parse_edition(cargo_toml: &str) -> Option<String> {
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        let Some((key, val)) = trimmed.split_once('=') else {
            continue;
        };
        if key.trim() == "edition"
            && let Some(cleaned) = scan::clean_version(val)
        {
            return Some(cleaned);
        }
    }
    None
}

fn read_toolchain(dir: &DirSnapshot) -> Option<String> {
    if let Some(text) = dir.read_bounded("rust-toolchain.toml") {
        for line in text.lines() {
            let Some((key, val)) = line.trim().split_once('=') else {
                continue;
            };
            if key.trim() == "channel" {
                let channel = val.trim().trim_matches(|c| matches!(c, '"' | '\''));
                if !channel.is_empty() && channel.len() <= 20 {
                    return Some(channel.to_owned());
                }
            }
        }
    }
    dir.read_first_line("rust-toolchain").and_then(|line| {
        let cleaned = line.trim();
        (!cleaned.is_empty() && cleaned.len() <= 20).then(|| cleaned.to_owned())
    })
}
