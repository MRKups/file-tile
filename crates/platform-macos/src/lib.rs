//! Safe macOS platform services for `file-tile`.
//!
//! Objective-C class registration and `msg_send!` FFI stay isolated in
//! `file-tile-platform-macos-appkit` while this crate enforces `unsafe_code = "forbid"`.

pub mod clipboard;
mod datetime;
mod displays;
pub mod drag;
pub mod file_copy;
mod file_urls;
mod folder_picker;
pub mod folder_usage;
pub mod launchers;
pub mod menu;
pub mod moving;
mod opening;
pub mod permissions;
pub mod renaming;
mod system_font;
pub mod trash;
pub mod updater;
pub mod volumes;

pub use datetime::format_local_datetime;
pub use displays::{DisplayBounds, display_areas, system_accent_color, window_cursor_position};
pub use folder_picker::{PathChoice, cancel_chooser, choose_folder, choose_target};
pub use opening::{OpenDiag, OpenError, open_directory, open_file};
pub use system_font::system_font;

use std::path::{Path, PathBuf};

/// Pure location policy. Directory creation belongs to the storage worker.
pub fn state_directory(home: &Path) -> PathBuf {
    home.join("Library/Application Support/file-tile")
}
