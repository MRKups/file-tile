//! Shared session estimates. Only explicit demand can refresh an expired value.
use crate::application::{Application, Effect, contracts::directory::ListingOrigin};
use crate::diagnostics::{Activity, Outcome, Subsystem, session};
use crate::domain::ViewId;
use std::{
    collections::{BTreeSet, HashMap},
    path::PathBuf,
    time::{Duration, Instant},
};

pub const CACHE_LIMIT: usize = 512;
pub const CACHE_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Estimate {
    pub bytes: u64,
    pub partial: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageError {
    Cancelled,
    TimedOut,
    Unavailable,
    InvalidOutput,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageRequest {
    pub directory: PathBuf,
    pub generation: u64,
}
#[derive(Debug, Clone)]
pub struct UsageReport {
    pub total: Estimate,
    pub children: Vec<(PathBuf, Estimate)>,
}
#[derive(Debug, Clone)]
pub struct UsageCompletion {
    pub request: UsageRequest,
    pub result: Result<UsageReport, UsageError>,
}
#[derive(Debug)]
enum Phase {
    Idle,
    Queued,
    Running(UsageRequest),
}
#[derive(Debug)]
struct Cached {
    phase: Phase,
    result: Option<Result<Estimate, UsageError>>,
    measured: Option<Instant>,
    attempted: Option<Instant>,
    failure: Option<UsageError>,
    generation: u64,
    seen: u64,
}
#[derive(Debug, Default)]
pub struct UsageBook {
    paths: HashMap<PathBuf, Cached>,
    generation: u64,
    clock: u64,
    explicit_reads: BTreeSet<u64>,
}
impl UsageBook {
    pub fn get(&self, path: &std::path::Path) -> Option<&Result<Estimate, UsageError>> {
        self.paths.get(path)?.result.as_ref()
    }
    pub fn is_pending(&self, path: &std::path::Path) -> bool {
        self.paths
            .get(path)
            .is_some_and(|entry| !matches!(entry.phase, Phase::Idle))
    }
    fn make_room(&mut self, protected: &BTreeSet<PathBuf>) -> bool {
        if self.paths.len() < CACHE_LIMIT {
            return true;
        }
        let oldest = self
            .paths
            .iter()
            .filter(|(path, entry)| {
                !protected.contains(*path) && !matches!(entry.phase, Phase::Running(_))
            })
            .min_by(|(a, x), (b, y)| x.seen.cmp(&y.seen).then(a.cmp(b)))
            .map(|(path, _)| path.clone());
        if let Some(oldest) = oldest {
            let generation = self
                .paths
                .remove(&oldest)
                .map_or(0, |entry| entry.generation);
            session().record(
                Subsystem::Size,
                Activity::Evict,
                Outcome::Complete,
                generation,
                0,
                None,
            );
            true
        } else {
            false
        }
    }
    fn demand(&mut self, path: PathBuf, protected: &BTreeSet<PathBuf>, now: Instant) {
        self.clock = self.clock.saturating_add(1);
        if let Some(entry) = self.paths.get_mut(&path) {
            entry.seen = self.clock;
            if matches!(entry.phase, Phase::Idle) {
                if entry
                    .attempted
                    .is_none_or(|at| now.saturating_duration_since(at) >= CACHE_TTL)
                {
                    entry.phase = Phase::Queued;
                    session().record(
                        Subsystem::Size,
                        Activity::CacheExpired,
                        Outcome::Requested,
                        entry.generation,
                        0,
                        entry.measured.map(|at| now.saturating_duration_since(at)),
                    );
                } else {
                    session().record(
                        Subsystem::Size,
                        Activity::CacheReuse,
                        if entry.failure.is_some() {
                            Outcome::Backoff
                        } else if matches!(entry.result, Some(Ok(Estimate { partial: true, .. }))) {
                            Outcome::Partial
                        } else {
                            Outcome::Complete
                        },
                        entry.generation,
                        0,
                        entry.measured.map(|at| now.saturating_duration_since(at)),
                    );
                }
            }
        } else if self.make_room(protected) {
            self.paths.insert(
                path,
                Cached {
                    phase: Phase::Queued,
                    result: None,
                    measured: None,
                    attempted: None,
                    failure: None,
                    generation: 0,
                    seen: self.clock,
                },
            );
            session().record(
                Subsystem::Size,
                Activity::CacheMiss,
                Outcome::Requested,
                0,
                0,
                None,
            );
        }
    }
}
impl Application {
    pub fn folder_usage(&self, view: ViewId) -> Option<&Result<Estimate, UsageError>> {
        let directory = &self.load(view)?.listing()?.directory;
        self.usage.get(directory)
    }
    pub fn folder_usages(&self) -> &UsageBook {
        &self.usage
    }
    pub fn usage_requests(&self) -> impl Iterator<Item = &UsageRequest> {
        self.usage
            .paths
            .values()
            .filter_map(|entry| match &entry.phase {
                Phase::Running(request) => Some(request),
                _ => None,
            })
    }
    fn usage_paths(&self) -> BTreeSet<PathBuf> {
        let policy = self.safety_policy(None);
        self.workspace
            .active_views()
            .filter_map(|view| {
                let listing = self.load(view.view)?.listing()?;
                (listing.origin == ListingOrigin::Local
                    && policy.check_read_directory(&listing.directory).is_ok())
                .then(|| listing.directory.clone())
            })
            .collect()
    }
    pub(in crate::application) fn demand_folder_usage(&mut self, path: PathBuf, now: Instant) {
        if self
            .safety_policy(None)
            .check_read_directory(&path)
            .is_err()
        {
            return;
        }
        let protected = self.usage_paths();
        self.usage.demand(path, &protected, now);
    }
    pub(in crate::application) fn demand_usage_read(&mut self, generation: u64) {
        self.usage.explicit_reads.insert(generation);
    }
    pub(in crate::application) fn finish_usage_read(&mut self, generation: u64) -> bool {
        self.usage.explicit_reads.remove(&generation)
    }
    pub(in crate::application) fn sync_folder_usage(&mut self) -> Vec<Effect> {
        let now = Instant::now();
        let live_reads: BTreeSet<_> = self
            .loads
            .values()
            .filter_map(|load| match load {
                crate::application::LoadState::Loading { request, .. } => Some(request.generation),
                _ => None,
            })
            .collect();
        self.usage
            .explicit_reads
            .retain(|id| live_reads.contains(id));
        if !self.accepts_edits() {
            for entry in self.usage.paths.values_mut() {
                if !matches!(entry.phase, Phase::Idle) {
                    entry.phase = Phase::Idle;
                    entry.attempted = Some(now);
                    entry.failure = Some(UsageError::Cancelled);
                    if entry.result.is_none() {
                        entry.result = Some(Err(UsageError::Cancelled));
                    }
                }
            }
            return vec![];
        }
        let paths = self.usage_paths();
        for path in &paths {
            if !self.usage.paths.contains_key(path) {
                self.usage.demand(path.clone(), &paths, now);
            }
        }
        let mut slots = 2usize.saturating_sub(self.usage_requests().count());
        let mut effects = vec![];
        for path in paths {
            if slots == 0 {
                break;
            }
            let Some(entry) = self.usage.paths.get_mut(&path) else {
                continue;
            };
            if !matches!(entry.phase, Phase::Queued) {
                continue;
            }
            let Some(generation) = self.usage.generation.checked_add(1) else {
                entry.phase = Phase::Idle;
                entry.attempted = Some(now);
                entry.failure = Some(UsageError::Unavailable);
                if entry.result.is_none() {
                    entry.result = Some(Err(UsageError::Unavailable));
                }
                continue;
            };
            self.usage.generation = generation;
            let request = UsageRequest {
                directory: path,
                generation,
            };
            entry.phase = Phase::Running(request.clone());
            entry.generation = generation;
            session().record(
                Subsystem::Size,
                Activity::Measure,
                Outcome::Queued,
                generation,
                0,
                None,
            );
            slots -= 1;
            effects.push(Effect::MeasureFolderUsage(request));
        }
        effects
    }
    pub(in crate::application) fn complete_folder_usage(&mut self, completion: UsageCompletion) {
        self.complete_folder_usage_at(completion, Instant::now());
    }
    fn complete_folder_usage_at(&mut self, completion: UsageCompletion, now: Instant) {
        let Some(entry) = self.usage.paths.get_mut(&completion.request.directory) else {
            session().record(
                Subsystem::Size,
                Activity::Receipt,
                Outcome::MissingOwner,
                completion.request.generation,
                0,
                None,
            );
            return;
        };
        if !matches!(&entry.phase, Phase::Running(request) if request == &completion.request) {
            session().record(
                Subsystem::Size,
                Activity::Receipt,
                Outcome::Stale,
                completion.request.generation,
                0,
                None,
            );
            return;
        }
        entry.phase = Phase::Idle;
        entry.attempted = Some(now);
        entry.failure = completion.result.as_ref().err().copied();
        let report = match completion.result {
            Ok(report) => {
                entry.result = Some(Ok(report.total));
                entry.measured = Some(now);
                report
            }
            Err(error) => {
                if !matches!(entry.result, Some(Ok(_))) {
                    entry.result = Some(Err(error));
                }
                return;
            }
        };
        let protected = self.usage_paths();
        for (path, value) in report.children.into_iter().take(CACHE_LIMIT) {
            if path.parent() != Some(completion.request.directory.as_path()) {
                continue;
            }
            if let Some(entry) = self.usage.paths.get_mut(&path) {
                if !matches!(entry.phase, Phase::Running(_))
                    && entry.generation <= completion.request.generation
                {
                    entry.phase = Phase::Idle;
                    entry.result = Some(Ok(value));
                    entry.measured = Some(now);
                    entry.attempted = Some(now);
                    entry.failure = None;
                    entry.generation = completion.request.generation;
                }
            } else if self.usage.make_room(&protected) {
                self.usage.paths.insert(
                    path,
                    Cached {
                        phase: Phase::Idle,
                        result: Some(Ok(value)),
                        measured: Some(now),
                        attempted: Some(now),
                        failure: None,
                        generation: completion.request.generation,
                        seen: self.usage.clock,
                    },
                );
            }
        }
    }
}
