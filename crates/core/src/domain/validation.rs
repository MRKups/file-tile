//! Centralized text input validation rules for configuration names, filesystem entries, and labels.
//!
//! Configuration identifiers (workspace names and custom theme names) use strict character and
//! length limits separate from filesystem entry names and shortcut tile labels.

pub(crate) const MAX_CONFIG_NAME_CHARS: usize = 32;
const MAX_ENTRY_NAME_BYTES: usize = 255;
const MAX_LAUNCHER_NAME_CHARS: usize = 128;

pub const CONFIG_NAME_ERROR: &str =
    "Use 1 to 32 letters, numbers, single spaces, hyphens, or underscores.";

fn is_config_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '-' || ch == '_'
}

/// Strips disallowed characters, drops leading spaces, collapses consecutive spaces, and clamps
/// to [`MAX_CONFIG_NAME_CHARS`] for live text inputs.
pub fn sanitize_config_name(raw: &str) -> String {
    let mut out = String::new();
    let mut count = 0;
    let mut prev_space = false;
    for ch in raw.chars() {
        if count >= MAX_CONFIG_NAME_CHARS {
            break;
        }
        if ch == ' ' {
            if !out.is_empty() && !prev_space {
                out.push(' ');
                count += 1;
                prev_space = true;
            }
        } else if is_config_word_char(ch) {
            out.push(ch);
            count += 1;
            prev_space = false;
        }
    }
    out
}

pub fn finalize_config_name(raw: &str) -> String {
    sanitize_config_name(raw).trim_end_matches(' ').to_owned()
}

pub(crate) fn validate_config_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() || name.starts_with(' ') || name.ends_with(' ') {
        return Err(CONFIG_NAME_ERROR);
    }
    let mut count = 0;
    let mut prev_space = false;
    for ch in name.chars() {
        count += 1;
        if count > MAX_CONFIG_NAME_CHARS {
            return Err(CONFIG_NAME_ERROR);
        }
        if ch == ' ' {
            if prev_space {
                return Err(CONFIG_NAME_ERROR);
            }
            prev_space = true;
        } else if is_config_word_char(ch) {
            prev_space = false;
        } else {
            return Err(CONFIG_NAME_ERROR);
        }
    }
    Ok(())
}

pub(crate) fn is_valid_config_name(name: &str) -> bool {
    validate_config_name(name).is_ok()
}

pub(super) fn validate_entry_name(name: &str) -> Result<(), &'static str> {
    if name.trim().is_empty()
        || name == "."
        || name == ".."
        || name.len() > MAX_ENTRY_NAME_BYTES
        || name.chars().any(|c| c == '/' || c.is_control())
    {
        Err("Use a filename of 1-255 bytes, without / or control characters.")
    } else {
        Ok(())
    }
}

pub(super) fn validate_launcher_name(name: &str) -> Result<(), &'static str> {
    if name.trim().is_empty()
        || name.chars().count() > MAX_LAUNCHER_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        Err("Use a name of 1-128 characters, without control characters.")
    } else {
        Ok(())
    }
}
