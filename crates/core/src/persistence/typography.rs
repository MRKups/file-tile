//! Strict typography records. Invalid values keep recovery read-only.
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::application::preferences::typography::{
    TextSize, TextWeight, Typeface, TypographyPreferences,
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TypographyRecord {
    typeface: String,
    size: u8,
    weight: String,
}

pub(super) fn restore(value: Value) -> Option<TypographyPreferences> {
    let record: TypographyRecord = serde_json::from_value(value).ok()?;
    let typeface = Typeface::parse(&record.typeface).unwrap_or(Typeface::SystemDefault);
    let size = TextSize::new(record.size).unwrap_or_default();
    let weight = TextWeight::parse(&record.weight).unwrap_or_default();
    Some(
        TypographyPreferences {
            typeface: Typeface::SystemDefault,
            size,
            weight,
        }
        .with_typeface(typeface),
    )
}

impl From<TypographyPreferences> for TypographyRecord {
    fn from(preferences: TypographyPreferences) -> Self {
        Self {
            typeface: preferences.typeface.encode(),
            size: preferences.size.get(),
            weight: preferences.weight.key().into(),
        }
    }
}
