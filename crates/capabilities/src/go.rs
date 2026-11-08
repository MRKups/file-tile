//! Go module (`go.mod`) and workspace (`go.work`) detection.

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
    let go_work = dir.read_bounded("go.work");
    let go_mod = dir.read_bounded("go.mod");

    if go_work.is_none() && go_mod.is_none() {
        return None;
    }

    let version = go_mod
        .as_deref()
        .and_then(parse_go_version)
        .or_else(|| go_work.as_deref().and_then(parse_go_version));
    let short_ver = version.as_deref().map(scan::short_major_minor);
    let is_work = go_work.is_some();

    let value = match (short_ver, is_work) {
        (Some(ver), true) => Some(format!("{ver} (work)")),
        (Some(ver), false) => Some(ver),
        (None, true) => Some("work".to_owned()),
        (None, false) => Some("mod".to_owned()),
    };

    let detail = match (version, is_work) {
        (Some(ver), true) => format!("Go: v{ver} (go.work)"),
        (Some(ver), false) => format!("Go: v{ver} (go.mod)"),
        (None, true) => "Go: workspace (go.work)".to_owned(),
        (None, false) => "Go: module (go.mod)".to_owned(),
    };

    Some(CapabilityItem::new(
        CapabilityKind::Languages,
        "go",
        value,
        detail,
    ))
}

fn parse_go_version(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("go ")
            && let Some(ver) = scan::clean_version(rest)
        {
            return Some(ver);
        }
    }
    None
}
