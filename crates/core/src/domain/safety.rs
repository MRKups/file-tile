//! Pure path-safety rules and per-category protection policies, without filesystem or UI state.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

pub const MAX_DISABLED_LOCATIONS: usize = 128;

const SYSTEM_SUBTREES: [&str; 12] = [
    "/System", "/Library", "/bin", "/sbin", "/usr", "/var", "/private", "/etc", "/tmp", "/dev",
    "/cores", "/opt",
];

const HOME_SYSTEM_SUBTREES: [&str; 2] = ["Library", ".Trash"];

const ROOT_CONTAINERS: [&str; 4] = ["/", "/Users", "/Volumes", "/Network"];

const USER_TOP_FOLDERS: [&str; 7] = [
    "Desktop",
    "Documents",
    "Downloads",
    "Music",
    "Pictures",
    "Movies",
    "Public",
];

const CREDENTIAL_PATHS: [&str; 9] = [
    ".ssh", ".gnupg", ".aws", ".kube", ".docker", ".config", ".netrc", ".npmrc", ".pypirc",
];

const HOME_FULL_DISK_SUBTREES: [&str; 12] = [
    "Library/Mail",
    "Library/Messages",
    "Library/Safari",
    "Library/Cookies",
    "Library/HomeKit",
    "Library/IdentityServices",
    "Library/Metadata/CoreSpotlight",
    "Library/PersonalizationPortrait",
    "Library/Suggestions",
    "Library/Containers",
    "Library/Group Containers",
    "Library/Application Support/com.apple.TCC",
];

const SYSTEM_FULL_DISK_SUBTREES: [&str; 2] = [
    "/Library/Application Support/com.apple.TCC",
    "/private/var/db",
];

const SHARED_USAGE_EXCLUSIONS: [&str; 29] = [
    ".Trash",
    "OneDrive*",
    "One Drive*",
    "CloudStorage",
    "Dropbox*",
    "Box",
    "Box Sync",
    "Box-*",
    "Box *",
    "Box_*",
    "Box(*",
    "Proton Drive*",
    "ProtonDrive*",
    "Google Drive*",
    "GoogleDrive*",
    "Nextcloud*",
    "nextcloud*",
    "pCloud*",
    "OwnCloud*",
    "ownCloud*",
    "SynologyDrive*",
    "Synology Drive*",
    "MEGA*",
    "Mega*",
    "mega*",
    "*.musiclibrary",
    "*.photoslibrary",
    "*.tvlibrary",
    "Media.localized",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProtectedAccess {
    Desktop,
    Documents,
    Downloads,
    ExternalDrives,
    NetworkShares,
    FullDisk,
}

impl ProtectedAccess {
    pub const ALL: [Self; 6] = [
        Self::Desktop,
        Self::Documents,
        Self::Downloads,
        Self::ExternalDrives,
        Self::NetworkShares,
        Self::FullDisk,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Desktop => "Desktop",
            Self::Documents => "Documents",
            Self::Downloads => "Downloads",
            Self::ExternalDrives => "External Drives",
            Self::NetworkShares => "Network Shares",
            Self::FullDisk => "Full Disk Access",
        }
    }

    pub const fn home_folder_name(self) -> Option<&'static str> {
        match self {
            Self::Desktop => Some("Desktop"),
            Self::Documents => Some("Documents"),
            Self::Downloads => Some("Downloads"),
            Self::ExternalDrives | Self::NetworkShares | Self::FullDisk => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafetyPreferences {
    pub desktop_access: bool,
    pub documents_access: bool,
    pub downloads_access: bool,
    pub full_disk_access: bool,
    pub external_drives_access: bool,
    pub network_shares_access: bool,
    pub disabled_locations: BTreeSet<PathBuf>,
    pub system_folders: bool,
    pub applications: bool,
    pub credentials: bool,
}

impl Default for SafetyPreferences {
    fn default() -> Self {
        Self {
            desktop_access: true,
            documents_access: true,
            downloads_access: true,
            full_disk_access: false,
            external_drives_access: true,
            network_shares_access: true,
            disabled_locations: BTreeSet::new(),
            system_folders: false,
            applications: false,
            credentials: false,
        }
    }
}

impl SafetyPreferences {
    pub const fn access_enabled(&self, target: ProtectedAccess) -> bool {
        match target {
            ProtectedAccess::Desktop => self.desktop_access,
            ProtectedAccess::Documents => self.documents_access,
            ProtectedAccess::Downloads => self.downloads_access,
            ProtectedAccess::ExternalDrives => self.external_drives_access,
            ProtectedAccess::NetworkShares => self.network_shares_access,
            ProtectedAccess::FullDisk => self.full_disk_access,
        }
    }

    pub fn with_access(mut self, target: ProtectedAccess, enabled: bool) -> Self {
        match target {
            ProtectedAccess::Desktop => self.desktop_access = enabled,
            ProtectedAccess::Documents => self.documents_access = enabled,
            ProtectedAccess::Downloads => self.downloads_access = enabled,
            ProtectedAccess::ExternalDrives => self.external_drives_access = enabled,
            ProtectedAccess::NetworkShares => self.network_shares_access = enabled,
            ProtectedAccess::FullDisk => self.full_disk_access = enabled,
        }
        self
    }

    pub fn is_location_enabled(&self, path: &Path) -> bool {
        !self.disabled_locations.contains(path)
    }

    pub fn with_location_access(mut self, path: PathBuf, enabled: bool) -> Self {
        if enabled {
            self.disabled_locations.remove(&path);
        } else if self.disabled_locations.contains(&path)
            || self.disabled_locations.len() < MAX_DISABLED_LOCATIONS
        {
            self.disabled_locations.insert(path);
        }
        self
    }

    pub fn excluded_home_usage_folders(&self) -> Vec<&'static str> {
        let mut excluded = Vec::with_capacity(4 + SHARED_USAGE_EXCLUSIONS.len() + 3);
        excluded.extend_from_slice(&["Library", "Pictures", "Music", "Movies"]);
        excluded.extend_from_slice(&SHARED_USAGE_EXCLUSIONS);
        if !self.desktop_access {
            excluded.push("Desktop");
        }
        if !self.documents_access {
            excluded.push("Documents");
        }
        if !self.downloads_access {
            excluded.push("Downloads");
        }
        excluded
    }

    pub fn excluded_usage_folders(&self, directory: &Path, home: &Path) -> Vec<&'static str> {
        if has_usable_home(home) && (directory == home || home.starts_with(directory)) {
            self.excluded_home_usage_folders()
        } else {
            SHARED_USAGE_EXCLUSIONS.to_vec()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafetyPolicy {
    pub preferences: SafetyPreferences,
    pub home: PathBuf,
    pub state_dir: Option<PathBuf>,
    pub prohibit_delete: bool,
    pub external_drive_roots: Vec<PathBuf>,
    pub network_share_roots: Vec<PathBuf>,
}

impl SafetyPolicy {
    pub fn check_read_directory(&self, directory: &Path) -> Result<(), &'static str> {
        check_read_directory(
            directory,
            &self.home,
            &self.preferences,
            &self.external_drive_roots,
            &self.network_share_roots,
        )
    }

    pub fn can_measure_directory(&self, directory: &Path) -> bool {
        if self.check_read_directory(directory).is_err() || is_root_container(directory) {
            return false;
        }
        if has_usable_home(&self.home) {
            for name in ["Library", ".Trash", "Music", "Pictures", "Movies"] {
                if directory.starts_with(self.home.join(name)) {
                    return false;
                }
            }
            if is_in_direct_home_cloud_folder(directory, &self.home) {
                return false;
            }
        }
        true
    }

    pub fn check_write_directory(&self, directory: &Path) -> Result<(), &'static str> {
        check_write_directory(
            directory,
            &self.home,
            self.state_dir.as_deref(),
            &self.preferences,
            &self.external_drive_roots,
            &self.network_share_roots,
        )
    }

    pub fn check_entry_modification(&self, target: &Path) -> Result<(), &'static str> {
        check_entry_modification(
            target,
            &self.home,
            self.state_dir.as_deref(),
            &self.preferences,
            &self.external_drive_roots,
            &self.network_share_roots,
        )
    }

    pub fn check_trash(&self, target: &Path) -> Result<(), &'static str> {
        check_trash(
            target,
            self.prohibit_delete,
            &self.home,
            self.state_dir.as_deref(),
            &self.preferences,
            &self.external_drive_roots,
            &self.network_share_roots,
        )
    }
}

pub(crate) fn check_read_directory(
    directory: &Path,
    home: &Path,
    safety: &SafetyPreferences,
    external_drive_roots: &[PathBuf],
    network_share_roots: &[PathBuf],
) -> Result<(), &'static str> {
    if has_usable_home(home) {
        if !safety.desktop_access && directory.starts_with(home.join("Desktop")) {
            return Err("Desktop access is turned off in Access and Safety settings.");
        }
        if !safety.documents_access && directory.starts_with(home.join("Documents")) {
            return Err("Documents access is turned off in Access and Safety settings.");
        }
        if !safety.downloads_access && directory.starts_with(home.join("Downloads")) {
            return Err("Downloads access is turned off in Access and Safety settings.");
        }
    }
    if !safety.full_disk_access && is_in_full_disk_subtree(directory, home) {
        return Err("Full Disk Access is turned off in Access and Safety settings.");
    }
    if !safety.external_drives_access
        && external_drive_roots
            .iter()
            .any(|root| is_non_empty(root) && directory.starts_with(root))
    {
        return Err("External drive access is turned off in Access and Safety settings.");
    }
    if !safety.network_shares_access
        && network_share_roots
            .iter()
            .any(|root| is_non_empty(root) && directory.starts_with(root))
    {
        return Err("Network share access is turned off in Access and Safety settings.");
    }
    if safety
        .disabled_locations
        .iter()
        .any(|root| is_non_empty(root) && directory.starts_with(root))
    {
        return Err(
            "Cloud storage access for this location is turned off in Access and Safety settings.",
        );
    }
    Ok(())
}

fn check_write_directory(
    directory: &Path,
    home: &Path,
    state_dir: Option<&Path>,
    safety: &SafetyPreferences,
    external_drive_roots: &[PathBuf],
    network_share_roots: &[PathBuf],
) -> Result<(), &'static str> {
    if state_dir.is_some_and(|state| is_non_empty(state) && directory.starts_with(state)) {
        return Err("Cannot modify file-tile's active state folder. Choose another folder.");
    }
    if !safety.system_folders
        && (is_in_system_subtree(directory, home) || is_root_container(directory))
    {
        return Err(
            "System folder protection blocks changes in this folder. You can change this in Application Settings.",
        );
    }
    if !safety.applications && is_in_applications_subtree(directory, home) {
        return Err(
            "Application protection blocks changes in Applications. You can change this in Application Settings.",
        );
    }
    if !safety.credentials && is_in_credentials_path(directory, home) {
        return Err(
            "Credential protection blocks changes in this folder. You can change this in Application Settings.",
        );
    }
    check_read_directory(
        directory,
        home,
        safety,
        external_drive_roots,
        network_share_roots,
    )?;
    Ok(())
}

fn check_entry_modification(
    target: &Path,
    home: &Path,
    state_dir: Option<&Path>,
    safety: &SafetyPreferences,
    external_drive_roots: &[PathBuf],
    network_share_roots: &[PathBuf],
) -> Result<(), &'static str> {
    let Some(parent) = target.parent() else {
        return Err("Cannot modify a filesystem root. Choose an item inside a folder.");
    };
    if is_state_dir_affected(target, parent, state_dir) {
        return Err("Cannot modify or move file-tile's active state folder.");
    }
    if !safety.system_folders && is_system_entry_protected(target, parent, home) {
        return Err(
            "System folder protection blocks moving or renaming this item. You can change this in Application Settings.",
        );
    }
    if !safety.applications && is_applications_entry_protected(target, parent, home) {
        return Err(
            "Application protection blocks moving or renaming items in Applications. You can change this in Application Settings.",
        );
    }
    if !safety.credentials && is_credentials_entry_protected(target, parent, home) {
        return Err(
            "Credential protection blocks moving or renaming this item. You can change this in Application Settings.",
        );
    }
    check_read_directory(
        target,
        home,
        safety,
        external_drive_roots,
        network_share_roots,
    )?;
    check_read_directory(
        parent,
        home,
        safety,
        external_drive_roots,
        network_share_roots,
    )?;
    Ok(())
}

fn check_trash(
    target: &Path,
    prohibit_delete: bool,
    home: &Path,
    state_dir: Option<&Path>,
    safety: &SafetyPreferences,
    external_drive_roots: &[PathBuf],
    network_share_roots: &[PathBuf],
) -> Result<(), &'static str> {
    if prohibit_delete {
        return Err(
            "Deletion is prohibited in this box. Turn off Prohibit deletion in Configure to move items to Trash.",
        );
    }
    let Some(parent) = target.parent() else {
        return Err("Cannot move a filesystem root to Trash.");
    };
    if is_state_dir_affected(target, parent, state_dir) {
        return Err("Cannot move file-tile's active state folder to Trash.");
    }
    if !safety.system_folders && is_system_entry_protected(target, parent, home) {
        return Err(
            "System folder protection blocks moving this item to Trash. You can change this in Application Settings.",
        );
    }
    if !safety.applications && is_applications_entry_protected(target, parent, home) {
        return Err(
            "Application protection blocks moving items in Applications to Trash. You can change this in Application Settings.",
        );
    }
    if !safety.credentials && is_credentials_entry_protected(target, parent, home) {
        return Err(
            "Credential protection blocks moving this item to Trash. You can change this in Application Settings.",
        );
    }
    check_read_directory(
        target,
        home,
        safety,
        external_drive_roots,
        network_share_roots,
    )?;
    check_read_directory(
        parent,
        home,
        safety,
        external_drive_roots,
        network_share_roots,
    )?;
    Ok(())
}

fn is_non_empty(path: &Path) -> bool {
    path.is_absolute() && !path.as_os_str().is_empty()
}

fn has_usable_home(home: &Path) -> bool {
    is_non_empty(home) && home != Path::new("/")
}

fn is_state_dir_affected(target: &Path, parent: &Path, state_dir: Option<&Path>) -> bool {
    state_dir.is_some_and(|state| {
        is_non_empty(state)
            && (target.starts_with(state) || parent.starts_with(state) || state.starts_with(target))
    })
}

fn is_root_container(path: &Path) -> bool {
    ROOT_CONTAINERS
        .iter()
        .any(|container| path == Path::new(container))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CloudWorkspaceLevel {
    ProviderRoot,
    Interior,
}

fn has_dot_components(path: &Path) -> bool {
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return true;
    }
    path.as_os_str()
        .as_encoded_bytes()
        .split(|&byte| byte == b'/' || (cfg!(windows) && byte == b'\\'))
        .any(|segment| segment == b"." || segment == b"..")
}

fn normal_segment(component: Component<'_>) -> Option<&OsStr> {
    match component {
        Component::Normal(segment) if !segment.is_empty() => Some(segment),
        Component::Normal(_)
        | Component::Prefix(_)
        | Component::RootDir
        | Component::CurDir
        | Component::ParentDir => None,
    }
}

fn is_cloud_storage_domain(segment: &OsStr) -> bool {
    let bytes = segment.as_encoded_bytes();
    !bytes.is_empty() && !bytes.starts_with(b".")
}

fn cloud_workspace_level(path: &Path, home: &Path) -> Option<CloudWorkspaceLevel> {
    if !has_usable_home(home) || has_dot_components(home) || has_dot_components(path) {
        return None;
    }
    let relative = path.strip_prefix(home).ok()?;
    let mut components = relative.components();
    if normal_segment(components.next()?)? != "Library" {
        return None;
    }
    let second = normal_segment(components.next()?)?;
    let third = normal_segment(components.next()?)?;
    let valid_provider = if second == "Mobile Documents" {
        third == "com~apple~CloudDocs"
    } else if second == "CloudStorage" {
        is_cloud_storage_domain(third)
    } else {
        false
    };
    if !valid_provider {
        return None;
    }
    let mut has_interior = false;
    for component in components {
        normal_segment(component)?;
        has_interior = true;
    }
    Some(if has_interior {
        CloudWorkspaceLevel::Interior
    } else {
        CloudWorkspaceLevel::ProviderRoot
    })
}

fn is_direct_home_cloud_name(name: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "proton drive",
        "protondrive",
        "google drive",
        "googledrive",
        "synology drive",
        "synologydrive",
        "pcloud drive",
        "pcloud",
        "box sync",
        "onedrive",
        "dropbox",
        "nextcloud",
        "owncloud",
        "mega",
        "box",
    ];
    PREFIXES.iter().any(|prefix| {
        name.len() >= prefix.len()
            && name.is_char_boundary(prefix.len())
            && name[..prefix.len()].eq_ignore_ascii_case(prefix)
            && {
                let rest = &name[prefix.len()..];
                rest.is_empty() || rest.starts_with([' ', '-', '_', '('])
            }
    })
}

fn is_in_direct_home_cloud_folder(path: &Path, home: &Path) -> bool {
    if !has_usable_home(home) {
        return false;
    }
    let Ok(relative) = path.strip_prefix(home) else {
        return false;
    };
    let Some(Component::Normal(first)) = relative.components().next() else {
        return false;
    };
    is_direct_home_cloud_name(&first.to_string_lossy())
}

fn is_direct_home_cloud_root(target: &Path, home: &Path) -> bool {
    if !has_usable_home(home) || has_dot_components(home) {
        return false;
    }
    let Ok(relative) = target.strip_prefix(home) else {
        return false;
    };
    let mut components = relative.components();
    let Some(first) = components.next().and_then(normal_segment) else {
        return false;
    };
    if !is_direct_home_cloud_name(&first.to_string_lossy()) {
        return false;
    }
    has_dot_components(target) || components.next().is_none()
}

fn is_in_system_subtree(path: &Path, home: &Path) -> bool {
    if SYSTEM_SUBTREES
        .iter()
        .any(|root| path.starts_with(Path::new(root)))
    {
        return true;
    }
    if !has_usable_home(home) || cloud_workspace_level(path, home).is_some() {
        return false;
    }
    HOME_SYSTEM_SUBTREES
        .iter()
        .any(|name| path.starts_with(home.join(name)))
}

fn is_system_entry_protected(target: &Path, parent: &Path, home: &Path) -> bool {
    if is_in_system_subtree(target, home)
        || is_in_system_subtree(parent, home)
        || is_root_container(target)
        || is_root_container(parent)
    {
        return true;
    }
    if SYSTEM_SUBTREES
        .iter()
        .any(|root| Path::new(root).starts_with(target))
    {
        return true;
    }
    if !has_usable_home(home) {
        return false;
    }
    if home.starts_with(target)
        || HOME_SYSTEM_SUBTREES
            .iter()
            .any(|name| home.join(name).starts_with(target))
        || matches!(
            cloud_workspace_level(target, home),
            Some(CloudWorkspaceLevel::ProviderRoot)
        )
        || is_direct_home_cloud_root(target, home)
    {
        return true;
    }
    USER_TOP_FOLDERS
        .iter()
        .any(|name| target == home.join(name))
}

fn is_in_applications_subtree(path: &Path, home: &Path) -> bool {
    path.starts_with(Path::new("/Applications"))
        || (has_usable_home(home) && path.starts_with(home.join("Applications")))
}

fn is_applications_entry_protected(target: &Path, parent: &Path, home: &Path) -> bool {
    if is_in_applications_subtree(target, home)
        || is_in_applications_subtree(parent, home)
        || Path::new("/Applications").starts_with(target)
    {
        return true;
    }
    has_usable_home(home) && home.join("Applications").starts_with(target)
}

fn is_in_credentials_path(path: &Path, home: &Path) -> bool {
    has_usable_home(home)
        && CREDENTIAL_PATHS
            .iter()
            .any(|name| path.starts_with(home.join(name)))
}

fn is_credentials_entry_protected(target: &Path, parent: &Path, home: &Path) -> bool {
    if is_in_credentials_path(target, home) || is_in_credentials_path(parent, home) {
        return true;
    }
    has_usable_home(home)
        && CREDENTIAL_PATHS
            .iter()
            .any(|name| home.join(name).starts_with(target))
}

fn is_in_full_disk_subtree(path: &Path, home: &Path) -> bool {
    if SYSTEM_FULL_DISK_SUBTREES
        .iter()
        .any(|root| path.starts_with(Path::new(root)))
    {
        return true;
    }
    has_usable_home(home)
        && HOME_FULL_DISK_SUBTREES
            .iter()
            .any(|subpath| path.starts_with(home.join(subpath)))
}
