//! Owned directory-observation tags and freshness receipts, never native events.

use std::path::PathBuf;

use crate::domain::ViewId;

use crate::application::{Application, Effect, LoadState};
use crate::diagnostics::{Activity, Outcome, Subsystem, session};
use crate::domain::NavigationIntent;
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObservationTag {
    pub view: ViewId,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryObservation {
    pub tag: ObservationTag,
    pub directory: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchState {
    Starting,
    Watching,
    Unavailable(String),
}

#[derive(Debug, Clone)]
pub struct WatchUpdate {
    pub tag: ObservationTag,
    pub state: Option<WatchState>,
    pub changed: bool,
}

#[derive(Debug, Clone, Default)]
pub struct WatchBatch {
    pub updates: Vec<WatchUpdate>,
}

#[derive(Debug)]
struct ObservedView {
    observation: DirectoryObservation,
    state: WatchState,
}

#[derive(Debug)]
struct ReloadTarget {
    view: ViewId,
    directory: PathBuf,
    observation: Option<ObservationTag>,
}

#[derive(Debug, Default)]
pub(in crate::application) struct ObservationBook {
    views: HashMap<ViewId, ObservedView>,
    next_generation: u64,
    failure: Option<WatchState>,
    reloads: VecDeque<ReloadTarget>,
}

impl Application {
    pub(in crate::application) fn invalidate_workspace_observations(&mut self) {
        self.watching.views.clear();
        self.watching.reloads.clear();
        self.watching.failure = None;
    }
    pub fn watch_state(&self, view: ViewId) -> Option<&WatchState> {
        self.workspace.view(view)?;
        self.watching
            .failure
            .as_ref()
            .or_else(|| self.watching.views.get(&view).map(|entry| &entry.state))
    }

    pub fn observations(&self) -> Vec<DirectoryObservation> {
        let mut observations: Vec<_> = self
            .watching
            .views
            .values()
            .map(|entry| entry.observation.clone())
            .collect();
        observations.sort_by_key(|item| item.tag.view.0);
        observations
    }

    pub(in crate::application) fn sync_observations(&mut self, force: bool) -> Vec<Effect> {
        let before = self.watching.views.len();
        let enabled = self.accepts_edits();
        self.watching.views.retain(|view, entry| {
            enabled
                && self.workspace.is_active(*view)
                && self
                    .workspace
                    .view(*view)
                    .is_some_and(|item| item.navigation.current() == entry.observation.directory)
        });
        let mut changed = before != self.watching.views.len();
        if enabled && self.watching.failure.is_none() {
            for item in self.workspace.active_views() {
                if self.watching.views.contains_key(&item.view) {
                    continue;
                }
                let Some(generation) = self.watching.next_generation.checked_add(1) else {
                    self.watching.views.clear();
                    self.watching.failure = Some(WatchState::Unavailable("Observation identities are exhausted. Save the workspace and relaunch; reload all boxes in the meantime.".into()));
                    changed = true;
                    break;
                };
                self.watching.next_generation = generation;
                self.watching.views.insert(
                    item.view,
                    ObservedView {
                        observation: DirectoryObservation {
                            tag: ObservationTag {
                                view: item.view,
                                generation,
                            },
                            directory: item.navigation.current().into(),
                        },
                        state: WatchState::Starting,
                    },
                );
                changed = true;
            }
        }
        self.watching.reloads.retain(|target| {
            self.workspace
                .view(target.view)
                .is_some_and(|item| item.navigation.current() == target.directory)
                && self.workspace.is_active(target.view)
                && target.observation.is_none_or(|tag| {
                    self.watching
                        .views
                        .get(&tag.view)
                        .is_some_and(|entry| entry.observation.tag == tag)
                })
        });
        if !enabled {
            self.watching.reloads.clear();
        }
        if changed || force {
            vec![Effect::ObserveDirectories(self.observations())]
        } else {
            Vec::new()
        }
    }

    pub(in crate::application) fn watch_completed(&mut self, batch: WatchBatch) {
        let mut stale = 0;
        let count = batch.updates.len() as u64;
        session().record(
            Subsystem::Watcher,
            Activity::ReloadWatch,
            Outcome::Requested,
            0,
            batch.updates.iter().filter(|update| update.changed).count() as u64,
            None,
        );
        for update in batch.updates {
            let Some(entry) = self
                .watching
                .views
                .get_mut(&update.tag.view)
                .filter(|entry| entry.observation.tag == update.tag)
            else {
                stale += 1;
                continue;
            };
            if let Some(state) = update.state {
                entry.state = state;
            }
            if update.changed {
                let target = ReloadTarget {
                    view: update.tag.view,
                    directory: entry.observation.directory.clone(),
                    observation: Some(update.tag),
                };
                self.enqueue_reload(target);
            }
        }
        if stale > 0 {
            session().record(
                Subsystem::Watcher,
                Activity::Receipt,
                Outcome::Stale,
                count,
                stale,
                None,
            );
        }
    }

    pub(in crate::application) fn reload_all(&mut self) {
        session().record(
            Subsystem::Listing,
            Activity::ReloadManual,
            Outcome::Requested,
            0,
            self.workspace.active_views().count() as u64,
            None,
        );
        let targets: Vec<_> = self
            .workspace
            .active_views()
            .map(|item| ReloadTarget {
                view: item.view,
                directory: item.navigation.current().into(),
                observation: None,
            })
            .collect();
        for target in targets {
            self.demand_folder_usage(target.directory.clone(), std::time::Instant::now());
            self.enqueue_reload(target);
        }
    }

    pub(in crate::application) fn invalidate_file_directory(
        &mut self,
        directory: &std::path::Path,
    ) {
        session().record(
            Subsystem::Listing,
            Activity::ReloadWrite,
            Outcome::Requested,
            0,
            0,
            None,
        );
        let targets: Vec<_> = self.workspace.active_views().filter_map(|item| {
            let pending_target = matches!(self.load(item.view), Some(crate::application::LoadState::Loading { request, .. }) if request.directory == directory);
            (item.navigation.current() == directory || pending_target).then(|| ReloadTarget {
                view: item.view, directory: directory.into(), observation: None,
            })
        }).collect();
        for target in targets {
            self.enqueue_reload(target);
        }
    }

    fn enqueue_reload(&mut self, target: ReloadTarget) {
        if !self
            .watching
            .reloads
            .iter()
            .any(|queued| queued.view == target.view && queued.directory == target.directory)
        {
            self.watching.reloads.push_back(target);
        }
    }

    pub(in crate::application) fn fill_reload_reads(&mut self) -> Vec<Effect> {
        if !self.accepts_edits() {
            return Vec::new();
        }
        let mut effects = Vec::new();
        let mut loading = self
            .loads
            .values()
            .filter(|load| matches!(load, LoadState::Loading { .. }))
            .count();
        let candidates = self.watching.reloads.len();
        for _ in 0..candidates {
            if loading >= 2 {
                break;
            }
            let Some(target) = self.watching.reloads.pop_front() else {
                break;
            };
            if matches!(self.load(target.view), Some(LoadState::Loading { request, .. }) if request.directory == target.directory)
            {
                self.watching.reloads.push_back(target);
                continue;
            }
            if !self
                .workspace
                .view(target.view)
                .is_some_and(|item| item.navigation.current() == target.directory)
                || !self.workspace.is_active(target.view)
                || target.observation.is_some_and(|tag| {
                    !self
                        .watching
                        .views
                        .get(&tag.view)
                        .is_some_and(|entry| entry.observation.tag == tag)
                })
            {
                continue;
            }
            // Never replace pending navigation/initialization with a content reread.
            if matches!(self.load(target.view), Some(LoadState::Loading { .. })) {
                self.watching.reloads.push_back(target);
                continue;
            }
            // A still-queued startup read has the same job. Let the bounded restore owner admit it.
            if self.restore_queue.contains(&target.view) {
                continue;
            }
            if let Some(effect) =
                self.request(target.view, target.directory, NavigationIntent::Reload)
            {
                loading += 1;
                effects.push(effect);
            }
        }
        effects
    }
}
