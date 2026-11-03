//! Read display frames on Iced's window thread, without owning a native window.
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSColor, NSColorSpace, NSScreen};

pub type DisplayBounds = ((f32, f32, f32, f32), (f32, f32, f32, f32));

pub fn system_accent_color() -> Option<(u8, u8, u8)> {
    if cfg!(test) {
        return None;
    }
    let _main = MainThreadMarker::new()?;
    let srgb = NSColorSpace::sRGBColorSpace();
    let accent = NSColor::controlAccentColor().colorUsingColorSpace(&srgb)?;
    let channel = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    Some((
        channel(accent.redComponent()),
        channel(accent.greenComponent()),
        channel(accent.blueComponent()),
    ))
}

pub fn window_cursor_position() -> Option<(f32, f32)> {
    let main = MainThreadMarker::new()?;
    let app = NSApplication::sharedApplication(main);
    let window = app
        .keyWindow()
        .or_else(|| app.mainWindow())
        .or_else(|| app.windows().firstObject())?;
    let view = window.contentView()?;
    let frame = view.frame();
    let point = window.mouseLocationOutsideOfEventStream();
    let x = (point.x - frame.origin.x) as f32;
    let y = (frame.origin.y + frame.size.height - point.y) as f32;
    (x.is_finite()
        && y.is_finite()
        && x >= 0.0
        && y >= 0.0
        && x <= frame.size.width as f32
        && y <= frame.size.height as f32)
        .then_some((x, y))
}

pub fn display_areas() -> Result<Vec<DisplayBounds>, String> {
    let main = MainThreadMarker::new().ok_or("Display discovery requires the main thread.")?;
    let screens = NSScreen::screens(main);
    let primary = screens
        .firstObject()
        .ok_or("macOS reported no connected displays.")?;
    // NSScreen's first screen owns the menu bar, not necessarily keyboard focus.
    // Winit uses this same desktop origin, with Y increasing downwards.
    let primary_height = primary.frame().size.height;
    let mut displays = Vec::with_capacity(screens.len());
    for screen in &screens {
        let frame = screen.frame();
        let work = screen.visibleFrame();
        let convert = |x: f64, y: f64, width: f64, height: f64| {
            let x = x as f32;
            let y = (primary_height - y - height) as f32;
            let width = width as f32;
            let height = height as f32;
            (x.is_finite() && y.is_finite() && width.is_finite() && height.is_finite())
                .then_some((x, y, width, height))
        };
        let frame = convert(
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
        )
        .ok_or("macOS returned invalid display bounds.")?;
        let work = convert(
            work.origin.x,
            work.origin.y,
            work.size.width,
            work.size.height,
        )
        .ok_or("macOS returned an invalid display work area.")?;
        displays.push((frame, work));
    }
    Ok(displays)
}
