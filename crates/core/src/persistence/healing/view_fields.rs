//! Scenarios:
//! - `ConfigHealingScenario::MalformedViewTopItem`
//! - `ConfigHealingScenario::MalformedBoxSidebar`
//!
//! Heals malformed `top_item` and `sidebar` values on workspace view and box
//! records without locking workspace autosave into read-only mode, while
//! treating omitted keys in pre-WO14 and pre-WO15 `schema_version: 1` files as
//! defaults without healing notices.

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

/// Restores a box's `sidebar` flag from JSON, healing invalid or conflicting
/// present values (when `tabs_visible` is also `true`) to `false`.
pub(in crate::persistence) fn restore_box_sidebar(
    value: Option<Value>,
    tabs_visible: bool,
) -> (bool, Option<ConfigHealingScenario>) {
    match value {
        None => (false, None),
        Some(Value::Bool(sidebar)) => {
            if sidebar && tabs_visible {
                (false, Some(ConfigHealingScenario::MalformedBoxSidebar))
            } else {
                (sidebar, None)
            }
        }
        Some(_) => (false, Some(ConfigHealingScenario::MalformedBoxSidebar)),
    }
}
