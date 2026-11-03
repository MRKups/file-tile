//! Windows platform services for `file-tile`.
//!
//! Exposes the same module and function surface as `file-tile-platform-macos`
//! so `file-tile-core` and `file-tile-ui-iced` compile without OS-specific branching.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

pub type DisplayBounds = ((f32, f32, f32, f32), (f32, f32, f32, f32));
pub type PathChoice = Pin<Box<dyn Future<Output = Result<Option<PathBuf>, String>> + Send>>;

pub fn state_directory(home: &Path) -> PathBuf {
    home.join("AppData/Roaming/file-tile")
}

pub fn format_local_datetime(_time: SystemTime) -> Option<String> {
    None
}

pub fn system_accent_color() -> Option<(u8, u8, u8)> {
    None
}

pub fn window_cursor_position() -> Option<(f32, f32)> {
    None
}

pub fn display_areas() -> Result<Vec<DisplayBounds>, String> {
    Ok(Vec::new())
}

pub fn system_font() -> Result<(String, String), String> {
    Err("System font discovery is not yet implemented on Windows.".into())
}

pub fn choose_folder(_directory: PathBuf, _cancelled: Arc<AtomicBool>) -> PathChoice {
    Box::pin(async { Err("Folder chooser is not yet implemented on Windows.".into()) })
}

pub fn choose_target(_directory: PathBuf, _cancelled: Arc<AtomicBool>) -> PathChoice {
    Box::pin(async { Err("Target chooser is not yet implemented on Windows.".into()) })
}

pub fn cancel_chooser() -> bool {
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenDiag {
    Cancelled,
    TimedOut,
    Io(std::io::ErrorKind),
    FileFailure(&'static str),
}

pub struct OpenError {
    pub message: String,
    pub diagnostic: OpenDiag,
}

impl OpenError {
    pub fn safe(message: String, explanation: &'static str) -> Self {
        Self {
            message,
            diagnostic: OpenDiag::FileFailure(explanation),
        }
    }
}

impl From<&'static str> for OpenError {
    fn from(message: &'static str) -> Self {
        Self::safe(message.into(), message)
    }
}

pub fn open_file(_path: &Path, _is_cancelled: impl Fn() -> bool) -> Result<(), OpenError> {
    Err("External file opening is not yet implemented on Windows.".into())
}

pub fn open_directory(_path: &Path, _is_cancelled: impl Fn() -> bool) -> Result<(), OpenError> {
    Err("External folder opening is not yet implemented on Windows.".into())
}

pub mod launchers {
    use std::path::Path;

    use super::OpenError;

    pub fn open_path(_path: &Path, _is_cancelled: impl Fn() -> bool) -> Result<(), OpenError> {
        Err("Shortcut opening is not yet implemented on Windows.".into())
    }

    pub fn open_link(_link: &str, _is_cancelled: impl Fn() -> bool) -> Result<(), OpenError> {
        Err("Shortcut link opening is not yet implemented on Windows.".into())
    }
}

pub mod clipboard {
    use std::path::PathBuf;

    pub struct Files {
        pub paths: Vec<PathBuf>,
        pub cut: bool,
        pub change_count: isize,
    }

    pub fn copy_files(_paths: &[PathBuf]) -> Result<(), String> {
        Err("Clipboard file copy is not yet implemented on Windows.".into())
    }

    pub fn cut_files(_paths: &[PathBuf]) -> Result<(), String> {
        Err("Clipboard file cut is not yet implemented on Windows.".into())
    }

    pub fn consume_move(_change_count: isize) {}

    pub fn copy_text(_value: &str) -> Result<(), String> {
        Err("Clipboard text copy is not yet implemented on Windows.".into())
    }

    pub fn paste_files() -> Result<Files, String> {
        Err("Clipboard file paste is not yet implemented on Windows.".into())
    }
}

pub mod drag {
    use std::path::PathBuf;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct DragCompletion {
        pub session_id: u64,
        pub moved: bool,
    }

    pub fn start_outbound_drag(_paths: &[PathBuf], _allow_move: bool) -> Result<u64, String> {
        Err("Outbound file dragging is not yet implemented on Windows.".into())
    }

    pub fn take_completed_drag() -> Option<DragCompletion> {
        None
    }

    pub fn reset_stale_session() {}

    pub fn can_start_window_drag() -> bool {
        true
    }
}

pub mod file_copy {
    use std::path::Path;

    pub fn copy(_source: &Path, _destination: &Path) -> Result<(), String> {
        Err("Native file copy is not yet implemented on Windows.".into())
    }
}

pub mod folder_usage {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct RawUsage {
        pub total_bytes: u64,
        pub total_partial: bool,
        pub children: Vec<(PathBuf, u64, bool)>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum UsageFailure {
        Cancelled,
        TimedOut,
        Unavailable,
        InvalidOutput,
    }

    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
    pub struct UsageErrors {
        pub access_denied: u32,
        pub not_found: u32,
        pub io: u32,
        pub other: u32,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum UsageIssue {
        TimedOut,
        Unavailable,
        OutputLimit,
        InvalidOutput,
        UtilityExit {
            code: Option<i32>,
            errors: UsageErrors,
        },
    }

    pub fn measure_with_exclusions(
        _directory: &Path,
        _excluded: &[&str],
        _is_cancelled: impl Fn() -> bool,
    ) -> (Result<RawUsage, UsageFailure>, Option<UsageIssue>, Duration) {
        (
            Err(UsageFailure::Unavailable),
            Some(UsageIssue::Unavailable),
            Duration::ZERO,
        )
    }
}

pub mod menu {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum MenuAction {
        About,
        CheckForUpdates,
        Settings,
        SelectBrowse,
        SelectConfigure,
        SelectLayout,
        Quit,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum CheckedMode {
        Browse,
        Configure,
        Layout,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct NativeMenuState {
        pub about_enabled: bool,
        pub check_updates_enabled: bool,
        pub settings_enabled: bool,
        pub modes_enabled: bool,
        pub checked_mode: CheckedMode,
    }

    pub struct NativeMenu;

    impl NativeMenu {
        pub fn install(_on_action: impl Fn(MenuAction) + 'static) -> Result<Self, String> {
            Ok(Self)
        }

        pub fn update(&self, _state: NativeMenuState) {}
    }

    pub fn set_application_icon(_icon_bytes: &[u8]) {}
}

pub mod moving {
    use std::path::Path;

    pub enum MoveError {
        Conflict,
        CrossVolume,
        Failed(String),
    }

    pub fn move_entry(
        _source: &Path,
        _destination: &Path,
        _source_parent: (u64, u64),
        _destination_parent: (u64, u64),
        _original: (u64, u64),
    ) -> Result<(), MoveError> {
        Err(MoveError::Failed(
            "Exclusive native move is not yet implemented on Windows.".into(),
        ))
    }

    pub fn remove_verified_source(
        _source: &Path,
        _source_parent: (u64, u64),
        _original: (u64, u64),
        _is_dir: bool,
    ) -> Result<(), String> {
        Err("Verified source cleanup is not yet implemented on Windows.".into())
    }
}

pub mod permissions {
    use std::path::Path;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum PermissionStatus {
        Granted,
        Denied,
        Unavailable,
    }

    pub fn check_full_disk_access(_home: &Path) -> PermissionStatus {
        PermissionStatus::Unavailable
    }

    pub fn check_folder_access(_folder_name: Option<&str>, _home: &Path) -> PermissionStatus {
        PermissionStatus::Granted
    }

    pub fn request_folder_access(_folder_name: Option<&str>, _home: &Path) -> PermissionStatus {
        PermissionStatus::Unavailable
    }

    pub fn open_privacy_settings(_full_disk: bool) -> Result<(), String> {
        Err("Privacy settings deep links are not supported on Windows.".into())
    }
}

pub mod renaming {
    use std::path::Path;

    pub enum RenameError {
        Conflict,
        Failed(String),
    }

    pub fn rename(
        _source: &Path,
        _destination: &Path,
        _parent: (u64, u64),
        _original: (u64, u64),
    ) -> Result<(), RenameError> {
        Err(RenameError::Failed(
            "Exclusive native rename is not yet implemented on Windows.".into(),
        ))
    }
}

pub mod trash {
    use std::path::Path;

    pub fn trash(_target: &Path) -> Result<(), String> {
        Err("Native Recycle Bin placement is not yet implemented on Windows.".into())
    }
}

pub mod updater {
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum UpdaterStatus {
        Ready,
        AwaitingPublicKey,
        Unbundled,
        Unavailable(String),
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct UpdaterSnapshot {
        pub status: UpdaterStatus,
        pub can_check: bool,
        pub automatic_checks: bool,
    }

    pub struct ApplicationUpdater;

    impl ApplicationUpdater {
        pub fn install() -> Self {
            Self
        }

        pub fn snapshot(&self) -> UpdaterSnapshot {
            UpdaterSnapshot {
                status: UpdaterStatus::Unbundled,
                can_check: false,
                automatic_checks: false,
            }
        }

        pub fn check_for_updates(&self) -> Result<(), &'static str> {
            Err("In-app updates are not yet supported on Windows.")
        }

        pub fn set_automatic_checks(&self, _enabled: bool) -> Result<bool, &'static str> {
            Err("Automatic updates are not yet supported on Windows.")
        }
    }
}
