//! Per-view ordered projections. Directory snapshots and UI selection stay independent.
use crate::application::{
    Application, Effect, LoadState,
    contracts::directory::{DirectoryEntry, DirectoryListing},
};
use crate::domain::{ViewId, sorting::SortSettings};
use std::{collections::HashMap, sync::Arc};
pub(crate) mod ordering;

pub const PREPARING: &str = "Preparing sort…";

#[derive(Debug, Clone)]
pub struct SortRequest {
    pub view: ViewId,
    pub generation: u64,
    pub listing: Arc<DirectoryListing>,
    pub settings: SortSettings,
    pub show_hidden: bool,
}
#[derive(Debug, Clone)]
pub struct SortCompletion {
    pub request: SortRequest,
    pub result: Result<SortedEntries, String>,
}
#[derive(Debug, Clone)]
pub struct SortedEntries {
    pub indices: Vec<usize>,
    pub unavailable: usize,
}
#[derive(Debug, Default)]
pub(in crate::application) struct SortBook {
    views: HashMap<ViewId, State>,
    generation: u64,
}
#[derive(Debug)]
struct State {
    request: SortRequest,
    phase: Phase,
    order: Option<SortedEntries>,
    revision: u64,
    status: Option<String>,
}
#[derive(Debug, PartialEq, Eq)]
enum Phase {
    Queued,
    Running,
    Finished,
}
impl Application {
    pub fn sort_requests(&self) -> impl Iterator<Item = &SortRequest> {
        self.sorting
            .views
            .values()
            .filter(|state| state.phase == Phase::Running)
            .map(|state| &state.request)
    }
    pub fn sort_indices(&self, view: ViewId) -> Option<&[usize]> {
        self.sorting
            .views
            .get(&view)?
            .order
            .as_ref()
            .map(|order| order.indices.as_slice())
    }
    pub fn sort_revision(&self, view: ViewId) -> u64 {
        self.sorting
            .views
            .get(&view)
            .map_or(0, |state| state.revision)
    }
    pub fn sort_status(&self, view: ViewId) -> Option<&str> {
        self.sorting.views.get(&view)?.status.as_deref()
    }
    pub fn ordered_entries(&self, view: ViewId) -> impl Iterator<Item = &DirectoryEntry> {
        let listing = self.load(view).and_then(LoadState::listing);
        let hidden = self
            .workspace
            .view(view)
            .is_some_and(|item| item.settings.show_hidden);
        let indices = self.sort_indices(view);
        let count = indices.map_or_else(
            || listing.map_or(0, |listing| listing.display_len(hidden)),
            <[usize]>::len,
        );
        (0..count).filter_map(move |index| {
            let listing = listing?;
            if let Some(indices) = indices {
                listing.entries().get(*indices.get(index)?)
            } else {
                listing.display_entry(index, hidden)
            }
        })
    }
    pub(in crate::application) fn sync_sorting(&mut self) -> Vec<Effect> {
        let accepts = self.accepts_edits();
        let workspace = &self.workspace;
        let loads = &self.loads;
        self.sorting.views.retain(|view, _| {
            accepts
                && workspace.is_active(*view)
                && loads
                    .get(view)
                    .and_then(LoadState::retained_listing)
                    .is_some()
        });
        let active: Vec<_> = self
            .workspace
            .boxes()
            .iter()
            .map(|item| (item.active_view().view, item.settings))
            .collect();
        for (view, settings) in active {
            if !self.accepts_sort_view(view) {
                continue;
            }
            let Some(listing) = self.loads.get(&view).and_then(LoadState::retained_listing) else {
                continue;
            };
            let state = self.sorting.views.get(&view);
            if state.is_some_and(|state| {
                Arc::ptr_eq(&listing, &state.request.listing)
                    && state.request.settings == settings.sort
                    && state.request.show_hidden == settings.show_hidden
            }) {
                continue;
            }
            let Some(generation) = self.sorting.generation.checked_add(1) else {
                self.sorting.views.remove(&view);
                continue;
            };
            self.sorting.generation = generation;
            let previous = self.sorting.views.remove(&view);
            let announce = previous.as_ref().is_none_or(|state| {
                state.request.listing.directory != listing.directory
                    || state.request.settings != settings.sort
                    || state.request.show_hidden != settings.show_hidden
            });
            // Preserve usable prior ordering across same-folder catch-ups and reloads
            // while background workers collect fresh sort metadata.
            let request = SortRequest {
                view,
                generation,
                listing,
                settings: settings.sort,
                show_hidden: settings.show_hidden,
            };
            let (order, revision, carried_status) = carry_over_order(previous, &request);
            let status = if announce {
                Some(PREPARING.into())
            } else {
                carried_status
            };
            self.sorting.views.insert(
                view,
                State {
                    request,
                    phase: Phase::Queued,
                    order,
                    revision,
                    status,
                },
            );
        }
        let mut slots = 2usize.saturating_sub(self.sort_requests().count());
        let mut views: Vec<_> = self.sorting.views.keys().copied().collect();
        views.sort_by_key(|view| view.0);
        let mut effects = vec![];
        for view in views {
            if slots == 0 {
                break;
            }
            if let Some(state) = self.sorting.views.get_mut(&view)
                && state.phase == Phase::Queued
            {
                state.phase = Phase::Running;
                slots -= 1;
                effects.push(Effect::SortDirectory(state.request.clone()));
            }
        }
        effects
    }
    fn accepts_sort_view(&self, view: ViewId) -> bool {
        self.accepts_edits() && self.workspace.is_active(view)
    }
    pub(in crate::application) fn complete_sort(&mut self, completion: SortCompletion) {
        let Some(state) = self.sorting.views.get_mut(&completion.request.view) else {
            return;
        };
        if state.phase != Phase::Running
            || state.request.generation != completion.request.generation
        {
            return;
        }
        state.phase = Phase::Finished;
        match completion.result {
            Ok(order) => {
                state.status = (order.unavailable > 0).then(|| {
                    format!(
                        "Sort values unavailable for {} items. Reload folder listings to retry.",
                        order.unavailable
                    )
                });
                if state
                    .order
                    .as_ref()
                    .is_none_or(|previous| previous.indices != order.indices)
                {
                    state.revision = state.request.generation;
                }
                state.order = Some(order);
            }
            Err(error) => {
                state.status = Some(format!(
                    "Cannot sort folder: {error}. Reload folder listings to retry."
                ))
            }
        }
    }
}

fn carry_over_order(
    previous: Option<State>,
    request: &SortRequest,
) -> (Option<SortedEntries>, u64, Option<String>) {
    let Some(state) = previous.filter(|state| {
        state.request.show_hidden == request.show_hidden
            && state.request.listing.directory == request.listing.directory
    }) else {
        return (None, 0, None);
    };
    if Arc::ptr_eq(&request.listing, &state.request.listing)
        || state.request.listing.entries() == request.listing.entries()
    {
        return (state.order, state.revision, state.status);
    }
    if state.request.settings != request.settings {
        return (None, 0, None);
    }
    let entries = request.listing.entries();
    if matches!(
        request.settings.field,
        crate::domain::sorting::SortField::Name | crate::domain::sorting::SortField::Type
    ) {
        let rows = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| request.show_hidden || !entry.hidden)
            .map(|(index, _)| ordering::SortValue {
                index,
                metadata: None,
            })
            .collect();
        return (
            Some(ordering::order(request, rows)),
            state.revision,
            state.status,
        );
    }
    let Some(prev_order) = state.order else {
        return (None, 0, None);
    };
    let projected = project_reloaded_order(&state.request.listing, &prev_order, request);
    (Some(projected), state.revision, state.status)
}

fn project_reloaded_order(
    previous_listing: &DirectoryListing,
    previous_order: &SortedEntries,
    request: &SortRequest,
) -> SortedEntries {
    let entries = request.listing.entries();
    let mut remaining: HashMap<&std::path::Path, usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| request.show_hidden || !entry.hidden)
        .map(|(idx, entry)| (entry.path.as_path(), idx))
        .collect();
    let survivors =
        match_reloaded_survivors(previous_listing, previous_order, entries, &mut remaining);
    let added_rows: Vec<_> = remaining
        .into_values()
        .map(|index| ordering::SortValue {
            index,
            metadata: None,
        })
        .collect();
    let added = ordering::order(request, added_rows).indices;
    let prepend = request.settings.field == crate::domain::sorting::SortField::Modified
        && request.settings.direction == crate::domain::sorting::SortDirection::Descending;
    let is_dir = |idx: usize| {
        entries[idx].kind == crate::application::contracts::directory::EntryKind::Directory
    };
    let mut indices = Vec::with_capacity(survivors.len() + added.len());
    if request.settings.folders_first {
        let surv_split = survivors
            .iter()
            .position(|&idx| !is_dir(idx))
            .unwrap_or(survivors.len());
        let add_split = added
            .iter()
            .position(|&idx| !is_dir(idx))
            .unwrap_or(added.len());
        if prepend {
            indices.extend_from_slice(&added[..add_split]);
            indices.extend_from_slice(&survivors[..surv_split]);
            indices.extend_from_slice(&added[add_split..]);
            indices.extend_from_slice(&survivors[surv_split..]);
        } else {
            indices.extend_from_slice(&survivors[..surv_split]);
            indices.extend_from_slice(&added[..add_split]);
            indices.extend_from_slice(&survivors[surv_split..]);
            indices.extend_from_slice(&added[add_split..]);
        }
    } else if prepend {
        indices.extend_from_slice(&added);
        indices.extend_from_slice(&survivors);
    } else {
        indices.extend_from_slice(&survivors);
        indices.extend_from_slice(&added);
    }
    SortedEntries {
        indices,
        unavailable: previous_order.unavailable,
    }
}

fn match_reloaded_survivors<'a>(
    previous_listing: &DirectoryListing,
    previous_order: &SortedEntries,
    entries: &'a [DirectoryEntry],
    remaining: &mut HashMap<&'a std::path::Path, usize>,
) -> Vec<usize> {
    let mut exact_paths = std::collections::HashSet::with_capacity(previous_order.indices.len());
    let mut old_inodes = HashMap::new();
    for &old_idx in &previous_order.indices {
        let Some(old_entry) = previous_listing.entries().get(old_idx) else {
            continue;
        };
        if remaining.contains_key(old_entry.path.as_path()) {
            exact_paths.insert(old_entry.path.as_path());
        } else if let Some(ino) = old_entry.inode.filter(|&ino| ino != 0) {
            *old_inodes.entry((ino, old_entry.kind)).or_insert(0usize) += 1;
        }
    }
    let mut new_inodes = HashMap::new();
    for (&path, &new_idx) in remaining.iter() {
        if exact_paths.contains(path) {
            continue;
        }
        if let Some(entry) = entries.get(new_idx)
            && let Some(ino) = entry.inode.filter(|&ino| ino != 0)
        {
            new_inodes
                .entry((ino, entry.kind))
                .and_modify(|slot| *slot = None)
                .or_insert(Some((path, new_idx)));
        }
    }
    let mut survivors = Vec::with_capacity(remaining.len());
    for &old_idx in &previous_order.indices {
        let Some(old_entry) = previous_listing.entries().get(old_idx) else {
            continue;
        };
        if let Some(new_idx) = remaining.remove(old_entry.path.as_path()) {
            survivors.push(new_idx);
        } else if let Some(ino) = old_entry.inode.filter(|&ino| ino != 0)
            && old_inodes.get(&(ino, old_entry.kind)) == Some(&1)
            && let Some(Some((new_path, new_idx))) = new_inodes.remove(&(ino, old_entry.kind))
            && remaining.remove(new_path).is_some()
        {
            survivors.push(new_idx);
        }
    }
    survivors
}
