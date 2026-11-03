//! Safe platform adapter for outbound macOS file drag sessions.
pub use file_tile_platform_macos_appkit::drag::{
    DragCompletion, can_start_window_drag, reset_stale_session, start_outbound_drag,
    take_completed_drag,
};
