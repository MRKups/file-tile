//! Optional `user-themes.json` codec and atomic publication.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::application::contracts::storage::StorageFailure;
use crate::application::preferences::appearance::{CustomThemeId, Rgb8, ThemeSpec, UserTheme};

use super::codec::decode_header;
use super::files::{Temporary, read_optional, sync_directory};
use super::{ConfigError, FORMAT, SCHEMA_VERSION};

const FILENAME: &str = "user-themes.json";
const BYTE_LIMIT: usize = 256 * 1024;
const KIND: &str = "user_themes";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DocumentPayload {
    themes: Vec<ThemeRecord>,
}

#[derive(Serialize)]
struct EncodedDocument {
    format: &'static str,
    schema_version: u32,
    kind: &'static str,
    themes: Vec<ThemeRecord>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ThemeRecord {
    id: String,
    name: String,
    window: String,
    boxes: String,
    configure: String,
    text: String,
    muted: String,
    border: String,
    accent: String,
    error: String,
}

fn decode(bytes: &[u8]) -> Result<Vec<UserTheme>, ConfigError> {
    let payload: DocumentPayload = serde_json::from_value(decode_header(bytes, BYTE_LIMIT, KIND)?)
        .map_err(|_| ConfigError::InvalidPayload)?;
    if payload.themes.len() > UserTheme::MAX_THEMES {
        return Err(ConfigError::LimitExceeded);
    }
    let mut themes = Vec::with_capacity(payload.themes.len());
    for record in payload.themes {
        themes.push(parse_record(record)?);
    }
    UserTheme::validate_collection(&themes)
        .map_err(|reason| ConfigError::InvalidState(reason.into()))?;
    Ok(themes)
}

fn parse_record(record: ThemeRecord) -> Result<UserTheme, ConfigError> {
    let hex = |raw: &str| Rgb8::parse_hex(raw).ok_or(ConfigError::InvalidPayload);
    Ok(UserTheme {
        id: CustomThemeId::parse(&record.id).map_err(|_| ConfigError::InvalidPayload)?,
        name: record.name,
        spec: ThemeSpec {
            window: hex(&record.window)?,
            boxes: hex(&record.boxes)?,
            configure: hex(&record.configure)?,
            text: hex(&record.text)?,
            muted: hex(&record.muted)?,
            border: hex(&record.border)?,
            accent: hex(&record.accent)?,
            error: hex(&record.error)?,
        },
    })
}

fn encode(themes: &[UserTheme]) -> Result<Vec<u8>, ConfigError> {
    UserTheme::validate_collection(themes)
        .map_err(|reason| ConfigError::InvalidState(reason.into()))?;
    let records = themes
        .iter()
        .map(|theme| ThemeRecord {
            id: theme.id.to_string(),
            name: theme.name.clone(),
            window: theme.spec.window.to_hex(),
            boxes: theme.spec.boxes.to_hex(),
            configure: theme.spec.configure.to_hex(),
            text: theme.spec.text.to_hex(),
            muted: theme.spec.muted.to_hex(),
            border: theme.spec.border.to_hex(),
            accent: theme.spec.accent.to_hex(),
            error: theme.spec.error.to_hex(),
        })
        .collect();
    let doc = EncodedDocument {
        format: FORMAT,
        schema_version: SCHEMA_VERSION,
        kind: KIND,
        themes: records,
    };
    let mut bytes = serde_json::to_vec_pretty(&doc).map_err(|_| ConfigError::InvalidPayload)?;
    bytes.push(b'\n');
    if bytes.len() > BYTE_LIMIT {
        return Err(ConfigError::TooLarge);
    }
    Ok(bytes)
}

pub(super) fn load_optional(
    config_dir: &Path,
) -> (Arc<[UserTheme]>, Option<Vec<u8>>, Option<String>) {
    let path = config_dir.join(FILENAME);
    match read_optional(&path, BYTE_LIMIT) {
        Ok(None) => (Arc::from([]), None, None),
        Ok(Some(bytes)) => match decode(&bytes) {
            Ok(themes) => (Arc::from(themes), Some(bytes), None),
            Err(error) => (
                Arc::from([]),
                None,
                Some(format!(
                    "Cannot load {FILENAME}: {}. Using default theme for missing custom themes; fix the file or apply custom themes to replace it.",
                    error.reason()
                )),
            ),
        },
        Err(error) => (
            Arc::from([]),
            None,
            Some(format!(
                "Cannot read {FILENAME} ({}). Using default theme for missing custom themes.",
                error.category()
            )),
        ),
    }
}

pub(super) fn publish(
    config_dir: &Path,
    themes: &[UserTheme],
    baseline: &mut Option<Vec<u8>>,
) -> Result<Arc<[UserTheme]>, StorageFailure> {
    let bytes = encode(themes)
        .map_err(|error| StorageFailure::InvalidConfiguration(error.reason().into()))?;
    if baseline.as_deref() == Some(bytes.as_slice()) {
        return Ok(Arc::from(themes.to_vec()));
    }
    let target = config_dir.join(FILENAME);
    let existing = read_optional(&target, BYTE_LIMIT)?;
    let temporary = Temporary::write(config_dir, &bytes)?;
    if let Some(previous) = existing {
        let backup = config_dir.join(format!("{FILENAME}.bak"));
        if let Ok(meta) = fs::symlink_metadata(&backup)
            && !meta.is_file()
        {
            return Err(StorageFailure::UnsafePath);
        }
        let backup_tmp = Temporary::write(config_dir, &previous)?;
        fs::rename(&backup_tmp.path, &backup)?;
        sync_directory(config_dir)?;
    } else if fs::symlink_metadata(&target).is_ok() {
        return Err(StorageFailure::UnsafePath);
    }
    fs::rename(&temporary.path, &target)?;
    sync_directory(config_dir)?;
    *baseline = Some(bytes);
    Ok(Arc::from(themes.to_vec()))
}
