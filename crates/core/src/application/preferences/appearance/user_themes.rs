//! User-defined custom theme identities, choices, and pure resolution.

use std::fmt;

use uuid::Uuid;

use super::{BoxPercent, InvalidAppearance, Percent, Preset, ThemeSpec};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CustomThemeId(Uuid);

impl CustomThemeId {
    pub fn random() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn parse(value: &str) -> Result<Self, InvalidCustomThemeId> {
        let id = Uuid::parse_str(value).map_err(|_| InvalidCustomThemeId)?;
        if id.is_nil() || id.hyphenated().to_string() != value {
            return Err(InvalidCustomThemeId);
        }
        Ok(Self(id))
    }
}

impl fmt::Display for CustomThemeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.hyphenated().fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidCustomThemeId;

impl fmt::Display for InvalidCustomThemeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Use a canonical lowercase, hyphenated, non-nil custom theme UUID")
    }
}

impl std::error::Error for InvalidCustomThemeId {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeChoice {
    Preset(Preset),
    Custom(CustomThemeId),
}

impl Default for ThemeChoice {
    fn default() -> Self {
        Self::Preset(Preset::default())
    }
}

impl From<Preset> for ThemeChoice {
    fn from(preset: Preset) -> Self {
        Self::Preset(preset)
    }
}

impl ThemeChoice {
    pub const fn default_transparency(self) -> u8 {
        match self {
            Self::Preset(preset) => preset.default_transparency(),
            Self::Custom(_) => 0,
        }
    }

    pub fn encode(self) -> String {
        match self {
            Self::Preset(preset) => preset.as_str().to_owned(),
            Self::Custom(id) => format!("custom:{id}"),
        }
    }

    pub fn parse(value: &str) -> Result<Self, InvalidAppearance> {
        if let Some(raw) = value.strip_prefix("custom:") {
            return CustomThemeId::parse(raw)
                .map(Self::Custom)
                .map_err(|_| InvalidAppearance::Preset(value.into()));
        }
        Preset::parse(value).map(Self::Preset)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserTheme {
    pub id: CustomThemeId,
    pub name: String,
    pub spec: ThemeSpec,
}

impl UserTheme {
    pub const MAX_THEMES: usize = 128;
    pub const MAX_NAME_CHARS: usize = crate::domain::validation::MAX_CONFIG_NAME_CHARS;

    pub fn name_error(name: &str) -> Option<&'static str> {
        if let Err(error) = crate::domain::validation::validate_config_name(name) {
            return Some(error);
        }
        if Preset::ALL.iter().any(|preset| {
            preset.label().eq_ignore_ascii_case(name) || preset.as_str().eq_ignore_ascii_case(name)
        }) {
            return Some("Choose a name different from built-in themes.");
        }
        None
    }

    pub fn valid_name(name: &str) -> bool {
        Self::name_error(name).is_none()
    }

    pub fn validate_collection(themes: &[Self]) -> Result<(), &'static str> {
        if themes.len() > Self::MAX_THEMES {
            return Err("custom themes exceed the 128-theme limit");
        }
        for (index, theme) in themes.iter().enumerate() {
            if !Self::valid_name(&theme.name) {
                return Err(
                    "custom theme names must be 1 to 32 letters, numbers, single spaces, hyphens, or underscores without leading or trailing spaces and distinct from built-in presets",
                );
            }
            for earlier in &themes[..index] {
                if earlier.id == theme.id {
                    return Err("custom theme identifiers must be unique");
                }
                if earlier.name.eq_ignore_ascii_case(&theme.name) {
                    return Err("custom theme names must be unique");
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedAppearance {
    pub spec: ThemeSpec,
    pub liquid_accent: bool,
    pub transparency: Percent,
    pub box_transparency: BoxPercent,
    pub surface_texture: super::SurfaceTexture,
}

impl Default for ResolvedAppearance {
    fn default() -> Self {
        let preset = Preset::default();
        Self {
            spec: preset.spec(),
            liquid_accent: preset.is_liquid(),
            transparency: Percent::LIQUID_DEFAULT,
            box_transparency: BoxPercent::OPAQUE,
            surface_texture: super::SurfaceTexture::default(),
        }
    }
}
