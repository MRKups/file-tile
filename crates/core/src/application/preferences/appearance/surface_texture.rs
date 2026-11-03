//! Optional box surface shading and texture presets.

use std::fmt;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SurfaceTexture {
    #[default]
    Off,
    GlassSheen,
    Recessed,
    Prismatic,
}

impl SurfaceTexture {
    pub const ALL: [Self; 4] = [Self::Off, Self::GlassSheen, Self::Recessed, Self::Prismatic];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::GlassSheen => "Glass sheen",
            Self::Recessed => "Recessed well",
            Self::Prismatic => "Prismatic",
        }
    }

    pub const fn code(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::GlassSheen => "glass_sheen",
            Self::Recessed => "recessed",
            Self::Prismatic => "prismatic",
        }
    }

    pub fn parse(input: &str) -> Result<Self, &'static str> {
        Self::ALL
            .into_iter()
            .find(|preset| preset.code() == input)
            .ok_or("unknown surface texture")
    }
}

impl fmt::Display for SurfaceTexture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}
