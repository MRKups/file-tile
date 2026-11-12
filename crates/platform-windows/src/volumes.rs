//! Windows volume and cloud-provider discovery stubs.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CloudProvider {
    ICloudDrive,
    ProtonDrive,
    OneDrive,
    Dropbox,
    Box,
    GoogleDrive,
    Nextcloud,
    PCloud,
    OwnCloud,
    SynologyDrive,
    Mega,
    Other,
}

impl CloudProvider {
    pub const fn label(self) -> &'static str {
        match self {
            Self::ICloudDrive => "iCloud Drive",
            Self::ProtonDrive => "Proton Drive",
            Self::OneDrive => "OneDrive",
            Self::Dropbox => "Dropbox",
            Self::Box => "Box",
            Self::GoogleDrive => "Google Drive",
            Self::Nextcloud => "Nextcloud",
            Self::PCloud => "pCloud",
            Self::OwnCloud => "OwnCloud",
            Self::SynologyDrive => "Synology Drive",
            Self::Mega => "MEGA",
            Self::Other => "Cloud Storage",
        }
    }

    pub const fn slug(self) -> &'static str {
        match self {
            Self::ICloudDrive => "icloud",
            Self::ProtonDrive => "proton",
            Self::OneDrive => "onedrive",
            Self::Dropbox => "dropbox",
            Self::Box => "box",
            Self::GoogleDrive => "gdrive",
            Self::Nextcloud => "nextcloud",
            Self::PCloud => "pcloud",
            Self::OwnCloud => "owncloud",
            Self::SynologyDrive => "synology",
            Self::Mega => "mega",
            Self::Other => "cloud",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocationKind {
    Cloud(CloudProvider),
    ExternalDrive,
    NetworkDrive { fstype: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedLocation {
    pub label: String,
    pub path: PathBuf,
    pub kind: LocationKind,
    pub accessible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VolumeMount {
    Local,
    External {
        name: Option<String>,
    },
    Network {
        fstype: String,
        name: Option<String>,
    },
}

pub fn detect_locations(_home: &Path) -> Vec<DetectedLocation> {
    Vec::new()
}

pub fn classify_mount(_path: &Path) -> VolumeMount {
    VolumeMount::Local
}

pub fn is_local_measurable_mount(_path: &Path) -> bool {
    true
}

pub fn location_watch_roots(_home: &Path) -> Vec<PathBuf> {
    Vec::new()
}
