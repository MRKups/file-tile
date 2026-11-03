//! Modular box surface shading and texture styles, seeded once per process session.

use std::sync::OnceLock;

use iced::widget::{Stack, container, space};
use iced::{Color, Element, Fill, Radians, Shadow, Vector, gradient};

use crate::application::preferences::appearance::SurfaceTexture;

use super::Palette;

static SESSION_SALT: OnceLock<u64> = OnceLock::new();

pub fn box_panel(palette: Palette, box_id: u64) -> container::Style {
    let base = palette.panel();
    let alpha = palette.background.a;
    let seed = session_seed(box_id);
    let background = match palette.surface_texture {
        SurfaceTexture::Off => return base,
        SurfaceTexture::GlassSheen => glass_panel_background(palette, seed),
        SurfaceTexture::Prismatic => prismatic_panel_background(palette, seed),
        SurfaceTexture::Recessed => iced::Background::Color(palette.background),
    };
    container::Style {
        background: Some(background),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.22 * alpha),
            offset: Vector::new(0.0, 3.0),
            blur_radius: 12.0,
        },
        ..base
    }
}

pub fn overlay<'a, Message: 'a>(palette: Palette, box_id: u64) -> Option<Element<'a, Message>> {
    let seed = session_seed(box_id);
    match palette.surface_texture {
        SurfaceTexture::Off => None,
        SurfaceTexture::GlassSheen => Some(layer(move || glass_sheen_overlay(palette, seed))),
        SurfaceTexture::Recessed => Some(recessed_overlay(palette)),
        SurfaceTexture::Prismatic => Some(layer(move || prismatic_overlay(palette, seed))),
    }
}

fn glass_panel_background(palette: Palette, seed: u64) -> iced::Background {
    let alpha = palette.background.a;
    let angle = mix_unit(seed, 1) * std::f32::consts::TAU;
    let pivot = 0.35 + mix_unit(seed, 2) * 0.30;
    let delta = (0.026 + mix_unit(seed, 3) * 0.012) * alpha;
    let light = offset_color(
        palette.background,
        delta,
        delta,
        delta * 1.08,
        (alpha + 0.018 * alpha).min(1.0),
    );
    let dark = offset_color(palette.background, -delta, -delta, -delta * 0.92, alpha);
    let ramp = gradient::Linear::new(Radians(angle))
        .add_stop(0.0, light)
        .add_stop(pivot, palette.background)
        .add_stop(1.0, dark);
    iced::Background::Gradient(iced::Gradient::Linear(ramp))
}

fn prismatic_panel_background(palette: Palette, seed: u64) -> iced::Background {
    let alpha = palette.background.a;
    let angle = mix_unit(seed, 1) * std::f32::consts::TAU;
    let warm = blend_rgb(palette.background, palette.accent, 0.085 * alpha);
    let cool = blend_rgb(palette.background, palette.folder_icon, 0.075 * alpha);
    let ramp = gradient::Linear::new(Radians(angle))
        .add_stop(0.0, warm)
        .add_stop(0.52, palette.background)
        .add_stop(1.0, cool);
    iced::Background::Gradient(iced::Gradient::Linear(ramp))
}

fn layer<'a, Message: 'a>(style: impl Fn() -> container::Style + 'a) -> Element<'a, Message> {
    container(space())
        .width(Fill)
        .height(Fill)
        .style(move |_| style())
        .into()
}

fn glass_sheen_overlay(palette: Palette, seed: u64) -> container::Style {
    let alpha = palette.background.a;
    let base_angle = mix_unit(seed, 1) * std::f32::consts::TAU;
    let cross_angle = base_angle + 1.25 + mix_unit(seed, 6) * 0.75;
    let center = 0.28 + mix_unit(seed, 7) * 0.44;
    let band_half = 0.16;
    let ramp = gradient::Linear::new(Radians(cross_angle))
        .add_stop(0.0, Color::from_rgba(0.0, 0.0, 0.0, 0.024 * alpha))
        .add_stop((center - band_half).max(0.05), Color::TRANSPARENT)
        .add_stop(center, Color::from_rgba(1.0, 1.0, 1.0, 0.028 * alpha))
        .add_stop((center + band_half).min(0.95), Color::TRANSPARENT)
        .add_stop(1.0, Color::from_rgba(0.0, 0.0, 0.0, 0.024 * alpha));
    gradient_style(ramp)
}

fn recessed_overlay<'a, Message: 'a>(palette: Palette) -> Element<'a, Message> {
    let alpha = palette.background.a;
    let edge = Color::from_rgba(0.0, 0.0, 0.0, 0.14 * alpha);
    let soft = Color::from_rgba(0.0, 0.0, 0.0, 0.035 * alpha);
    let center = Color::from_rgba(1.0, 1.0, 1.0, 0.018 * alpha);
    let vertical = gradient::Linear::new(Radians(0.0))
        .add_stop(0.0, edge)
        .add_stop(0.12, soft)
        .add_stop(0.50, center)
        .add_stop(0.88, soft)
        .add_stop(1.0, edge);
    let horizontal = gradient::Linear::new(Radians(std::f32::consts::FRAC_PI_2))
        .add_stop(0.0, edge)
        .add_stop(0.10, soft)
        .add_stop(0.50, Color::TRANSPARENT)
        .add_stop(0.90, soft)
        .add_stop(1.0, edge);
    Stack::new()
        .width(Fill)
        .height(Fill)
        .push(layer(move || gradient_style(vertical)))
        .push(layer(move || gradient_style(horizontal)))
        .into()
}

fn prismatic_overlay(palette: Palette, seed: u64) -> container::Style {
    let alpha = palette.background.a;
    let angle = mix_unit(seed, 1) * std::f32::consts::TAU + 1.35;
    let pivot = 0.35 + mix_unit(seed, 13) * 0.30;
    let tint_a = Color {
        a: 0.045 * alpha,
        ..palette.selected_border
    };
    let tint_b = Color {
        a: 0.040 * alpha,
        ..palette.image_icon
    };
    let mid = Color::from_rgba(1.0, 1.0, 1.0, 0.018 * alpha);
    let ramp = gradient::Linear::new(Radians(angle))
        .add_stop(0.0, tint_a)
        .add_stop(pivot, mid)
        .add_stop(0.70, Color::TRANSPARENT)
        .add_stop(1.0, tint_b);
    gradient_style(ramp)
}

fn gradient_style(ramp: gradient::Linear) -> container::Style {
    container::Style {
        background: Some(iced::Background::Gradient(iced::Gradient::Linear(ramp))),
        ..container::Style::default()
    }
}

fn offset_color(base: Color, dr: f32, dg: f32, db: f32, a: f32) -> Color {
    Color {
        r: (base.r + dr).clamp(0.0, 1.0),
        g: (base.g + dg).clamp(0.0, 1.0),
        b: (base.b + db).clamp(0.0, 1.0),
        a,
    }
}

fn blend_rgb(base: Color, tint: Color, weight: f32) -> Color {
    let w = weight.clamp(0.0, 1.0);
    Color {
        r: base.r * (1.0 - w) + tint.r * w,
        g: base.g * (1.0 - w) + tint.g * w,
        b: base.b * (1.0 - w) + tint.b * w,
        a: base.a,
    }
}

fn session_seed(box_id: u64) -> u64 {
    let salt = *SESSION_SALT.get_or_init(|| uuid::Uuid::new_v4().as_u128() as u64);
    splitmix(box_id ^ salt)
}

fn mix_unit(seed: u64, salt: u64) -> f32 {
    let z = splitmix(seed.wrapping_add(salt.wrapping_mul(0x9e37_79b9_7f4a_7c15)));
    f32::from((z >> 48) as u16) / 65_535.0
}

const fn splitmix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}
