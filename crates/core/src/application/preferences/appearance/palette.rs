//! Pure semantic colors mapped directly from explicit preset definitions.

use super::{ResolvedAppearance, Rgb8, ThemeSpec};

#[derive(Debug, Clone, Copy)]
pub struct Palettes {
    pub window: Palette,
    pub boxes: Palette,
}

#[derive(Debug, Clone, Copy)]
pub struct Rgba {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
    pub alpha: f32,
}

impl Rgba {
    const fn black(alpha: f32) -> Self {
        Self {
            red: 0.0,
            green: 0.0,
            blue: 0.0,
            alpha,
        }
    }

    fn from_rgb(color: Rgb8, alpha: f32) -> Self {
        Self {
            red: f32::from(color.red) / 255.0,
            green: f32::from(color.green) / 255.0,
            blue: f32::from(color.blue) / 255.0,
            alpha,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub background: Rgb8,
    pub background_alpha: f32,
    pub contrast_background: Rgb8,
    pub text: Rgb8,
    pub on_accent: Rgb8,
    pub path_text: Rgb8,
    pub muted: Rgb8,
    pub accent: Rgb8,
    pub border: Rgb8,
    pub selected_border: Rgb8,
    pub tool_active: Rgba,
    pub tool_active_icon: Rgb8,
    pub tool_hover: Rgba,
    pub navigation: Rgb8,
    pub row_hover: Rgba,
    pub row_selected: Rgba,
    pub grid: Rgba,
    pub grid_hover: Rgba,
    pub grid_lit: Rgba,
    pub grid_invalid: Rgba,
    pub grid_border: Rgba,
    pub preview: Rgb8,
    pub preview_invalid: Rgb8,
    pub error: Rgb8,
    pub remove_fill: Rgba,
    pub mode_text: Rgb8,
    pub path_border: Rgb8,
    pub editing_overlay: Rgb8,
    pub tab_text: Rgb8,
    pub empty_title: Rgb8,
    pub empty_help: Rgb8,
    pub folder_icon: Rgb8,
    pub file_icon: Rgb8,
    pub image_icon: Rgb8,
}

pub fn resolve_with_accent(
    appearance: ResolvedAppearance,
    system_accent: Option<Rgb8>,
) -> Palettes {
    let spec = spec_with_accent(appearance, system_accent);
    Palettes {
        window: build(spec, spec.window, appearance.transparency.alpha()),
        boxes: build(spec, spec.boxes, appearance.box_transparency.alpha()),
    }
}

/// Opaque Configure card surface using the active theme's foreground tokens.
pub fn configure_with_accent(
    appearance: ResolvedAppearance,
    system_accent: Option<Rgb8>,
) -> Palette {
    let spec = spec_with_accent(appearance, system_accent);
    build(spec, spec.configure, 1.0)
}

fn spec_with_accent(appearance: ResolvedAppearance, system_accent: Option<Rgb8>) -> ThemeSpec {
    let mut spec = appearance.spec;
    if appearance.liquid_accent
        && let Some(os_accent) = system_accent
    {
        spec.accent = os_accent;
    }
    spec
}

fn build(spec: ThemeSpec, background: Rgb8, background_alpha: f32) -> Palette {
    let accent = spec.accent;
    let on_accent = on_accent_color(accent);
    let dark = relative_luminance(background) < 0.25;
    let (row_hover, row_selected, tool_hover, tool_active, grid, grid_border) = if dark {
        let hover_tint = mix(spec.text, accent, 0.35);
        let fill_alpha = background_alpha.max(0.88);
        (
            Rgba::from_rgb(mix(background, hover_tint, 0.10), fill_alpha),
            Rgba::from_rgb(mix(background, accent, 0.20), fill_alpha),
            Rgba::from_rgb(mix(background, hover_tint, 0.14), fill_alpha),
            Rgba::from_rgb(mix(background, accent, 0.22), fill_alpha),
            Rgba::from_rgb(spec.border, 0.35),
            Rgba::from_rgb(spec.border, 0.85),
        )
    } else {
        (
            Rgba::black(0.06),
            Rgba::black(0.12),
            Rgba::black(0.10),
            Rgba::black(0.16),
            Rgba::black(0.035),
            Rgba::black(0.14),
        )
    };
    Palette {
        background,
        background_alpha,
        contrast_background: background,
        text: spec.text,
        on_accent,
        path_text: spec.text,
        muted: spec.muted,
        accent,
        border: spec.border,
        selected_border: accent,
        tool_active,
        tool_active_icon: accent,
        tool_hover,
        navigation: accent,
        row_hover,
        row_selected,
        grid,
        grid_hover: Rgba::from_rgb(accent, if dark { 0.12 } else { 0.16 }),
        grid_lit: Rgba::from_rgb(accent, if dark { 0.20 } else { 0.24 }),
        grid_invalid: Rgba::from_rgb(spec.error, 0.20),
        grid_border,
        preview: accent,
        preview_invalid: spec.error,
        error: spec.error,
        remove_fill: Rgba::from_rgb(spec.error, 0.15),
        mode_text: spec.text,
        path_border: accent,
        editing_overlay: background,
        tab_text: spec.text,
        empty_title: spec.muted,
        empty_help: spec.muted,
        folder_icon: accent,
        file_icon: spec.muted,
        image_icon: accent,
    }
}

fn mix(base: Rgb8, tint: Rgb8, weight: f32) -> Rgb8 {
    let channel = |a: u8, b: u8| {
        (f32::from(a).mul_add(1.0 - weight, f32::from(b) * weight))
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Rgb8::new(
        channel(base.red, tint.red),
        channel(base.green, tint.green),
        channel(base.blue, tint.blue),
    )
}

fn relative_luminance(color: Rgb8) -> f32 {
    let channel = |c: u8| {
        let v = f32::from(c) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.red) + 0.7152 * channel(color.green) + 0.0722 * channel(color.blue)
}

/// Select legible label ink on top of an accent-filled primary control without
/// altering the accent color itself.
fn on_accent_color(accent: Rgb8) -> Rgb8 {
    if relative_luminance(accent) > 0.22 {
        Rgb8::new(17, 20, 24)
    } else {
        Rgb8::new(255, 255, 255)
    }
}
