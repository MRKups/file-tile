//! Core domain, application state, persistence, diagnostics, and filesystem workers for `file-tile`.
//!
//! Enforces `unsafe_code = "forbid"` and contains no UI framework dependencies.

#[cfg(target_os = "linux")]
pub use file_tile_platform_linux as platform;
#[cfg(target_os = "macos")]
pub use file_tile_platform_macos as platform;
#[cfg(target_os = "windows")]
pub use file_tile_platform_windows as platform;

pub mod application;
pub mod diagnostics;
pub mod domain;
pub mod filesystem;
pub mod persistence;
