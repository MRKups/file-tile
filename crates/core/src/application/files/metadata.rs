//! View-owned visible metadata demand. Listings remain usable when individual stats fail.
use crate::application::contracts::directory::DirectoryListing;
use crate::application::{Application, Effect, LoadState};
use crate::domain::ViewId;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

pub const BATCH_LIMIT: usize = 64;
pub const CACHE_LIMIT: usize = 512;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMetadata {
    pub bytes: Option<u64>,
    pub modified: Result<SystemTime, MetadataError>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataError {
    Missing,
    Denied,
    Unsupported,
    Cancelled,
    Busy,
    Unavailable,
    Io(String),
}
impl MetadataError {
    pub fn from_io(error: std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::NotFound => Self::Missing,
            std::io::ErrorKind::PermissionDenied => Self::Denied,
            _ => Self::Io(error.to_string()),
        }
    }
    pub fn guidance(&self) -> String {
        format!(
            "Cannot read file metadata: {}. Reload the folder to retry.",
            match self {
                Self::Missing => "the entry was removed",
                Self::Denied => "access was denied",
                Self::Unsupported => "this metadata is unsupported",
                Self::Cancelled => "the read was cancelled",
                Self::Busy => "file workers are busy",
                Self::Unavailable => "the location or worker is unavailable",
                Self::Io(error) => error,
            }
        )
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataRequest {
    pub view: ViewId,
    pub generation: u64,
    pub paths: Vec<PathBuf>,
}
#[derive(Debug, Clone)]
pub struct MetadataCompletion {
    pub request: MetadataRequest,
    pub result: Result<MetadataValues, MetadataError>,
}
pub type MetadataValues = Vec<(PathBuf, Result<FileMetadata, MetadataError>)>;
#[derive(Debug, Default)]
pub(in crate::application) struct MetadataBook {
    views: HashMap<ViewId, ViewMetadata>,
    generation: u64,
    cursor: u64,
}
#[derive(Debug)]
struct ViewMetadata {
    listing: Arc<DirectoryListing>,
    demand: Vec<PathBuf>,
    cache: HashMap<PathBuf, Result<FileMetadata, MetadataError>>,
    order: VecDeque<PathBuf>,
    pending: Option<MetadataRequest>,
}
impl Application {
    pub fn metadata_values(
        &self,
        view: ViewId,
    ) -> Option<&HashMap<PathBuf, Result<FileMetadata, MetadataError>>> {
        Some(&self.metadata.views.get(&view)?.cache)
    }
    pub fn metadata_demand_paths(&self, view: ViewId) -> Option<&[PathBuf]> {
        Some(&self.metadata.views.get(&view)?.demand)
    }
    pub fn metadata(
        &self,
        view: ViewId,
        path: &Path,
    ) -> Option<&Result<FileMetadata, MetadataError>> {
        self.metadata.views.get(&view)?.cache.get(path)
    }
    pub fn metadata_requests(&self) -> impl Iterator<Item = &MetadataRequest> {
        self.metadata
            .views
            .values()
            .filter_map(|view| view.pending.as_ref())
    }
    pub(in crate::application) fn sync_metadata(&mut self) {
        let workspace = &self.workspace;
        let loads = &self.loads;
        let closing = !self.accepts_edits();
        self.metadata.views.retain(|view, state| {
            !closing
                && workspace.is_active(*view)
                && workspace.view(*view).is_some()
                && loads
                    .get(view)
                    .and_then(LoadState::retained_listing)
                    .is_some_and(|listing| Arc::ptr_eq(&listing, &state.listing))
        });
    }
    pub(in crate::application) fn request_metadata(
        &mut self,
        view: ViewId,
        paths: Vec<PathBuf>,
    ) -> Vec<Effect> {
        if !self.workspace.is_active(view) || paths.len() > CACHE_LIMIT {
            return vec![];
        }
        let Some(listing) = self.loads.get(&view).and_then(LoadState::retained_listing) else {
            return vec![];
        };
        let Some(item) = self.workspace.view(view) else {
            return vec![];
        };
        let preview = self.valid_column_preview(view).cloned();
        let mut seen = HashSet::new();
        let demand: Vec<_> = paths
            .iter()
            .filter(|path| {
                listing
                    .entry(path)
                    .or_else(|| preview.as_deref().and_then(|sub| sub.entry(path)))
                    .is_some_and(|entry| item.settings.show_hidden || !entry.hidden)
            })
            .filter(|path| seen.insert(path.as_path()))
            .cloned()
            .collect();
        let state = self
            .metadata
            .views
            .entry(view)
            .or_insert_with(|| ViewMetadata {
                listing,
                demand: vec![],
                cache: HashMap::new(),
                order: VecDeque::new(),
                pending: None,
            });
        if state.demand != demand {
            state.pending = None;
            state.demand = demand;
        }
        self.fill_metadata()
    }
    pub(in crate::application) fn complete_metadata(
        &mut self,
        completion: MetadataCompletion,
    ) -> Vec<Effect> {
        self.sync_metadata();
        let Some(state) = self.metadata.views.get_mut(&completion.request.view) else {
            return vec![];
        };
        if state.pending.as_ref() != Some(&completion.request) {
            return vec![];
        }
        state.pending = None;
        for path in &completion.request.paths {
            let result = match &completion.result {
                Ok(values) => values
                    .iter()
                    .find(|(candidate, _)| candidate == path)
                    .map(|(_, value)| value.clone())
                    .unwrap_or(Err(MetadataError::Unavailable)),
                Err(error) => Err(error.clone()),
            };
            state.cache.insert(path.clone(), result);
            state.order.retain(|candidate| candidate != path);
            state.order.push_back(path.clone());
        }
        while state.cache.len() > CACHE_LIMIT {
            if let Some(path) = state.order.pop_front() {
                state.cache.remove(&path);
            } else {
                break;
            }
        }
        self.fill_metadata()
    }
    pub(in crate::application) fn fill_metadata(&mut self) -> Vec<Effect> {
        let mut slots = 2usize.saturating_sub(self.metadata_requests().count());
        let mut effects = vec![];
        let mut views: Vec<_> = self.metadata.views.keys().copied().collect();
        views.sort_by_key(|view| (view.0 <= self.metadata.cursor, view.0));
        for view in views {
            if slots == 0 {
                break;
            }
            let Some(state) = self.metadata.views.get_mut(&view) else {
                continue;
            };
            if state.pending.is_some() {
                continue;
            }
            let paths: Vec<_> = state
                .demand
                .iter()
                .filter(|path| !state.cache.contains_key(*path))
                .take(BATCH_LIMIT)
                .cloned()
                .collect();
            if paths.is_empty() {
                continue;
            }
            let Some(generation) = self.metadata.generation.checked_add(1) else {
                break;
            };
            self.metadata.generation = generation;
            let request = MetadataRequest {
                view,
                generation,
                paths,
            };
            state.pending = Some(request.clone());
            effects.push(Effect::ReadMetadata(request));
            slots -= 1;
            self.metadata.cursor = view.0;
        }
        effects
    }
}
