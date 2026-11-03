//! Query only the current OS UI font. No installed-font inventory or native drawing.
use objc2::MainThreadMarker;
use objc2_app_kit::NSFont;

pub fn system_font() -> Result<(String, String), String> {
    let _main = MainThreadMarker::new().ok_or("System font lookup requires the main thread.")?;
    let font = NSFont::systemFontOfSize(13.0);
    let raw_name = font.fontName().to_string();
    let family = font
        .familyName()
        .ok_or("macOS returned no system font family.")?
        .to_string();
    let name = if family == ".AppleSystemUIFont"
        || raw_name.starts_with(".AppleSystemUIFont")
        || raw_name.starts_with(".SFNS")
    {
        "San Francisco".into()
    } else if !family.starts_with('.') {
        family.clone()
    } else {
        font.displayName()
            .map(|display| display.to_string())
            .and_then(|display| {
                let trimmed = display.strip_suffix(" Regular").unwrap_or(&display).trim();
                (!trimmed.is_empty() && !trimmed.starts_with('.')).then(|| trimmed.to_owned())
            })
            .unwrap_or(raw_name)
    };
    Ok((name, family))
}
