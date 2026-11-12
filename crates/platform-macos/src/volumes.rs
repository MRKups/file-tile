//! macOS cloud-provider folder, external drive, and network mount discovery.

use std::collections::{HashMap, HashSet};
use std::ffi::c_char;
use std::fs;
use std::path::{Path, PathBuf};

const MNT_LOCAL: u32 = 0x0000_1000;
const MNT_ROOTFS: u32 = 0x0000_4000;
const MNT_DONTBROWSE: u32 = 0x0010_0000;

const NETWORK_FSTYPES: &[&str] = &["smbfs", "nfs", "afpfs", "webdav", "cifs", "ftp"];
const VIRTUAL_FSTYPES: &[&str] = &["osxfuse", "macfuse", "fusefs", "fskit"];

const KNOWN_CLOUD_PREFIXES: &[(&str, CloudProvider)] = &[
    ("proton drive", CloudProvider::ProtonDrive),
    ("protondrive", CloudProvider::ProtonDrive),
    ("google drive", CloudProvider::GoogleDrive),
    ("googledrive", CloudProvider::GoogleDrive),
    ("synology drive", CloudProvider::SynologyDrive),
    ("synologydrive", CloudProvider::SynologyDrive),
    ("pcloud drive", CloudProvider::PCloud),
    ("pcloud", CloudProvider::PCloud),
    ("box sync", CloudProvider::Box),
    ("onedrive", CloudProvider::OneDrive),
    ("dropbox", CloudProvider::Dropbox),
    ("nextcloud", CloudProvider::Nextcloud),
    ("owncloud", CloudProvider::OwnCloud),
    ("mega", CloudProvider::Mega),
    ("box", CloudProvider::Box),
];

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

struct CloudCandidate {
    provider: CloudProvider,
    raw_name: String,
    suffix: Option<String>,
    path: PathBuf,
}

pub fn detect_locations(home: &Path) -> Vec<DetectedLocation> {
    let mut locations = detect_cloud_locations(home);
    let seen_paths: HashSet<PathBuf> = locations.iter().map(|loc| loc.path.clone()).collect();
    locations.extend(
        detect_mounted_volumes()
            .into_iter()
            .filter(|vol| !seen_paths.contains(&vol.path)),
    );
    locations.sort_by(|a, b| {
        kind_rank(&a.kind)
            .cmp(&kind_rank(&b.kind))
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
            .then_with(|| a.label.cmp(&b.label))
            .then_with(|| a.path.cmp(&b.path))
    });
    locations
}

pub fn classify_mount(path: &Path) -> VolumeMount {
    let Ok(stat) = rustix::fs::statfs(path) else {
        return VolumeMount::Local;
    };
    let mntonname = c_chars_to_string(&stat.f_mntonname);
    let fstype = c_chars_to_string(&stat.f_fstypename).to_ascii_lowercase();
    if is_network_mount(stat.f_flags, &fstype) {
        return VolumeMount::Network {
            fstype,
            name: mount_volume_name(&mntonname),
        };
    }
    if (stat.f_flags & MNT_ROOTFS) == 0
        && (stat.f_flags & MNT_DONTBROWSE) == 0
        && mntonname.starts_with("/Volumes/")
        && Path::new(&mntonname) != Path::new("/Volumes")
    {
        return VolumeMount::External {
            name: mount_volume_name(&mntonname),
        };
    }
    VolumeMount::Local
}

pub fn is_local_measurable_mount(path: &Path) -> bool {
    let Ok(stat) = rustix::fs::statfs(path) else {
        return false;
    };
    if (stat.f_flags & MNT_LOCAL) == 0 {
        return false;
    }
    let fstype = c_chars_to_string(&stat.f_fstypename).to_ascii_lowercase();
    !NETWORK_FSTYPES.contains(&fstype.as_str()) && !VIRTUAL_FSTYPES.contains(&fstype.as_str())
}

pub fn location_watch_roots(home: &Path) -> Vec<PathBuf> {
    vec![
        PathBuf::from("/Volumes"),
        home.join("Library/CloudStorage"),
        home.join("Library/Mobile Documents"),
        home.to_path_buf(),
    ]
}

fn detect_cloud_locations(home: &Path) -> Vec<DetectedLocation> {
    let mut seen_canonical = HashSet::new();
    let mut candidates = Vec::new();

    let mobile_docs = home.join("Library/Mobile Documents");
    let icloud = mobile_docs.join("com~apple~CloudDocs");
    let icloud_present = match fs::metadata(&icloud) {
        Ok(meta) => meta.is_dir(),
        Err(error) => {
            error.kind() == std::io::ErrorKind::PermissionDenied
                && fs::metadata(&mobile_docs).is_ok_and(|meta| meta.is_dir())
        }
    };
    if icloud_present {
        let canonical = canonical_or_self(&icloud);
        if seen_canonical.insert(canonical.clone()) {
            candidates.push(CloudCandidate {
                provider: CloudProvider::ICloudDrive,
                raw_name: CloudProvider::ICloudDrive.label().to_owned(),
                suffix: None,
                path: canonical,
            });
        }
    }

    collect_cloud_storage_domains(
        &home.join("Library/CloudStorage"),
        &mut seen_canonical,
        &mut candidates,
    );
    collect_home_cloud_folders(home, &mut seen_canonical, &mut candidates);

    let mut counts: HashMap<CloudProvider, usize> = HashMap::new();
    for candidate in &candidates {
        *counts.entry(candidate.provider).or_insert(0) += 1;
    }

    candidates
        .into_iter()
        .map(|candidate| {
            let count = counts.get(&candidate.provider).copied().unwrap_or(0);
            let label = format_cloud_label(&candidate, count);
            let accessible = crate::permissions::check_path_access(&candidate.path)
                != crate::permissions::PermissionStatus::Denied;
            DetectedLocation {
                label,
                path: candidate.path,
                kind: LocationKind::Cloud(candidate.provider),
                accessible,
            }
        })
        .collect()
}

fn collect_cloud_storage_domains(
    cloud_storage: &Path,
    seen_canonical: &mut HashSet<PathBuf>,
    candidates: &mut Vec<CloudCandidate>,
) {
    for (name, path) in sorted_visible_subdirs(cloud_storage) {
        let canonical = canonical_or_self(&path);
        if !seen_canonical.insert(canonical.clone()) {
            continue;
        }
        let (provider, suffix) = match match_known_cloud(&name, false) {
            Some(matched) => matched,
            None => (CloudProvider::Other, None),
        };
        candidates.push(CloudCandidate {
            provider,
            raw_name: name,
            suffix,
            path: canonical,
        });
    }
}

fn collect_home_cloud_folders(
    home: &Path,
    seen_canonical: &mut HashSet<PathBuf>,
    candidates: &mut Vec<CloudCandidate>,
) {
    let Ok(read_dir) = fs::read_dir(home) else {
        return;
    };
    let mut entries: Vec<(String, PathBuf, CloudProvider, Option<String>)> = read_dir
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.is_empty() || name.starts_with('.') {
                return None;
            }
            let (provider, suffix) = match_known_cloud(&name, true)?;
            let path = entry.path();
            is_dir_or_restricted(&entry, &path).then_some((name, path, provider, suffix))
        })
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    for (name, path, provider, suffix) in entries {
        let canonical = canonical_or_self(&path);
        if seen_canonical.insert(canonical.clone()) {
            candidates.push(CloudCandidate {
                provider,
                raw_name: name,
                suffix,
                path: canonical,
            });
        }
    }
}

fn format_cloud_label(candidate: &CloudCandidate, provider_count: usize) -> String {
    if candidate.provider == CloudProvider::Other {
        return candidate.raw_name.clone();
    }
    if provider_count > 1
        && let Some(suffix) = &candidate.suffix
    {
        return format!("{} ({suffix})", candidate.provider.label());
    }
    candidate.provider.label().to_owned()
}

fn match_known_cloud(
    name: &str,
    require_boundary: bool,
) -> Option<(CloudProvider, Option<String>)> {
    for &(prefix, provider) in KNOWN_CLOUD_PREFIXES {
        if name.len() < prefix.len() || !name.is_char_boundary(prefix.len()) {
            continue;
        }
        if !name[..prefix.len()].eq_ignore_ascii_case(prefix) {
            continue;
        }
        let rest = &name[prefix.len()..];
        let has_boundary = rest.is_empty() || rest.starts_with([' ', '-', '_', '(']);
        if require_boundary && !has_boundary {
            continue;
        }
        let suffix = if has_boundary {
            clean_account_suffix(rest)
        } else {
            None
        };
        return Some((provider, suffix));
    }
    None
}

fn clean_account_suffix(rest: &str) -> Option<String> {
    let mut trimmed = rest.trim();
    if let Some(stripped) = trimmed.strip_prefix(['-', '_']) {
        trimmed = stripped.trim();
    }
    if let Some(inner) = trimmed.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        trimmed = inner.trim();
    }
    let folder_tag = "-folder";
    if trimmed.len() >= folder_tag.len()
        && trimmed.is_char_boundary(trimmed.len() - folder_tag.len())
        && trimmed[trimmed.len() - folder_tag.len()..].eq_ignore_ascii_case(folder_tag)
    {
        trimmed = trimmed[..trimmed.len() - folder_tag.len()].trim();
    } else if trimmed.eq_ignore_ascii_case("folder") {
        trimmed = "";
    }
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn detect_mounted_volumes() -> Vec<DetectedLocation> {
    let mut seen_mounts = HashSet::new();
    let mut volumes = Vec::new();

    for (name, entry_path) in sorted_visible_subdirs(Path::new("/Volumes")) {
        let Ok(stat) = rustix::fs::statfs(&entry_path) else {
            continue;
        };
        if (stat.f_flags & MNT_ROOTFS) != 0 || (stat.f_flags & MNT_DONTBROWSE) != 0 {
            continue;
        }
        let mntonname = c_chars_to_string(&stat.f_mntonname);
        let mount_path = Path::new(&mntonname);
        if !mntonname.starts_with("/Volumes/") || mount_path == Path::new("/Volumes") {
            continue;
        }
        let matches_mount = mount_path == entry_path
            || fs::canonicalize(&entry_path).is_ok_and(|canon| canon == mount_path);
        if !matches_mount || !seen_mounts.insert(mntonname.clone()) {
            continue;
        }
        let fstype = c_chars_to_string(&stat.f_fstypename).to_ascii_lowercase();
        let kind = if is_network_mount(stat.f_flags, &fstype) {
            LocationKind::NetworkDrive { fstype }
        } else {
            LocationKind::ExternalDrive
        };
        let label = mount_volume_name(&mntonname).unwrap_or(name);
        let path = PathBuf::from(mntonname);
        let accessible = crate::permissions::check_path_access(&path)
            != crate::permissions::PermissionStatus::Denied;
        volumes.push(DetectedLocation {
            label,
            path,
            kind,
            accessible,
        });
    }
    volumes
}

fn sorted_visible_subdirs(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(read_dir) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<(String, PathBuf)> = read_dir
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.is_empty() || name.starts_with('.') {
                return None;
            }
            let path = entry.path();
            is_dir_or_restricted(&entry, &path).then_some((name, path))
        })
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    entries
}

fn is_dir_or_restricted(entry: &fs::DirEntry, path: &Path) -> bool {
    if let Ok(ft) = entry.file_type()
        && !ft.is_symlink()
    {
        return ft.is_dir();
    }
    match fs::metadata(path) {
        Ok(meta) => meta.is_dir(),
        Err(error) => error.kind() == std::io::ErrorKind::PermissionDenied,
    }
}

fn canonical_or_self(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn is_network_mount(flags: u32, fstype: &str) -> bool {
    (flags & MNT_LOCAL) == 0 || NETWORK_FSTYPES.contains(&fstype)
}

fn mount_volume_name(mntonname: &str) -> Option<String> {
    Path::new(mntonname)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
}

fn c_chars_to_string(chars: &[c_char]) -> String {
    let bytes: Vec<u8> = chars
        .iter()
        .copied()
        .take_while(|&ch| ch != 0)
        .map(|ch| ch as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

const fn kind_rank(kind: &LocationKind) -> u8 {
    match kind {
        LocationKind::Cloud(_) => 0,
        LocationKind::ExternalDrive => 1,
        LocationKind::NetworkDrive { .. } => 2,
    }
}
