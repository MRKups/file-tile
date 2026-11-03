//! Lossless UTF-8 `file://` URL encoding and percent-decoded `NSURL` validation.
//!
//! Preserves exact UTF-8 path bytes, including decomposed vs precomposed Unicode,
//! rather than normalizing paths through `NSString::path` or `fileURLWithPath:`.

use std::path::{Path, PathBuf};

use objc2::rc::Retained;
use objc2_foundation::{NSString, NSURL, NSURLComponents, NSUTF8StringEncoding};

pub const MAX_PATH_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileUrlError {
    NonUtf8,
    TooLongOrInvalid,
    NonFileUrl,
    NonLocalHost,
    UnresolvableReference,
    MalformedUrl,
    InvalidPercentEncoding,
    InvalidDecodedPath,
}

/// Encodes an absolute UTF-8 path into a `file://` URL string without Unicode normalization.
pub fn encode_file_url_string(path: &Path) -> Result<String, FileUrlError> {
    let value = path.to_str().ok_or(FileUrlError::NonUtf8)?;
    if !path.is_absolute()
        || value.is_empty()
        || value.len() > MAX_PATH_BYTES
        || value.contains('\0')
    {
        return Err(FileUrlError::TooLongOrInvalid);
    }
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len() + 7);
    encoded.push_str("file://");
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[(byte >> 4) as usize]));
            encoded.push(char::from(HEX[(byte & 15) as usize]));
        }
    }
    Ok(encoded)
}

/// Encodes an absolute UTF-8 path into a Foundation `NSURL` preserving exact bytes.
pub fn encode_nsurl(path: &Path) -> Result<Retained<NSURL>, FileUrlError> {
    let encoded = encode_file_url_string(path)?;
    let text = NSString::from_str(&encoded);
    NSURL::URLWithString(&text).ok_or(FileUrlError::MalformedUrl)
}

/// Decodes a percent-encoded POSIX URL path into a validated UTF-8 `PathBuf`.
pub fn decode_percent_path(
    encoded_path: &str,
    strip_trailing_slash: bool,
) -> Result<PathBuf, FileUrlError> {
    if encoded_path.len() > MAX_PATH_BYTES * 3 + 32 {
        return Err(FileUrlError::TooLongOrInvalid);
    }
    let mut raw = Vec::with_capacity(encoded_path.len());
    let mut bytes = encoded_path.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = bytes
                .next()
                .and_then(|value| char::from(value).to_digit(16))
                .ok_or(FileUrlError::InvalidPercentEncoding)?;
            let low = bytes
                .next()
                .and_then(|value| char::from(value).to_digit(16))
                .ok_or(FileUrlError::InvalidPercentEncoding)?;
            raw.push((high * 16 + low) as u8);
        } else {
            raw.push(byte);
        }
    }
    let decoded = String::from_utf8(raw).map_err(|_| FileUrlError::NonUtf8)?;
    if decoded.contains('\0') {
        return Err(FileUrlError::TooLongOrInvalid);
    }
    let normalized = if strip_trailing_slash {
        let trimmed = decoded.trim_end_matches('/');
        if trimmed.is_empty() && decoded.starts_with('/') {
            "/"
        } else {
            trimmed
        }
    } else {
        decoded.as_str()
    };
    if normalized.len() > MAX_PATH_BYTES {
        return Err(FileUrlError::TooLongOrInvalid);
    }
    let path = Path::new(normalized);
    if !path.is_absolute() || path.as_os_str().is_empty() {
        return Err(FileUrlError::InvalidDecodedPath);
    }
    Ok(path.to_path_buf())
}

/// Validates and percent-decodes a local `file://` `NSURL` into exact UTF-8 path bytes.
pub fn decode_nsurl(url: &NSURL, strip_trailing_slash: bool) -> Result<PathBuf, FileUrlError> {
    if !url.isFileURL() {
        return Err(FileUrlError::NonFileUrl);
    }
    if url
        .host()
        .is_some_and(|host| !host.is_empty() && host.to_string() != "localhost")
    {
        return Err(FileUrlError::NonLocalHost);
    }
    let value = if url.isFileReferenceURL() {
        url.filePathURL()
            .and_then(|resolved| resolved.absoluteString())
            .ok_or(FileUrlError::UnresolvableReference)?
    } else {
        url.absoluteString().ok_or(FileUrlError::MalformedUrl)?
    };
    if !value.canBeConvertedToEncoding(NSUTF8StringEncoding) {
        return Err(FileUrlError::NonUtf8);
    }
    if value.len_utf16() > MAX_PATH_BYTES * 3 + 32 {
        return Err(FileUrlError::TooLongOrInvalid);
    }
    let components =
        NSURLComponents::componentsWithString(&value).ok_or(FileUrlError::MalformedUrl)?;
    let encoded = components
        .percentEncodedPath()
        .ok_or(FileUrlError::MalformedUrl)?
        .to_string();
    decode_percent_path(&encoded, strip_trailing_slash)
}
