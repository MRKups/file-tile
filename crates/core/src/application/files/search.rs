//! Pure global Search aggregation (`Open tiles`) and background `This Mac` request/open state.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::application::contracts::configuration::validate_saved_path;
use crate::application::contracts::directory::EntryKind;
use crate::application::{Application, Effect};
use crate::domain::Mode;
use crate::domain::SafetyPolicy;
use crate::domain::matching::{MatchTier, match_name};

pub const MAX_OPEN_TILE_RESULTS: usize = 200;
pub const MAX_THIS_MAC_RESULTS: usize = 60;
pub const MAX_QUERY_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub path: PathBuf,
    pub name: String,
    pub parent_display: String,
    pub kind: EntryKind,
    pub dataless: bool,
    pub tier: MatchTier,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenTilesMatches {
    pub results: Vec<SearchResult>,
    pub total_matches: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemSearchOutput {
    pub results: Vec<SearchResult>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum SystemSearchStatus {
    #[default]
    Idle,
    Searching,
    Ready {
        results: Vec<SearchResult>,
        truncated: bool,
    },
    Unavailable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRequest {
    pub generation: u64,
    pub query: String,
    pub safety: SafetyPolicy,
    pub exclude_paths: HashSet<PathBuf>,
    pub home: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchCompletion {
    pub generation: u64,
    pub result: Result<SystemSearchOutput, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchOpenRequest {
    pub generation: u64,
    pub path: PathBuf,
    pub kind: EntryKind,
    pub safety: SafetyPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchOpenCompletion {
    pub request: SearchOpenRequest,
    pub result: Result<(), String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SearchOpenPhase {
    Pending(u64),
    HandedOff,
    Failed(String),
}

#[derive(Debug, Default)]
pub(in crate::application) struct SearchBook {
    generation: u64,
    query: String,
    status: SystemSearchStatus,
    open_generation: u64,
    opening: Option<(SearchOpenRequest, SearchOpenPhase)>,
}

/// Trims surrounding whitespace so empty/whitespace-only search queries collapse consistently.
pub fn normalize_query(raw: &str) -> &str {
    raw.trim()
}

/// Formats `path`'s parent directory concisely, collapsing `home` prefixes to `~`.
pub fn format_parent_display(path: &Path, home: &Path) -> String {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(path);
    if !home.as_os_str().is_empty()
        && let Ok(relative) = parent.strip_prefix(home)
    {
        return if relative.as_os_str().is_empty() {
            "~".to_owned()
        } else {
            format!("~/{}", relative.display())
        };
    }
    parent.display().to_string()
}

impl Application {
    /// Collects deduplicated `Open tiles` matches across all loaded open browsable boxes,
    /// ordered `Prefix` before `Substring` and spatially `(row, column, box_id)` within each tier.
    pub fn search_open_tiles(&self, raw_query: &str) -> OpenTilesMatches {
        let query = normalize_query(raw_query);
        if query.is_empty() {
            return OpenTilesMatches::default();
        }
        let mut results = self.collect_open_tile_matches(query);
        let total_matches = results.len();
        let truncated = total_matches > MAX_OPEN_TILE_RESULTS;
        results.truncate(MAX_OPEN_TILE_RESULTS);
        OpenTilesMatches {
            results,
            total_matches,
            truncated,
        }
    }

    fn collect_open_tile_matches(&self, query: &str) -> Vec<SearchResult> {
        let policy = self.safety_policy(None);
        let mut seen = HashSet::new();
        let mut results = Vec::new();
        for (_box_id, view) in self.browsable_box_views() {
            let Some(listing) = self
                .load(view)
                .and_then(crate::application::LoadState::listing)
            else {
                continue;
            };
            for candidate in self.jump_candidates(view) {
                let entry = candidate.entry;
                if listing
                    .entry_target(entry)
                    .is_some_and(|target| policy.check_read_directory(target).is_err())
                {
                    continue;
                }
                if let Some(tier) = match_name(entry.name.as_ref(), query)
                    && seen.insert(entry.path.as_path())
                {
                    results.push(SearchResult {
                        path: entry.path.clone(),
                        name: entry.name.to_string_lossy().into_owned(),
                        parent_display: format_parent_display(&entry.path, &self.home),
                        kind: entry.kind,
                        dataless: entry.dataless,
                        tier,
                    });
                }
            }
        }
        results.sort_by_key(|item| item.tier);
        results
    }

    pub fn system_search_status(&self) -> &SystemSearchStatus {
        &self.search.status
    }

    pub fn search_open_error(&self) -> Option<&str> {
        match &self.search.opening {
            Some((_, SearchOpenPhase::Failed(message))) => Some(message.as_str()),
            _ => None,
        }
    }

    pub fn search_opening_busy(&self) -> bool {
        matches!(self.search.opening, Some((_, SearchOpenPhase::Pending(_))))
    }

    pub(in crate::application) fn search_system(&mut self, query: String) -> Vec<Effect> {
        if !self.accepts_workspace_input() || self.workspace.mode() != Mode::Browse {
            return Vec::new();
        }
        self.search.generation = self.search.generation.saturating_add(1);
        if matches!(self.search.opening, Some((_, SearchOpenPhase::Failed(_)))) {
            self.search.opening = None;
        }
        let trimmed = normalize_query(&query);
        if trimmed.is_empty() {
            self.search.query.clear();
            self.search.status = SystemSearchStatus::Idle;
            return Vec::new();
        }
        let exclude_paths = self
            .search_open_tiles(trimmed)
            .results
            .into_iter()
            .map(|item| item.path)
            .collect();
        self.search.query = trimmed.to_owned();
        self.search.status = SystemSearchStatus::Searching;
        vec![Effect::SearchSystem(SearchRequest {
            generation: self.search.generation,
            query: self.search.query.clone(),
            safety: self.safety_policy(None),
            exclude_paths,
            home: self.home.clone(),
        })]
    }

    pub(in crate::application) fn search_system_completed(&mut self, completion: SearchCompletion) {
        if completion.generation != self.search.generation
            || !self.accepts_edits()
            || self.workspace.mode() != Mode::Browse
        {
            return;
        }
        self.search.status = match completion.result {
            Ok(output) => SystemSearchStatus::Ready {
                results: output.results,
                truncated: output.truncated,
            },
            Err(message) => SystemSearchStatus::Unavailable(message),
        };
    }

    pub(in crate::application) fn cancel_search(&mut self) {
        self.search.generation = self.search.generation.saturating_add(1);
        self.search.query.clear();
        self.search.status = SystemSearchStatus::Idle;
        self.search.opening = None;
    }

    pub(in crate::application) fn open_search_result(
        &mut self,
        path: PathBuf,
        kind: EntryKind,
    ) -> Vec<Effect> {
        if !self.accepts_workspace_input() || self.workspace.mode() != Mode::Browse {
            return Vec::new();
        }
        if self.search_opening_busy() {
            return Vec::new();
        }
        let policy = self.safety_policy(None);
        if validate_saved_path(&path).is_err() {
            return self.fail_search_open(
                path,
                kind,
                "Cannot open this item: the path is invalid or exceeds 4,096 bytes.".into(),
            );
        }
        let safety_reason = policy.check_read_directory(&path).err().or_else(|| {
            path.parent()
                .and_then(|parent| policy.check_read_directory(parent).err())
        });
        if let Some(reason) = safety_reason {
            return self.fail_search_open(path, kind, reason.into());
        }
        if kind == EntryKind::Other {
            return self.fail_search_open(
                path,
                kind,
                "Cannot open this item: only regular files and folders are supported.".into(),
            );
        }
        let Some(generation) = self.search.open_generation.checked_add(1) else {
            return self.fail_search_open(
                path,
                kind,
                "Cannot open another item in this session. Relaunch the application and retry."
                    .into(),
            );
        };
        self.search.open_generation = generation;
        let request = SearchOpenRequest {
            generation,
            path,
            kind,
            safety: policy,
        };
        self.search.opening = Some((request.clone(), SearchOpenPhase::Pending(generation)));
        vec![Effect::OpenSearchResult(request)]
    }

    fn fail_search_open(&mut self, path: PathBuf, kind: EntryKind, message: String) -> Vec<Effect> {
        self.search.open_generation = self.search.open_generation.saturating_add(1);
        let request = SearchOpenRequest {
            generation: self.search.open_generation,
            path,
            kind,
            safety: self.safety_policy(None),
        };
        self.search.opening = Some((request, SearchOpenPhase::Failed(message)));
        Vec::new()
    }

    pub(in crate::application) fn search_result_opened(
        &mut self,
        completion: SearchOpenCompletion,
    ) {
        let Some((current, SearchOpenPhase::Pending(generation))) = &self.search.opening else {
            return;
        };
        if current != &completion.request || *generation != completion.request.generation {
            return;
        }
        let phase = match completion.result {
            Ok(()) => SearchOpenPhase::HandedOff,
            Err(message) => SearchOpenPhase::Failed(message),
        };
        self.search.opening = Some((completion.request, phase));
    }

    pub(in crate::application) fn sync_search(&mut self) {
        if !self.accepts_edits() || self.workspace.mode() != Mode::Browse {
            self.cancel_search();
        }
    }
}
