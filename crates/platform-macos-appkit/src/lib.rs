//! Isolated macOS AppKit adapters for outbound dragging, menus, open panels, and Sparkle updates.

#[cfg(target_os = "macos")]
pub mod drag;
#[cfg(target_os = "macos")]
#[forbid(unsafe_code)]
pub mod file_url;
#[cfg(target_os = "macos")]
pub mod menu;
#[cfg(target_os = "macos")]
#[forbid(unsafe_code)]
pub mod open_panel;
#[cfg(target_os = "macos")]
pub mod updater;
