//! Iced window configuration. Native traffic lights remain OS-owned.

use std::sync::OnceLock;

use iced::{Size, window};

const APP_ICON_PNG: &[u8] = include_bytes!("../../../../assets/icons/app/icon.png");
#[cfg(all(target_os = "macos", not(test)))]
pub(crate) const APP_ICON_ICNS: &[u8] = include_bytes!("../../../../assets/icons/app/icon.icns");

pub const INITIAL_SIZE: Size = Size {
    width: crate::domain::DEFAULT_WINDOW_WIDTH as f32,
    height: crate::domain::DEFAULT_WINDOW_HEIGHT as f32,
};

pub const MIN_SIZE: Size = Size {
    width: crate::domain::MIN_WINDOW_WIDTH as f32,
    height: crate::domain::MIN_WINDOW_HEIGHT as f32,
};

pub fn settings() -> window::Settings {
    window::Settings {
        size: INITIAL_SIZE,
        min_size: Some(MIN_SIZE),
        icon: window_icon(),
        exit_on_close_request: false,
        // Create an alpha-capable surface once. Runtime appearance keeps its
        // background opaque until the active renderer supports desktop alpha.
        transparent: true,
        blur: true,
        decorations: true,
        // Disabling decorations would also remove the native traffic lights.
        // Extend our surface behind a transparent, untitled macOS titlebar instead.
        #[cfg(target_os = "macos")]
        platform_specific: window::settings::PlatformSpecific {
            title_hidden: true,
            titlebar_transparent: true,
            fullsize_content_view: true,
        },
        ..window::Settings::default()
    }
}

fn decode_app_icon() -> Option<(u32, u32, Vec<u8>)> {
    let decoded =
        ::image::load_from_memory_with_format(APP_ICON_PNG, ::image::ImageFormat::Png).ok()?;
    let rgba = decoded.into_rgba8();
    let (width, height) = rgba.dimensions();
    Some((width, height, rgba.into_raw()))
}

fn window_icon() -> Option<window::Icon> {
    static ICON: OnceLock<Option<window::Icon>> = OnceLock::new();
    ICON.get_or_init(|| {
        let (width, height, rgba) = decode_app_icon()?;
        window::icon::from_rgba(rgba, width, height).ok()
    })
    .clone()
}

#[cfg(test)]
mod size_tests {
    use super::*;

    #[test]
    fn native_resize_has_a_logical_minimum_without_limiting_large_displays() {
        let settings = settings();
        assert!(
            !settings.exit_on_close_request,
            "The application owns the save-before-close barrier"
        );
        assert_eq!(settings.min_size, Some(Size::new(480.0, 360.0)));
        assert!(settings.size.width >= MIN_SIZE.width);
        assert!(settings.size.height >= MIN_SIZE.height);
        assert!(settings.max_size.is_none());
        assert!(settings.resizable);
    }
}
