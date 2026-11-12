//! App-wide pinned sidebar folders and live detected storage locations.

use std::path::{Path, PathBuf};

use crate::application::contracts::configuration::{MAX_PINNED_FOLDERS, validate_saved_path};
use crate::application::contracts::directory::EntryKind;
use crate::application::contracts::storage::{CloseStage, PersistenceState};
use crate::application::{Application, Effect, LoadState};
use crate::diagnostics::{Change, configuration_changed};
use crate::platform::volumes::DetectedLocation;

impl Application {
    pub fn pinned_folders(&self) -> &[PathBuf] {
        self.pinned_folders
            .as_deref()
            .or_else(|| {
                self.configuration
                    .as_ref()
                    .and_then(|(preferences, _)| preferences.pinned_folders.as_deref())
            })
            .unwrap_or(&self.default_pinned_folders)
    }

    pub fn is_pinned_folder(&self, path: &Path) -> bool {
        self.pinned_folders().iter().any(|item| item == path)
    }

    pub fn can_pin_folder(&self, path: &Path) -> bool {
        self.accepts_workspace_input()
            && validate_saved_path(path).is_ok()
            && !self.is_known_non_directory(path)
            && (self.is_pinned_folder(path) || self.pinned_folders().len() < MAX_PINNED_FOLDERS)
    }

    fn is_known_non_directory(&self, path: &Path) -> bool {
        let in_primary = self
            .loads
            .values()
            .filter_map(LoadState::listing)
            .any(|listing| {
                listing
                    .entry(path)
                    .is_some_and(|entry| entry.kind != EntryKind::Directory)
            });
        in_primary
            || self.column_previews.values().any(|listing| {
                listing
                    .entry(path)
                    .is_some_and(|entry| entry.kind != EntryKind::Directory)
            })
    }

    pub(in crate::application) fn pin_folder(
        &mut self,
        path: PathBuf,
        index: Option<usize>,
    ) -> Vec<Effect> {
        if !self.accepts_workspace_input()
            || validate_saved_path(&path).is_err()
            || self.is_known_non_directory(&path)
        {
            return Vec::new();
        }
        let mut list = self.pinned_folders().to_vec();
        let before = list.clone();
        let added_new = if let Some(existing) = list.iter().position(|item| item == &path) {
            let Some(target) = index else {
                return Vec::new();
            };
            let target = target.min(list.len());
            let item = list.remove(existing);
            let insert_at = if existing < target {
                target.saturating_sub(1)
            } else {
                target
            }
            .min(list.len());
            list.insert(insert_at, item);
            if list == before {
                return Vec::new();
            }
            false
        } else {
            if list.len() >= MAX_PINNED_FOLDERS {
                return Vec::new();
            }
            let insert_at = index.unwrap_or(list.len()).min(list.len());
            list.insert(insert_at, path);
            true
        };
        let mut effects = self.set_pinned_folders(list);
        if added_new {
            effects.extend(self.refresh_locations());
        }
        effects
    }

    pub(in crate::application) fn unpin_folder(&mut self, path: &Path) -> Vec<Effect> {
        if !self.accepts_workspace_input() {
            return Vec::new();
        }
        let mut list = self.pinned_folders().to_vec();
        let before_len = list.len();
        list.retain(|item| item != path);
        if list.len() == before_len {
            return Vec::new();
        }
        self.ungranted_pins.remove(path);
        self.set_pinned_folders(list)
    }

    pub(in crate::application) fn reorder_pinned_folder(
        &mut self,
        from: usize,
        to: usize,
    ) -> Vec<Effect> {
        if !self.accepts_workspace_input() {
            return Vec::new();
        }
        let mut list = self.pinned_folders().to_vec();
        if from >= list.len() {
            return Vec::new();
        }
        let before = list.clone();
        let item = list.remove(from);
        list.insert(to.min(list.len()), item);
        if list == before {
            return Vec::new();
        }
        self.set_pinned_folders(list)
    }

    fn set_pinned_folders(&mut self, list: Vec<PathBuf>) -> Vec<Effect> {
        self.pinned_folders = Some(list.clone());
        if let Some((preferences, _)) = &mut self.configuration {
            preferences.pinned_folders = Some(list);
        }
        configuration_changed(Change::AppSettings);
        self.save_effect(true)
    }

    pub fn locations(&self) -> &[DetectedLocation] {
        &self.locations
    }

    pub fn ungranted_pins(&self) -> &std::collections::BTreeSet<PathBuf> {
        &self.ungranted_pins
    }

    pub(in crate::application) fn refresh_locations(&mut self) -> Vec<Effect> {
        if self.files_closing()
            || matches!(
                self.persistence,
                PersistenceState::Ready {
                    closing: CloseStage::SamplingWindow | CloseStage::Flushing | CloseStage::Failed,
                    ..
                } | PersistenceState::Recovery { closing: true, .. }
            )
        {
            return Vec::new();
        }
        let Some(generation) = self.location_generation.checked_add(1) else {
            return Vec::new();
        };
        self.location_generation = generation;
        vec![Effect::RefreshLocations {
            generation,
            home: self.home.clone(),
            pinned: self.pinned_folders().to_vec(),
            safety: self.safety(),
        }]
    }

    pub(in crate::application) fn locations_loaded(
        &mut self,
        generation: u64,
        locations: Vec<DetectedLocation>,
        ungranted_pins: std::collections::BTreeSet<PathBuf>,
    ) -> Vec<Effect> {
        if generation != self.location_generation {
            return Vec::new();
        }
        let current_pins = self.pinned_folders();
        let filtered_pins: std::collections::BTreeSet<PathBuf> = ungranted_pins
            .into_iter()
            .filter(|pin| {
                current_pins.iter().any(|item| item == pin)
                    && !self.loads.values().any(|load| {
                        matches!(load, LoadState::Ready(listing) if listing.directory.starts_with(pin))
                    })
            })
            .collect();
        let mut adjusted_locations = locations;
        for loc in &mut adjusted_locations {
            if !loc.accessible
                && self.loads.values().any(|load| {
                    matches!(load, LoadState::Ready(listing) if listing.directory.starts_with(&loc.path))
                })
            {
                loc.accessible = true;
            }
        }
        if self.locations == adjusted_locations && self.ungranted_pins == filtered_pins {
            return Vec::new();
        }
        let previous_policy = self.safety_policy(None);
        self.locations = adjusted_locations;
        self.ungranted_pins = filtered_pins;
        let mut effects = self.reconcile_tree_roots();
        effects.extend(self.reconcile_safety_views_from_policy(&previous_policy));
        effects
    }
}
