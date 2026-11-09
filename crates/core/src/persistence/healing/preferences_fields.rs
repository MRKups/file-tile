//! Scenarios:
//! - `ConfigHealingScenario::MalformedCapabilitiesPreferences`
//! - `ConfigHealingScenario::MalformedSafetyPreferences`
//!
//! Heals missing, outdated, or malformed fields in the `capabilities` and
//! `safety` sections of `preferences.json` to safe defaults without locking
//! workspace autosave into read-only mode.

use serde_json::Value;

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

    let mut healed = map.len() != 7;
    let bool_field = |key: &str, fallback: bool, healed: &mut bool| -> bool {
        match map.get(key).and_then(Value::as_bool) {
            Some(val) => val,
            None => {
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
        system_folders: bool_field("system_folders", defaults.system_folders, &mut healed),
        applications: bool_field("applications", defaults.applications, &mut healed),
        credentials: bool_field("credentials", defaults.credentials, &mut healed),
    };
    let scenario = healed.then_some(ConfigHealingScenario::MalformedSafetyPreferences);
    (preferences, scenario)
}
