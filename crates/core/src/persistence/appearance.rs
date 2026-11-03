//! Saved appearance records with field-level fallback to application defaults.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::application::preferences::appearance::{
    AppearancePreferences, BoxPercent, Percent, SurfaceTexture, ThemeChoice,
};

#[derive(Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct AppearanceRecord {
    light_theme: String,
    dark_theme: String,
    transparency: Option<u8>,
    box_transparency: Option<u8>,
    surface_texture: Option<String>,
}

pub(super) fn restore(value: Value) -> Option<AppearancePreferences> {
    let record: AppearanceRecord = serde_json::from_value(value).ok()?;
    let defaults = AppearancePreferences::default();
    Some(AppearancePreferences {
        light_theme: ThemeChoice::parse(&record.light_theme).unwrap_or(defaults.light_theme),
        dark_theme: ThemeChoice::parse(&record.dark_theme).unwrap_or(defaults.dark_theme),
        transparency: record
            .transparency
            .and_then(|v| Percent::new(v).ok())
            .unwrap_or(defaults.transparency),
        box_transparency: record
            .box_transparency
            .and_then(|v| BoxPercent::new(v).ok())
            .unwrap_or(defaults.box_transparency),
        surface_texture: record
            .surface_texture
            .as_deref()
            .and_then(|code| SurfaceTexture::parse(code).ok())
            .unwrap_or(defaults.surface_texture),
    })
}

impl From<AppearancePreferences> for AppearanceRecord {
    fn from(preferences: AppearancePreferences) -> Self {
        Self {
            light_theme: preferences.light_theme.encode(),
            dark_theme: preferences.dark_theme.encode(),
            transparency: Some(preferences.transparency.get()),
            box_transparency: Some(preferences.box_transparency.get()),
            surface_texture: Some(preferences.surface_texture.code().to_owned()),
        }
    }
}
