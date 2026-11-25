//! Bounded cancel-on-supersede `This Mac` Spotlight search worker with `lstat` filtering.

use std::collections::HashSet;
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};

use super::work::{WorkError, Workers};
use crate::application::contracts::configuration::validate_saved_path;
use crate::application::contracts::directory::{Cancellation, EntryKind};
use crate::application::files::search::{
    MAX_THIS_MAC_RESULTS, SearchCompletion, SearchRequest, SearchResult, SystemSearchOutput,
    format_parent_display, normalize_query,
};
use crate::diagnostics::{Activity, Outcome, Subsystem, session};
use crate::domain::SafetyPolicy;
use crate::domain::matching::match_name;
use crate::platform::search::{SearchFailure, search_filenames};

pub struct SearchWorkers {
    workers: Workers<Option<Result<SystemSearchOutput, String>>>,
}

impl SearchWorkers {
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            workers: Workers::new("search", 1, 2)?,
        })
    }

    pub fn search(
        &self,
        request: SearchRequest,
    ) -> (
        Cancellation,
        impl Future<Output = Option<SearchCompletion>> + use<>,
    ) {
        let generation = request.generation;
        let future = self.workers.submit(move |cancel| {
            let started = std::time::Instant::now();
            let raw = match search_filenames(&request.query, || cancel.is_cancelled()) {
                Ok(raw) => raw,
                Err(SearchFailure::Cancelled) => return None,
                Err(SearchFailure::InvalidQuery) => {
                    return Some(Ok(SystemSearchOutput {
                        results: Vec::new(),
                        truncated: false,
                    }));
                }
                Err(SearchFailure::TimedOut) => {
                    record_search_issue(generation, Outcome::TimedOut, started.elapsed());
                    return Some(Err(
                        "Spotlight search timed out. Try a more specific filename.".into(),
                    ));
                }
                Err(SearchFailure::Unavailable(message)) => {
                    record_search_issue(generation, Outcome::Unavailable, started.elapsed());
                    return Some(Err(message));
                }
            };
            if cancel.is_cancelled() {
                return None;
            }
            Some(Ok(filter_candidates(
                raw.paths,
                raw.truncated,
                &request,
                cancel,
            )))
        });
        let cancellation = future.cancellation();
        (cancellation, async move {
            match future.await {
                Ok(Some(result)) => Some(SearchCompletion { generation, result }),
                Ok(None) | Err(WorkError::Cancelled) => None,
                Err(WorkError::Busy) => Some(SearchCompletion {
                    generation,
                    result: Err("Spotlight search is busy. Wait a moment and retry.".into()),
                }),
                Err(WorkError::Unavailable) => Some(SearchCompletion {
                    generation,
                    result: Err(
                        "Spotlight search service is unavailable. Relaunch and retry.".into(),
                    ),
                }),
            }
        })
    }
}

fn filter_candidates(
    paths: Vec<PathBuf>,
    raw_truncated: bool,
    request: &SearchRequest,
    cancel: &Cancellation,
) -> SystemSearchOutput {
    let query = normalize_query(&request.query);
    let mut seen = HashSet::with_capacity(paths.len());
    let mut results = Vec::new();
    for path in paths {
        if cancel.is_cancelled() {
            break;
        }
        if let Some(item) = inspect_candidate(
            path,
            query,
            &request.safety,
            &request.exclude_paths,
            &request.home,
            &mut seen,
        ) {
            results.push(item);
        }
    }
    results.sort_by(|left, right| {
        left.tier
            .cmp(&right.tier)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.path.cmp(&right.path))
    });
    let total = results.len();
    let truncated = raw_truncated || total > MAX_THIS_MAC_RESULTS;
    results.truncate(MAX_THIS_MAC_RESULTS);
    SystemSearchOutput { results, truncated }
}

fn inspect_candidate(
    path: PathBuf,
    query: &str,
    safety: &SafetyPolicy,
    exclude_paths: &HashSet<PathBuf>,
    home: &Path,
    seen: &mut HashSet<PathBuf>,
) -> Option<SearchResult> {
    if !path.is_absolute()
        || validate_saved_path(&path).is_err()
        || exclude_paths.contains(&path)
        || !seen.insert(path.clone())
    {
        return None;
    }
    let parent = path.parent()?;
    if safety.check_read_directory(&path).is_err() || safety.check_read_directory(parent).is_err() {
        return None;
    }
    let file_name = path.file_name()?;
    let tier = match_name(file_name, query)?;
    // Canonicalize only `parent` (never `path` itself) so symlinked ancestors stay enforced
    // without touching or hydrating `SF_DATALESS` files.
    let canonical_parent = fs::canonicalize(parent).ok()?;
    let canonical_path = canonical_parent.join(file_name);
    if safety.check_read_directory(&canonical_path).is_err()
        || safety.check_read_directory(&canonical_parent).is_err()
    {
        return None;
    }
    // Always use lstat (`symlink_metadata`) so online-only cloud items stay unhydrated.
    let meta = fs::symlink_metadata(&path).ok()?;
    let file_type = meta.file_type();
    let kind = if file_type.is_file() {
        EntryKind::File
    } else if file_type.is_dir() {
        EntryKind::Directory
    } else {
        return None;
    };
    let dataless = super::is_dataless(&meta);
    Some(SearchResult {
        name: file_name.to_string_lossy().into_owned(),
        parent_display: format_parent_display(&path, home),
        path,
        kind,
        dataless,
        tier,
    })
}

fn record_search_issue(generation: u64, outcome: Outcome, elapsed: std::time::Duration) {
    session().record(
        Subsystem::Search,
        Activity::Search,
        outcome,
        generation,
        0,
        Some(elapsed),
    );
}
