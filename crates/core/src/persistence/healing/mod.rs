//! Discrete storage and configuration self-healing scenarios.
//!
//! Each scenario is isolated in its own submodule with an explicit trigger
//! condition and recovery action so configuration edge cases never mix into
//! general decoding or store logic.

pub(super) mod orphaned_temps;
pub(super) mod preferences_fields;
pub(super) mod workspace_fallback;

/// Named storage self-healing scenarios recognized on startup or decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigHealingScenario {
    /// Leftover `.file-tile-*.tmp` files in an initialized `config/` directory
    /// after an interrupted save or process crash.
    OrphanedTemporaryFiles,
    /// Missing or invalid `capabilities` block in `preferences.json` reset to
    /// application defaults without locking workspace autosave.
    MalformedCapabilitiesPreferences,
    /// Invalid field in `safety` block in `preferences.json` reset to safe
    /// defaults without locking workspace autosave.
    MalformedSafetyPreferences,
    /// Referenced active `workspace-<UUID>.json` was deleted on disk while
    /// another valid workspace exists in `workspaces.json`.
    MissingActiveWorkspaceFallback,
    /// Referenced active `workspace-<UUID>.json` was deleted on disk and no
    /// other workspace exists, recreating an empty workspace for that UUID.
    EmptyWorkspaceRecreation,
}

impl ConfigHealingScenario {
    /// Short identifier for diagnostics and notices.
    pub const fn code(self) -> &'static str {
        match self {
            Self::OrphanedTemporaryFiles => "orphaned_temporary_files",
            Self::MalformedCapabilitiesPreferences => "malformed_capabilities_preferences",
            Self::MalformedSafetyPreferences => "malformed_safety_preferences",
            Self::MissingActiveWorkspaceFallback => "missing_active_workspace_fallback",
            Self::EmptyWorkspaceRecreation => "empty_workspace_recreation",
        }
    }
}
