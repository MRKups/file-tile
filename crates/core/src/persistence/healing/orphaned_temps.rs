//! Scenario: `ConfigHealingScenario::OrphanedTemporaryFiles`.
//!
//! When the application or operating system terminates mid-save, temporary
//! write files (`.file-tile-<UUID>.tmp`) remain in `config/`. Once the
//! exclusive `.writer.lock` is held for an already-initialized configuration
//! directory, any leftover `.file-tile-*.tmp` regular files cannot belong to an
//! active writer and are safely removed.

use std::fs;
use std::path::Path;

use super::ConfigHealingScenario;

/// Sweeps orphaned `.file-tile-*.tmp` regular files if `config_dir` is an
/// initialized configuration root (`preferences.json` and `workspaces.json`
/// both exist as regular files).
pub(in crate::persistence) fn sweep_if_initialized(
    config_dir: &Path,
) -> Option<(ConfigHealingScenario, usize)> {
    let has_preferences =
        fs::symlink_metadata(config_dir.join("preferences.json")).is_ok_and(|meta| meta.is_file());
    let has_index =
        fs::symlink_metadata(config_dir.join("workspaces.json")).is_ok_and(|meta| meta.is_file());
    if !has_preferences || !has_index {
        return None;
    }

    let entries = fs::read_dir(config_dir).ok()?;
    let mut removed = 0usize;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if name_str.starts_with(".file-tile-")
            && name_str.ends_with(".tmp")
            && entry.file_type().is_ok_and(|ft| ft.is_file())
            && fs::remove_file(entry.path()).is_ok()
        {
            removed += 1;
        }
    }

    (removed > 0).then_some((ConfigHealingScenario::OrphanedTemporaryFiles, removed))
}
