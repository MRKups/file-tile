//! Application-owned appearance preferences, independent of rendering and storage.

pub mod palette;
mod presets;
mod surface_texture;
mod user_themes;

use std::fmt;

pub use presets::{Preset, ThemeSpec};
pub use surface_texture::SurfaceTexture;
pub use user_themes::{
    CustomThemeId, InvalidCustomThemeId, ResolvedAppearance, ThemeChoice, UserTheme,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SystemAppearance {
    #[default]
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppearancePreferences {
    pub light_theme: ThemeChoice,
    pub dark_theme: ThemeChoice,
    pub transparency: Percent,
    pub box_transparency: BoxPercent,
    pub surface_texture: SurfaceTexture,
}

impl Default for AppearancePreferences {
    fn default() -> Self {
        Self {
            light_theme: ThemeChoice::Preset(Preset::LiquidLight),
            dark_theme: ThemeChoice::Preset(Preset::LiquidDark),
            transparency: Percent::LIQUID_DEFAULT,
            box_transparency: BoxPercent::DEFAULT,
            surface_texture: SurfaceTexture::default(),
        }
    }
}

impl AppearancePreferences {
    pub const fn theme_for(self, system: SystemAppearance) -> ThemeChoice {
        match system {
            SystemAppearance::Light => self.light_theme,
            SystemAppearance::Dark => self.dark_theme,
        }
    }

    pub fn resolve(
        self,
        system: SystemAppearance,
        user_themes: &[UserTheme],
    ) -> ResolvedAppearance {
        let (spec, liquid_accent) = match self.theme_for(system) {
            ThemeChoice::Preset(preset) => (preset.spec(), preset.is_liquid()),
            ThemeChoice::Custom(id) => {
                if let Some(theme) = user_themes.iter().find(|theme| theme.id == id) {
                    (theme.spec, false)
                } else {
                    let fallback = match system {
                        SystemAppearance::Light => Preset::LiquidLight,
                        SystemAppearance::Dark => Preset::LiquidDark,
                    };
                    (fallback.spec(), fallback.is_liquid())
                }
            }
        };
        ResolvedAppearance {
            spec,
            liquid_accent,
            transparency: self.transparency,
            box_transparency: self.box_transparency,
            surface_texture: self.surface_texture,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb8 {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Rgb8 {
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    pub fn parse_hex(input: &str) -> Option<Self> {
        let raw = input.trim();
        let hex = raw.strip_prefix('#').unwrap_or(raw);
        if hex.len() != 6 || !hex.is_ascii() {
            return None;
        }
        Some(Self {
            red: u8::from_str_radix(&hex[0..2], 16).ok()?,
            green: u8::from_str_radix(&hex[2..4], 16).ok()?,
            blue: u8::from_str_radix(&hex[4..6], 16).ok()?,
        })
    }

    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.red, self.green, self.blue)
    }
}

/// Window background transparency, from fully opaque (0) to fully transparent (100).
/// Liquid presets start at 50%; other themes start at 0%.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Percent(u8);

impl Percent {
    pub const OPAQUE: Self = Self(0);
    pub const LIQUID_DEFAULT: Self = Self(50);

    pub fn new(value: u8) -> Result<Self, InvalidAppearance> {
        if value > 100 {
            return Err(InvalidAppearance::Transparency(value));
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    pub fn alpha(self) -> f32 {
        1.0 - f32::from(self.0) / 100.0
    }
}

/// Box and shortcut surface transparency, from opaque (0) to 75%.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoxPercent(u8);

impl Default for BoxPercent {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl BoxPercent {
    pub const OPAQUE: Self = Self(0);
    pub const DEFAULT: Self = Self(20);
    pub const MAX: u8 = 75;

    pub fn new(value: u8) -> Result<Self, InvalidAppearance> {
        if value > Self::MAX {
            return Err(InvalidAppearance::BoxTransparency(value));
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    pub fn alpha(self) -> f32 {
        1.0 - f32::from(self.0) / 100.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidAppearance {
    Preset(String),
    Transparency(u8),
    BoxTransparency(u8),
}

impl fmt::Display for InvalidAppearance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preset(_) => f.write_str("Choose a theme from the preset list."),
            Self::Transparency(_) => f.write_str("Use a transparency from 0 to 100 percent."),
            Self::BoxTransparency(_) => f.write_str("Use a box transparency from 0 to 75 percent."),
        }
    }
}

impl std::error::Error for InvalidAppearance {}
