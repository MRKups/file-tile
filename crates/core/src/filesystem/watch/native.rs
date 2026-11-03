//! Native registration and path routing. All filesystem checks run on the owner thread.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::application::contracts::watching::{
    DirectoryObservation, ObservationTag, WatchState, WatchUpdate,
};

use super::{Shared, Signal};

#[derive(Clone)]
struct Route {
    target: PathBuf,
    tags: Vec<ObservationTag>,
    available: bool,
}

pub(super) struct Plan {
    routes: Vec<Route>,
    roots: BTreeSet<PathBuf>,
    pub stamp: Vec<(PathBuf, u64, u64)>,
}

impl Plan {
    pub fn new(observations: &[DirectoryObservation]) -> Self {
        let mut grouped: BTreeMap<PathBuf, Vec<ObservationTag>> = BTreeMap::new();
        for observation in observations {
            grouped
                .entry(observation.directory.clone())
                .or_default()
                .push(observation.tag);
        }
        let mut plan = Self {
            routes: Vec::new(),
            roots: BTreeSet::new(),
            stamp: Vec::new(),
        };
        for (directory, tags) in grouped {
            let current = std::fs::canonicalize(&directory)
                .ok()
                .filter(|path| path.is_dir());
            let parent = nearest_parent(&directory);
            let target = current.clone().unwrap_or_else(|| {
                parent.as_ref().map_or_else(
                    || directory.clone(),
                    |(original, resolved)| {
                        directory
                            .strip_prefix(original)
                            .map_or_else(|_| directory.clone(), |suffix| resolved.join(suffix))
                    },
                )
            });
            if let Some(current) = &current {
                plan.roots.insert(current.clone());
            }
            if let Some((_, parent)) = parent {
                plan.roots.insert(parent);
            }
            plan.routes.push(Route {
                target,
                tags,
                available: current.is_some(),
            });
        }
        #[cfg(unix)]
        for root in &plan.roots {
            use std::os::unix::fs::MetadataExt;
            if let Ok(metadata) = std::fs::metadata(root) {
                plan.stamp
                    .push((root.clone(), metadata.dev(), metadata.ino()));
            }
        }
        plan
    }

    pub fn states(&self, failure: Option<&str>) -> Vec<WatchUpdate> {
        self.routes
            .iter()
            .flat_map(|route| {
                let state = if let Some(failure) = failure {
                    WatchState::Unavailable(failure.into())
                } else if route.available {
                    WatchState::Watching
                } else {
                    WatchState::Unavailable(
                        "The folder is unavailable. Its recovery location is being observed."
                            .into(),
                    )
                };
                route.tags.iter().map(move |tag| WatchUpdate {
                    tag: *tag,
                    state: Some(state.clone()),
                    changed: false,
                })
            })
            .collect()
    }
}

fn nearest_parent(directory: &Path) -> Option<(PathBuf, PathBuf)> {
    let mut candidate = directory.parent();
    while let Some(path) = candidate {
        if let Ok(resolved) = std::fs::canonicalize(path)
            && resolved.is_dir()
        {
            return Some((path.to_path_buf(), resolved));
        }
        candidate = path.parent();
    }
    None
}

pub(super) struct Native {
    #[cfg(target_os = "macos")]
    _watcher: notify::RecommendedWatcher,
}

impl Native {
    #[cfg(target_os = "macos")]
    pub fn start(plan: &Plan, shared: Arc<Shared>, epoch: u64) -> Result<Self, String> {
        use notify::{PathOp, Watcher};
        let routes = plan.routes.clone();
        let callback = move |event: notify::Result<notify::Event>| match event {
            Ok(event) => {
                if matches!(event.kind, notify::EventKind::Access(_)) {
                    return;
                }
                let all = event.need_rescan() || event.paths.len() > 512;
                let (tags, recheck) = route_event(&routes, &event.paths, all);
                if !tags.is_empty() {
                    shared.signal(Signal::Changed {
                        epoch,
                        tags,
                        recheck,
                    });
                }
            }
            Err(error) => shared.signal(Signal::Fault {
                epoch,
                tags: routes
                    .iter()
                    .flat_map(|route| route.tags.iter().copied())
                    .collect(),
                reason: format!("{:#?}", error.kind).chars().take(512).collect(),
            }),
        };
        let mut watcher = notify::RecommendedWatcher::new(callback, notify::Config::default())
            .map_err(|error| error.to_string())?;
        let operations = plan
            .roots
            .iter()
            .cloned()
            .map(PathOp::watch_non_recursive)
            .collect();
        watcher
            .update_paths(operations)
            .map_err(|error| error.to_string())?;
        Ok(Self { _watcher: watcher })
    }

    #[cfg(not(target_os = "macos"))]
    pub fn start(_plan: &Plan, _shared: Arc<Shared>, _epoch: u64) -> Result<Self, String> {
        Err("Native folder notifications are only implemented on macOS.".into())
    }
}

#[cfg(target_os = "macos")]
fn route_event(routes: &[Route], paths: &[PathBuf], all: bool) -> (Vec<ObservationTag>, bool) {
    let mut tags = Vec::new();
    let mut recheck = all;
    for route in routes {
        if all
            || paths.iter().any(|path| {
                path == &route.target
                    || path.parent() == Some(route.target.as_path())
                    || route.target.starts_with(path)
            })
        {
            tags.extend(route.tags.iter().copied());
            if !recheck {
                recheck = paths.iter().any(|path| route.target.starts_with(path));
            }
        }
    }
    (tags, recheck)
}
