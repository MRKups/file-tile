//! Environment override file (`.env`, `.env.local`, `.envrc`) existence detection.
//!
//! For privacy and security, this module only checks whether environment files exist
//! via metadata and never reads their contents.

use std::path::Path;

use crate::{CapabilityItem, CapabilityKind, scan};

const ENV_FILES: [&str; 6] = [
    ".env",
    ".env.local",
    ".env.development",
    ".env.production",
    ".env.test",
    ".envrc",
];

pub fn detect(directory: &Path) -> Option<CapabilityItem> {
    let found: Vec<&str> = ENV_FILES
        .into_iter()
        .filter(|name| scan::has_file(directory, name))
        .collect();
    if found.is_empty() {
        return None;
    }
    let has_direnv = found.contains(&".envrc");
    let value = if has_direnv && found.len() == 1 {
        Some("direnv".to_owned())
    } else if found.len() > 1 {
        Some(format!("{}", found.len()))
    } else {
        None
    };
    Some(CapabilityItem::new(
        CapabilityKind::EnvFiles,
        "env",
        value,
        format!("Environment files: {}", found.join(", ")),
    ))
}
