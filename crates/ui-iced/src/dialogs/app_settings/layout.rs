//! Responsive modal bounds, without changing the application's native minimum.
use iced::Size;

use crate::window::window_shell;

pub(super) const MARGIN: f32 = 16.0;
const MAX_SIZE: Size = Size::new(1040.0, 820.0);

pub(super) fn padding(panel: Size) -> u16 {
    if panel.width < 520.0 { 16 } else { 24 }
}

pub(super) fn form_width(panel: Size) -> f32 {
    (panel.width - 2.0 * f32::from(padding(panel)) - 17.0).clamp(1.0, 960.0)
}

pub(super) fn column_widths(width: f32) -> (f32, f32) {
    if width < 680.0 {
        (width, width)
    } else {
        let available = width - 36.0;
        (available * 0.45, available * 0.55)
    }
}

pub(super) fn panel_size(window: Size) -> Size {
    Size::new(
        (window.width * 0.80)
            .clamp(window_shell::MIN_SIZE.width - 2.0 * MARGIN, MAX_SIZE.width)
            .min((window.width - 2.0 * MARGIN).max(1.0)),
        (window.height * 0.85)
            .clamp(
                window_shell::MIN_SIZE.height - 2.0 * MARGIN,
                MAX_SIZE.height,
            )
            .min((window.height - 2.0 * MARGIN).max(1.0)),
    )
}
