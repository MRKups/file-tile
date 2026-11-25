//! Zig build (`build.zig`, `build.zig.zon`) and version (`.zigversion`) detection.

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
    let has_build = dir.has_file("build.zig");
    let has_zon = dir.has_file("build.zig.zon");
    let dot_version = dir
        .read_first_line(".zigversion")
        .and_then(|line| scan::clean_version(&line));

    if !has_build && !has_zon && dot_version.is_none() {
        return None;
    }

    let version = dot_version.or_else(|| read_zon_version(dir));
    let short_ver = version.as_deref().map(scan::short_major_minor);

    let mut details = Vec::new();
    if let Some(ver) = &version {
        details.push(format!("v{ver}"));
    }
    if has_zon {
        details.push("build.zig.zon".to_owned());
    } else if has_build {
        details.push("build.zig".to_owned());
    }

    Some(CapabilityItem::new(
        CapabilityKind::Languages,
        "zig",
        short_ver,
        format!("Zig: {}", details.join(" · ")),
    ))
}

fn read_zon_version(dir: &DirSnapshot) -> Option<String> {
    let zon = dir.read_bounded("build.zig.zon")?;
    for line in zon.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(".minimum_zig_version")
            && let Some((_, val)) = rest.split_once('=')
            && let Some(ver) = scan::clean_version(val)
        {
            return Some(ver);
        }
    }
    None
}
