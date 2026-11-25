//! Environment override file (`.env*`, `.envrc`), auth dotfile, and private key
//! existence detection.
//!
//! For privacy and security, this module only inspects filenames from the
//! in-memory directory snapshot and never reads file contents.

use std::path::Path;

use crate::scan::DirSnapshot;
use crate::{CapabilityItem, CapabilityKind};

const AUTH_DOTFILES: [&str; 5] = [".npmrc", ".pypirc", ".netrc", ".pgpass", ".htpasswd"];
const SSH_KEY_FILES: [&str; 4] = ["id_rsa", "id_ed25519", "id_ecdsa", "id_dsa"];
const KEY_EXTENSIONS: [&str; 6] = ["pem", "key", "p12", "pfx", "keystore", "jks"];
const TEMPLATE_SUFFIXES: [&str; 4] = [".example", ".sample", ".template", ".dist"];

pub fn detect(snapshot: &DirSnapshot) -> Vec<CapabilityItem> {
    let mut env_files = Vec::new();
    let mut secret_files = Vec::new();

    for name in snapshot.files() {
        if is_env_file(name) {
            env_files.push(name.clone());
        } else if is_secret_or_key_file(name) {
            secret_files.push(name.clone());
        }
    }

    let mut items = Vec::new();
    if !env_files.is_empty() {
        let has_direnv = env_files.len() == 1 && env_files[0] == ".envrc";
        let value = if has_direnv {
            Some("direnv".to_owned())
        } else if env_files.len() > 1 {
            Some(format!("{}", env_files.len()))
        } else {
            None
        };
        items.push(CapabilityItem::new(
            CapabilityKind::EnvFiles,
            "env",
            value,
            format!("Environment files: {}", env_files.join(", ")),
        ));
    }
    if !secret_files.is_empty() {
        let value = if secret_files.len() == 1 {
            Some(secret_files[0].clone())
        } else {
            Some(format!("{}", secret_files.len()))
        };
        items.push(CapabilityItem::new(
            CapabilityKind::EnvFiles,
            "secrets",
            value,
            format!("Keys and credentials: {}", secret_files.join(", ")),
        ));
    }
    items
}

fn is_env_file(name: &str) -> bool {
    if name == ".env" || name == ".envrc" {
        return true;
    }
    if !name.starts_with(".env.") || name.len() <= 5 {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    !TEMPLATE_SUFFIXES
        .iter()
        .any(|suffix| lower.ends_with(suffix))
}

fn is_secret_or_key_file(name: &str) -> bool {
    if AUTH_DOTFILES.contains(&name) || SSH_KEY_FILES.contains(&name) {
        return true;
    }
    Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            KEY_EXTENSIONS
                .iter()
                .any(|candidate| ext.eq_ignore_ascii_case(candidate))
        })
}
