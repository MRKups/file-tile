//! Explicit macOS default-handler opening, without a shell or launch arguments.
use std::path::Path;

use super::{OpenDiag, OpenError, opening::handoff};

pub fn open_path(path: &Path, is_cancelled: impl Fn() -> bool) -> Result<(), OpenError> {
    objc2::rc::autoreleasepool(|_| {
        if is_cancelled() {
            return Err(OpenError {
                message: "Shortcut opening cancelled before handoff.".into(),
                diagnostic: OpenDiag::Cancelled,
            });
        }
        let info = std::fs::metadata(path).map_err(|error| {
            OpenError::io(
                &error,
                format!(
                    "Cannot open shortcut target: {error}. Check its Target in Configure and retry."
                ),
            )
        })?;
        if !info.is_file() && !info.is_dir() {
            return Err(
                "Cannot open this shortcut target: choose an application, file, or folder.".into(),
            );
        }
        handoff(path.as_os_str(), &is_cancelled)
    })
}

pub fn open_link(link: &str, is_cancelled: impl Fn() -> bool) -> Result<(), OpenError> {
    objc2::rc::autoreleasepool(|_| {
        if is_cancelled() {
            return Err(OpenError {
                message: "Shortcut opening cancelled before handoff.".into(),
                diagnostic: OpenDiag::Cancelled,
            });
        }
        let native = objc2_foundation::NSString::from_str(link);
        if objc2_foundation::NSURL::URLWithString(&native).is_none() {
            return Err("Cannot open this link. Correct its Target in Configure and retry.".into());
        }
        handoff(std::ffi::OsStr::new(link), &is_cancelled)
    })
}
