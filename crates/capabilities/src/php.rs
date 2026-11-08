//! PHP version (`.php-version`, `composer.json`) and Composer detection.

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
    let has_composer = dir.has_file("composer.json") || dir.has_file("composer.lock");
    let dot_version = dir
        .read_first_line(".php-version")
        .and_then(|line| scan::clean_version(&line));

    if !has_composer && dot_version.is_none() {
        return None;
    }

    let version = dot_version.or_else(|| read_composer_php(dir));
    let short_ver = version.as_deref().map(scan::short_major_minor);

    let value = match (short_ver.as_deref(), has_composer) {
        (Some(ver), true) => Some(format!("{ver} (composer)")),
        (Some(ver), false) => Some(ver.to_owned()),
        (None, true) => Some("composer".to_owned()),
        (None, false) => None,
    };

    let mut details = Vec::new();
    if let Some(ver) = &version {
        details.push(format!("v{ver}"));
    }
    if has_composer {
        details.push("composer".to_owned());
    }

    Some(CapabilityItem::new(
        CapabilityKind::Languages,
        "php",
        value,
        format!("PHP: {}", details.join(" · ")),
    ))
}

fn read_composer_php(dir: &DirSnapshot) -> Option<String> {
    let json = dir.read_bounded("composer.json")?;
    for line in json.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("\"php\"")
            && let Some((_, val)) = rest.split_once(':')
            && let Some(ver) = scan::clean_version(val)
        {
            return Some(ver);
        }
    }
    None
}
