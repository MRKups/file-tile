//! Core domain, application state, persistence, diagnostics, and filesystem workers for `file-tile`.
//!
//! Enforces `unsafe_code = "forbid"` and contains no UI framework dependencies.

/// Single application version string from `[workspace.package]` in `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Build-time Git commit identifier (`<short-sha>`, `<short-sha>+modified`, or `unknown`).
pub const GIT_COMMIT: &str = env!("FILE_TILE_GIT_COMMIT");

#[cfg(target_os = "linux")]
pub use file_tile_platform_linux as platform;
#[cfg(target_os = "macos")]
pub use file_tile_platform_macos as platform;
#[cfg(target_os = "windows")]
pub use file_tile_platform_windows as platform;

pub use file_tile_capabilities as capabilities;

pub mod application;
pub mod diagnostics;
pub mod domain;
pub mod filesystem;
pub mod persistence;
