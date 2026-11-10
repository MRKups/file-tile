//! Scenario:
//! - `ConfigHealingScenario::MalformedViewTopItem`
//!
//! Heals malformed `top_item` values on workspace view records to `0` without
//! locking workspace autosave into read-only mode, while treating omitted keys
//! in pre-WO14 `schema_version: 1` files as `0` without healing notices.

use serde_json::Value;

use super::ConfigHealingScenario;

/// Restores a view's `top_item` ordinal anchor from JSON, healing invalid
/// present values to `0`.
pub(in crate::persistence) fn restore_top_item(
    value: Option<Value>,
) -> (u32, Option<ConfigHealingScenario>) {
    let Some(value) = value else {
        return (0, None);
    };
    match value.as_u64().and_then(|number| u32::try_from(number).ok()) {
        Some(top_item) => (top_item, None),
        None => (0, Some(ConfigHealingScenario::MalformedViewTopItem)),
    }
}
