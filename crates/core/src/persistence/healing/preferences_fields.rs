//! Scenarios:
//! - `ConfigHealingScenario::MalformedCapabilitiesPreferences`
//! - `ConfigHealingScenario::MalformedSafetyPreferences`
//! - `ConfigHealingScenario::MalformedPinnedFolders`
//!
//! Heals missing, outdated, or malformed fields in the `capabilities`, `safety`,
//! and `pinned_folders` sections of `preferences.json` to safe defaults without
//! locking workspace autosave into read-only mode.

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;

use serde_json::Value;

use crate::application::contracts::configuration::MAX_PINNED_FOLDERS;
use crate::domain::safety::MAX_DISABLED_LOCATIONS;
use crate::domain::{CapabilityPreferences, SafetyPreferences};

use super::ConfigHealingScenario;

/// Restores `CapabilityPreferences` from JSON, healing missing or unrecognized
/// fields to [`CapabilityPreferences::default`].
pub(in crate::persistence) fn restore_capabilities(
    value: Option<Value>,
) -> (CapabilityPreferences, Option<ConfigHealingScenario>) {
    let defaults = CapabilityPreferences::default();
    let Some(value) = value else {
        return (
            defaults,
            Some(ConfigHealingScenario::MalformedCapabilitiesPreferences),
        );
    };
    let Value::Object(map) = value else {
        return (
            defaults,
            Some(ConfigHealingScenario::MalformedCapabilitiesPreferences),
        );
    };

    let mut healed = map.len() != 3;
    let bool_field = |key: &str, fallback: bool, healed: &mut bool| -> bool {
        match map.get(key).and_then(Value::as_bool) {
            Some(val) => val,
            None => {
                *healed = true;
                fallback
            }
        }
    };

    let preferences = CapabilityPreferences {
        git: bool_field("git", defaults.git, &mut healed),
        languages: bool_field("languages", defaults.languages, &mut healed),
        env_files: bool_field("env_files", defaults.env_files, &mut healed),
    };
    let scenario = healed.then_some(ConfigHealingScenario::MalformedCapabilitiesPreferences);
    (preferences, scenario)
}

const KNOWN_SAFETY_KEYS: [&str; 10] = [
    "desktop_access",
    "documents_access",
    "downloads_access",
    "full_disk_access",
    "external_drives_access",
    "network_shares_access",
    "disabled_locations",
    "system_folders",
    "applications",
    "credentials",
];

/// Restores `SafetyPreferences` from JSON, healing missing or unrecognized
/// fields to [`SafetyPreferences::default`].
pub(in crate::persistence) fn restore_safety(
    value: Value,
) -> (SafetyPreferences, Option<ConfigHealingScenario>) {
    let defaults = SafetyPreferences::default();
    let Value::Object(map) = value else {
        return (
            defaults,
            Some(ConfigHealingScenario::MalformedSafetyPreferences),
        );
    };

    let mut healed = map
        .keys()
        .any(|key| !KNOWN_SAFETY_KEYS.contains(&key.as_str()));
    let bool_field = |key: &str, fallback: bool, healed: &mut bool| -> bool {
        match map.get(key).and_then(Value::as_bool) {
            Some(val) => val,
            None => {
                *healed = true;
                fallback
            }
        }
    };
    let optional_bool = |key: &str, fallback: bool, healed: &mut bool| -> bool {
        match map.get(key) {
            None => fallback,
            Some(Value::Bool(val)) => *val,
            Some(_) => {
                *healed = true;
                fallback
            }
        }
    };

    let preferences = SafetyPreferences {
        desktop_access: bool_field("desktop_access", defaults.desktop_access, &mut healed),
        documents_access: bool_field("documents_access", defaults.documents_access, &mut healed),
        downloads_access: bool_field("downloads_access", defaults.downloads_access, &mut healed),
        full_disk_access: bool_field("full_disk_access", defaults.full_disk_access, &mut healed),
        external_drives_access: optional_bool(
            "external_drives_access",
            defaults.external_drives_access,
            &mut healed,
        ),
        network_shares_access: optional_bool(
            "network_shares_access",
            defaults.network_shares_access,
            &mut healed,
        ),
        disabled_locations: restore_disabled_locations(map.get("disabled_locations"), &mut healed),
        system_folders: bool_field("system_folders", defaults.system_folders, &mut healed),
        applications: bool_field("applications", defaults.applications, &mut healed),
        credentials: bool_field("credentials", defaults.credentials, &mut healed),
    };
    let scenario = healed.then_some(ConfigHealingScenario::MalformedSafetyPreferences);
    (preferences, scenario)
}

fn restore_disabled_locations(value: Option<&Value>, healed: &mut bool) -> BTreeSet<PathBuf> {
    let Some(value) = value else {
        return BTreeSet::new();
    };
    let Value::Array(items) = value else {
        *healed = true;
        return BTreeSet::new();
    };
    if items.len() > MAX_DISABLED_LOCATIONS {
        *healed = true;
    }
    let mut valid = BTreeSet::new();
    for item in items.iter().take(MAX_DISABLED_LOCATIONS) {
        let parsed = serde_json::from_value::<super::super::path::PathRecord>(item.clone())
            .ok()
            .and_then(|record| record.into_path().ok());
        match parsed {
            Some(path) => {
                if !valid.insert(path) {
                    *healed = true;
                }
            }
            None => *healed = true,
        }
    }
    valid
}

/// Restores `pinned_folders` from JSON, pruning invalid or duplicate entries
/// and resetting wholly invalid values to `None` so defaults are seeded.
pub(in crate::persistence) fn restore_pinned_folders(
    value: Option<Value>,
) -> (Option<Vec<PathBuf>>, Option<ConfigHealingScenario>) {
    match value {
        None | Some(Value::Null) => (None, None),
        Some(Value::Array(items)) => {
            let mut healed = items.len() > MAX_PINNED_FOLDERS;
            let mut seen = HashSet::new();
            let mut valid = Vec::new();
            for item in items.into_iter().take(MAX_PINNED_FOLDERS) {
                let parsed = serde_json::from_value::<super::super::path::PathRecord>(item)
                    .ok()
                    .and_then(|record| record.into_path().ok());
                match parsed {
                    Some(path) if seen.insert(path.clone()) => valid.push(path),
                    _ => healed = true,
                }
            }
            if valid.is_empty() && healed {
                (None, Some(ConfigHealingScenario::MalformedPinnedFolders))
            } else {
                (
                    Some(valid),
                    healed.then_some(ConfigHealingScenario::MalformedPinnedFolders),
                )
            }
        }
        Some(_) => (None, Some(ConfigHealingScenario::MalformedPinnedFolders)),
    }
}
