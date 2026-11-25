//! Dart and Flutter (`pubspec.yaml`, `.fvmrc`) detection.

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
    let pubspec = dir.read_bounded("pubspec.yaml");
    let fvm = dir.read_bounded(".fvmrc");

    if pubspec.is_none() && fvm.is_none() {
        return None;
    }

    let is_flutter = fvm.is_some()
        || pubspec
            .as_deref()
            .is_some_and(|text| text.lines().any(|line| line.trim() == "flutter:"));
    let sdk_version = pubspec.as_deref().and_then(read_sdk_version);
    let short_ver = sdk_version.as_deref().map(scan::short_major_minor);

    let prefix = if is_flutter { "flutter" } else { "dart" };
    let label = if is_flutter { "Flutter" } else { "Dart" };
    let detail = match &sdk_version {
        Some(ver) => format!("{label}: SDK {ver} (pubspec.yaml)"),
        None => format!("{label} (pubspec.yaml)"),
    };

    Some(CapabilityItem::new(
        CapabilityKind::Languages,
        prefix,
        short_ver,
        detail,
    ))
}

fn read_sdk_version(pubspec: &str) -> Option<String> {
    for line in pubspec.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("sdk:")
            && let Some(ver) = scan::clean_version(rest)
        {
            return Some(ver);
        }
    }
    None
}
