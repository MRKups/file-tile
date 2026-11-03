//! Exact UTF-8 file-URL encoding shared by clipboard transport and native copying.
use std::path::Path;

use file_tile_platform_macos_appkit::file_url::{FileUrlError, encode_file_url_string};
use objc2::rc::Retained;
use objc2_foundation::{NSString, NSURL};

pub(super) use file_tile_platform_macos_appkit::file_url::MAX_PATH_BYTES;

pub(super) fn encode(path: &Path) -> Result<Retained<NSString>, String> {
    match encode_file_url_string(path) {
        Ok(encoded) => Ok(NSString::from_str(&encoded)),
        Err(FileUrlError::NonUtf8) => {
            Err("Cannot represent this native path exactly. Use Finder for this entry.".into())
        }
        Err(_) => {
            if !path.is_absolute() {
                Err("Cannot represent this native path exactly. Use Finder for this entry.".into())
            } else {
                Err("Cannot encode this file path within the supported limits. Use Finder.".into())
            }
        }
    }
}

pub(super) fn encode_url(path: &Path, err_msg: &str) -> Result<Retained<NSURL>, String> {
    let encoded = encode(path)?;
    NSURL::URLWithString(&encoded).ok_or_else(|| err_msg.to_owned())
}
