//! macOS TCC folder-access probing and System Settings Privacy deep links.

use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::process::{Command, Stdio};

use rustix::fs::{Access, access};
use rustix::io::Errno;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionStatus {
    Granted,
    Denied,
    Unavailable,
}

pub fn check_full_disk_access(home: &Path) -> PermissionStatus {
    let system_tcc = Path::new("/Library/Application Support/com.apple.TCC");
    if fs::read_dir(system_tcc).is_ok() {
        return PermissionStatus::Granted;
    }
    let user_safari = home.join("Library/Safari");
    if user_safari.exists() && fs::read_dir(&user_safari).is_ok() {
        return PermissionStatus::Granted;
    }
    let user_tcc = home.join("Library/Application Support/com.apple.TCC");
    if user_tcc.exists() && fs::read_dir(&user_tcc).is_ok() {
        return PermissionStatus::Granted;
    }
    PermissionStatus::Denied
}

pub fn check_path_access(path: &Path) -> PermissionStatus {
    match fs::metadata(path) {
        Ok(meta) if !meta.is_dir() => return PermissionStatus::Unavailable,
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::PermissionDenied => {
            return PermissionStatus::Denied;
        }
        Err(_) => return PermissionStatus::Unavailable,
    }
    match access(path, Access::READ_OK) {
        Ok(()) => PermissionStatus::Granted,
        Err(Errno::ACCESS | Errno::PERM) => PermissionStatus::Denied,
        Err(_) => PermissionStatus::Unavailable,
    }
}

pub fn check_folder_access(folder_name: Option<&str>, home: &Path) -> PermissionStatus {
    match folder_name {
        None => check_full_disk_access(home),
        Some(folder_name) => check_path_access(&home.join(folder_name)),
    }
}

pub fn request_path_access(path: &Path) -> PermissionStatus {
    match fs::metadata(path) {
        Ok(meta) if !meta.is_dir() => return PermissionStatus::Unavailable,
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::PermissionDenied => {
            return PermissionStatus::Denied;
        }
        Err(_) => return PermissionStatus::Unavailable,
    }
    match fs::read_dir(path) {
        Ok(mut entries) => {
            let _ = entries.next();
            PermissionStatus::Granted
        }
        Err(error) if error.kind() == ErrorKind::PermissionDenied => PermissionStatus::Denied,
        Err(_) => PermissionStatus::Unavailable,
    }
}

pub fn request_folder_access(folder_name: Option<&str>, home: &Path) -> PermissionStatus {
    match folder_name {
        None => {
            let status = check_full_disk_access(home);
            if status != PermissionStatus::Granted {
                let _ = open_privacy_settings(true);
            }
            status
        }
        Some(folder_name) => request_path_access(&home.join(folder_name)),
    }
}

pub fn open_privacy_settings(full_disk: bool) -> Result<(), String> {
    let url = if full_disk {
        "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles"
    } else {
        "x-apple.systempreferences:com.apple.preference.security?Privacy_FilesAndFolders"
    };
    let status = Command::new("/usr/bin/open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| "Cannot open macOS System Settings.".to_owned())?;
    if status.success() {
        Ok(())
    } else {
        Err("macOS System Settings returned an error.".to_owned())
    }
}
