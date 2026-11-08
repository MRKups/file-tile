//! Swift Package Manager (`Package.swift`) and Xcode project detection.

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
    let has_spm = dir.has_file("Package.swift");
    let has_xcode = dir.has_dir_ext("xcworkspace") || dir.has_dir_ext("xcodeproj");
    let dot_version = dir
        .read_first_line(".swift-version")
        .and_then(|line| scan::clean_version(&line));

    if !has_spm && !has_xcode && dot_version.is_none() {
        return None;
    }

    let spm_version = has_spm.then(|| read_tools_version(dir)).flatten();
    let version = spm_version.or(dot_version);
    let short_ver = version.as_deref().map(scan::short_major_minor);

    let value = match (short_ver, has_xcode, has_spm) {
        (Some(ver), true, _) => Some(format!("{ver} (xcode)")),
        (Some(ver), false, _) => Some(ver),
        (None, true, _) => Some("xcode".to_owned()),
        (None, false, true) => Some("spm".to_owned()),
        (None, false, false) => None,
    };

    let mut details = Vec::new();
    if let Some(ver) = &version {
        details.push(format!("v{ver}"));
    }
    if has_spm {
        details.push("Package.swift".to_owned());
    }
    if has_xcode {
        details.push("Xcode".to_owned());
    }

    Some(CapabilityItem::new(
        CapabilityKind::Languages,
        "swift",
        value,
        format!("Swift: {}", details.join(" · ")),
    ))
}

fn read_tools_version(dir: &DirSnapshot) -> Option<String> {
    let pkg = dir.read_bounded("Package.swift")?;
    for line in pkg.lines().take(5) {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("// swift-tools-version:") {
            return scan::clean_version(rest);
        }
    }
    None
}
