use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, ser::SerializeStruct};

pub(super) use crate::application::contracts::configuration::{
    MAX_PATH_BYTES, validate_saved_path as validate,
};

#[derive(Deserialize)]
#[serde(
    tag = "encoding",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(super) enum PathRecord {
    Utf8(String),
    UnixBytes(Vec<u8>),
}

impl PathRecord {
    pub fn into_path(self) -> Result<PathBuf, &'static str> {
        let path = match self {
            Self::Utf8(value) => {
                if value.len() > MAX_PATH_BYTES || value.contains('\0') {
                    return Err("path exceeds its byte limit or contains a null byte");
                }
                PathBuf::from(value)
            }
            Self::UnixBytes(value) => unix_path(value)?,
        };
        validate(&path)?;
        Ok(path)
    }
}

#[cfg(unix)]
fn unix_path(value: Vec<u8>) -> Result<PathBuf, &'static str> {
    use std::os::unix::ffi::OsStringExt;
    if value.len() > MAX_PATH_BYTES || value.contains(&0) {
        return Err("path exceeds its byte limit or contains a null byte");
    }
    Ok(std::ffi::OsString::from_vec(value).into())
}

#[cfg(not(unix))]
fn unix_path(_value: Vec<u8>) -> Result<PathBuf, &'static str> {
    Err("Unix byte paths are unsupported on this platform")
}

pub(super) struct PathRef<'a>(pub &'a Path);

impl Serialize for PathRef<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        validate(self.0).map_err(serde::ser::Error::custom)?;
        let mut record = serializer.serialize_struct("Path", 2)?;
        if let Some(value) = self.0.to_str() {
            record.serialize_field("encoding", "utf8")?;
            record.serialize_field("value", value)?;
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStrExt;
                record.serialize_field("encoding", "unix_bytes")?;
                record.serialize_field("value", self.0.as_os_str().as_bytes())?;
            }
            #[cfg(not(unix))]
            return Err(serde::ser::Error::custom(
                "This platform's opaque paths are unsupported",
            ));
        }
        record.end()
    }
}
