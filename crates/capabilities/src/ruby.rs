//! Ruby version (`.ruby-version`, `Gemfile.lock`) and Bundler (`Gemfile`) detection.

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
    let has_gemfile = dir.has_file("Gemfile") || dir.has_file("gems.rb");
    let has_lock = dir.has_file("Gemfile.lock") || dir.has_file("gems.locked");
    let dot_version = dir
        .read_first_line(".ruby-version")
        .and_then(|line| scan::clean_version(line.trim_start_matches("ruby-")));

    if !has_gemfile && !has_lock && dot_version.is_none() {
        return None;
    }

    let version = dot_version.or_else(|| read_lock_ruby_version(dir));
    let short_ver = version.as_deref().map(scan::short_major_minor);
    let has_bundler = has_gemfile || has_lock;

    let value = match (short_ver.as_deref(), has_bundler) {
        (Some(ver), true) => Some(format!("{ver} (bundler)")),
        (Some(ver), false) => Some(ver.to_owned()),
        (None, true) => Some("bundler".to_owned()),
        (None, false) => None,
    };

    let mut details = Vec::new();
    if let Some(ver) = &version {
        details.push(format!("v{ver}"));
    }
    if has_bundler {
        details.push("bundler".to_owned());
    }

    Some(CapabilityItem::new(
        CapabilityKind::Languages,
        "ruby",
        value,
        format!("Ruby: {}", details.join(" · ")),
    ))
}

fn read_lock_ruby_version(dir: &DirSnapshot) -> Option<String> {
    let lock = dir.read_bounded("Gemfile.lock")?;
    let mut lines = lock.lines().map(str::trim);
    while let Some(line) = lines.next() {
        if line == "RUBY VERSION"
            && let Some(next) = lines.next()
        {
            let cleaned = next.trim_start_matches("ruby").trim();
            return scan::clean_version(cleaned);
        }
    }
    None
}
