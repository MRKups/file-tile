//! Bounded background worker for volume, cloud-storage, and pinned-folder access discovery.

use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::path::{Path, PathBuf};

use super::work::Workers;
use crate::application::contracts::directory::Cancellation;
use crate::domain::{SafetyPolicy, SafetyPreferences};
use crate::platform::permissions::{PermissionStatus, check_path_access};
use crate::platform::volumes::{DetectedLocation, LocationKind};

pub type LocationProbe = (Vec<DetectedLocation>, BTreeSet<PathBuf>);

pub struct LocationWorkers {
    workers: Workers<LocationProbe>,
}

impl LocationWorkers {
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            workers: Workers::new("locations", 1, 2)?,
        })
    }

    pub fn detect(
        &self,
        home: PathBuf,
        pinned: Vec<PathBuf>,
        safety: SafetyPreferences,
    ) -> (
        Cancellation,
        impl Future<Output = Option<LocationProbe>> + use<>,
    ) {
        let future = self.workers.submit(move |cancel| {
            if cancel.is_cancelled() {
                return (Vec::new(), BTreeSet::new());
            }
            let locations = crate::platform::volumes::detect_locations(&home);
            if cancel.is_cancelled() {
                return (Vec::new(), BTreeSet::new());
            }
            let ungranted_pins = probe_pinned_folders(&home, pinned, safety, &locations, cancel);
            (locations, ungranted_pins)
        });
        let cancellation = future.cancellation();
        (cancellation, async move { future.await.ok() })
    }
}

fn probe_pinned_folders(
    home: &Path,
    pinned: Vec<PathBuf>,
    safety: SafetyPreferences,
    locations: &[DetectedLocation],
    cancel: &Cancellation,
) -> BTreeSet<PathBuf> {
    let mut external_drive_roots = Vec::new();
    let mut network_share_roots = Vec::new();
    for loc in locations {
        match loc.kind {
            LocationKind::ExternalDrive => external_drive_roots.push(loc.path.clone()),
            LocationKind::NetworkDrive { .. } => network_share_roots.push(loc.path.clone()),
            LocationKind::Cloud(_) => {}
        }
    }
    let policy = SafetyPolicy {
        preferences: safety,
        home: home.to_path_buf(),
        state_dir: None,
        prohibit_delete: false,
        external_drive_roots,
        network_share_roots,
    };

    let mut home_root_cache: HashMap<PathBuf, PermissionStatus> = HashMap::new();
    let protected_home_roots = [
        home.join("Desktop"),
        home.join("Documents"),
        home.join("Downloads"),
    ];

    let mut ungranted = BTreeSet::new();
    for pin in pinned {
        if cancel.is_cancelled() {
            return BTreeSet::new();
        }
        if policy.check_read_directory(&pin).is_err() {
            continue;
        }
        if let Some(loc) = locations.iter().find(|loc| pin.starts_with(&loc.path)) {
            if !loc.accessible {
                ungranted.insert(pin);
                continue;
            }
            if matches!(loc.kind, LocationKind::NetworkDrive { .. }) {
                continue;
            }
        }
        if let Some(root) = protected_home_roots
            .iter()
            .find(|root| pin != **root && pin.starts_with(root))
        {
            let status = *home_root_cache
                .entry(root.clone())
                .or_insert_with(|| check_path_access(root));
            if status == PermissionStatus::Denied {
                ungranted.insert(pin);
                continue;
            }
        }
        if check_path_access(&pin) == PermissionStatus::Denied {
            ungranted.insert(pin);
        }
    }
    ungranted
}
