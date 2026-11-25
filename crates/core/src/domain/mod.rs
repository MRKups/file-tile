//! Pure workspace and window decisions, without widgets, disk, or platform calls.

mod geometry;
mod navigation;
mod scope;
pub use scope::{Range as NavigationRange, Scope, ScopeKind};
#[cfg(test)]
mod tests;
mod window;
mod workspace;

pub use geometry::{Cell, GridExtent, GridRect};
pub(crate) use navigation::normalize_path;
pub use navigation::{Navigation, NavigationIntent, PathInputError, expand_path};
pub use window::{
    DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_WIDTH, DisplayArea, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH,
    WindowPlacement, WindowRestoration, restore_window,
};
pub use workspace::{
    BoxId, BoxSetting, BoxSettings, DrawOutcome, DrawPreview, DrawRejection, FileView, FolderBox,
    FolderView, GridDelta, ItemKind, ItemPlacement, LauncherConfig, LauncherIcon, LauncherTarget,
    LauncherTile, LayoutKind, LayoutOutcome, LayoutPreview, LeftPanel, Mode, ResizeHandle,
    TabCommand, Tabs, UndoOutcome, ViewId, ViewRef, Workspace,
};
pub mod files;
pub mod matching;
pub mod safety;
pub mod sorting;
pub mod validation;

pub use file_tile_capabilities::{
    CapabilityItem, CapabilityKind, CapabilityPreferences, FolderCapabilities,
};
pub use safety::{MAX_DISABLED_LOCATIONS, ProtectedAccess, SafetyPolicy, SafetyPreferences};
