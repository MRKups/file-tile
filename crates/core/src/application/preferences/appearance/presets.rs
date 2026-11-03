//! Stable preset identities and explicit theme palettes.

use super::{InvalidAppearance, Rgb8};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Preset {
    #[default]
    LiquidLight,
    LiquidDark,
    GithubLight,
    GithubDark,
    Nord,
    TokyoNight,
    CatppuccinLatte,
}

impl Preset {
    pub const ALL: [Self; 7] = [
        Self::LiquidLight,
        Self::LiquidDark,
        Self::GithubLight,
        Self::GithubDark,
        Self::Nord,
        Self::TokyoNight,
        Self::CatppuccinLatte,
    ];

    pub const fn is_liquid(self) -> bool {
        matches!(self, Self::LiquidLight | Self::LiquidDark)
    }

    pub const fn default_transparency(self) -> u8 {
        match self {
            Self::LiquidLight | Self::LiquidDark => 50,
            _ => 0,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::LiquidLight => "Liquid Light",
            Self::LiquidDark => "Liquid Dark",
            Self::GithubLight => "GitHub Light",
            Self::GithubDark => "GitHub Dark",
            Self::Nord => "Nord",
            Self::TokyoNight => "Tokyo Night",
            Self::CatppuccinLatte => "Catppuccin Latte",
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LiquidLight => "liquid_light",
            Self::LiquidDark => "liquid_dark",
            Self::GithubLight => "github_light",
            Self::GithubDark => "github_dark",
            Self::Nord => "nord",
            Self::TokyoNight => "tokyo_night",
            Self::CatppuccinLatte => "catppuccin_latte",
        }
    }

    pub fn parse(value: &str) -> Result<Self, InvalidAppearance> {
        match value {
            "liquid_light" => Ok(Self::LiquidLight),
            "liquid_dark" => Ok(Self::LiquidDark),
            "github_light" => Ok(Self::GithubLight),
            "github_dark" => Ok(Self::GithubDark),
            "nord" => Ok(Self::Nord),
            "tokyo_night" => Ok(Self::TokyoNight),
            "catppuccin_latte" => Ok(Self::CatppuccinLatte),
            _ => Err(InvalidAppearance::Preset(value.into())),
        }
    }

    pub const fn spec(self) -> ThemeSpec {
        match self {
            Self::LiquidLight => ThemeSpec {
                window: color(0xe4e8f0),
                boxes: color(0xffffff),
                configure: color(0xf4f6fa),
                text: color(0x14181f),
                muted: color(0x5b6577),
                border: color(0xcfd6e2),
                accent: color(0x007aff),
                error: color(0xd92d20),
            },
            Self::LiquidDark => ThemeSpec {
                window: color(0x0d1017),
                boxes: color(0x171c26),
                configure: color(0x202736),
                text: color(0xeef2f7),
                muted: color(0x94a3b8),
                border: color(0x2b3445),
                accent: color(0x0a84ff),
                error: color(0xff453a),
            },
            Self::GithubLight => ThemeSpec {
                window: color(0xeaeef2),
                boxes: color(0xffffff),
                configure: color(0xf6f8fa),
                text: color(0x1f2328),
                muted: color(0x59636e),
                border: color(0xd0d7de),
                accent: color(0x0969da),
                error: color(0xcf222e),
            },
            Self::GithubDark => ThemeSpec {
                window: color(0x010409),
                boxes: color(0x0d1117),
                configure: color(0x161b22),
                text: color(0xe6edf3),
                muted: color(0x8b949e),
                border: color(0x30363d),
                accent: color(0x2f81f7),
                error: color(0xf85149),
            },
            Self::Nord => ThemeSpec {
                window: color(0x242933),
                boxes: color(0x2e3440),
                configure: color(0x3b4252),
                text: color(0xeceff4),
                muted: color(0xb4bece),
                border: color(0x434c5e),
                accent: color(0x88c0d0),
                error: color(0xbf616a),
            },
            Self::TokyoNight => ThemeSpec {
                window: color(0x13141c),
                boxes: color(0x1a1b26),
                configure: color(0x24283b),
                text: color(0xc0caf5),
                muted: color(0x8a93ba),
                border: color(0x2f3549),
                accent: color(0x7aa2f7),
                error: color(0xf7768e),
            },
            Self::CatppuccinLatte => ThemeSpec {
                window: color(0xdce0e8),
                boxes: color(0xeff1f5),
                configure: color(0xe6e9ef),
                text: color(0x4c4f69),
                muted: color(0x6c6f85),
                border: color(0xbcc0cc),
                accent: color(0x8839ef),
                error: color(0xd20f39),
            },
        }
    }
}

impl std::fmt::Display for Preset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeSpec {
    pub window: Rgb8,
    pub boxes: Rgb8,
    pub configure: Rgb8,
    pub text: Rgb8,
    pub muted: Rgb8,
    pub border: Rgb8,
    pub accent: Rgb8,
    pub error: Rgb8,
}

const fn color(hex: u32) -> Rgb8 {
    Rgb8::new((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}
