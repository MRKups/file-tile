//! Elixir (`mix.exs`, `.elixir-version`) and Mix project detection.

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
    let has_mix = dir.has_file("mix.exs");
    let dot_version = dir
        .read_first_line(".elixir-version")
        .and_then(|line| scan::clean_version(&line));

    if !has_mix && dot_version.is_none() {
        return None;
    }

    let version = dot_version.or_else(|| read_mix_elixir_version(dir));
    let short_ver = version.as_deref().map(scan::short_major_minor);

    let value = match (short_ver, has_mix) {
        (Some(ver), true) => Some(format!("{ver} (mix)")),
        (Some(ver), false) => Some(ver),
        (None, true) => Some("mix".to_owned()),
        (None, false) => None,
    };

    let mut details = Vec::new();
    if let Some(ver) = &version {
        details.push(format!("v{ver}"));
    }
    if has_mix {
        details.push("mix.exs".to_owned());
    }

    Some(CapabilityItem::new(
        CapabilityKind::Languages,
        "elixir",
        value,
        format!("Elixir: {}", details.join(" · ")),
    ))
}

fn read_mix_elixir_version(dir: &DirSnapshot) -> Option<String> {
    let mix = dir.read_bounded("mix.exs")?;
    for line in mix.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("elixir:")
            && let Some(ver) = scan::clean_version(rest)
        {
            return Some(ver);
        }
    }
    None
}
