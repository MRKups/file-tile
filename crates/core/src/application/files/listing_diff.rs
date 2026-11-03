//! Pure entry-level diff between consecutive reloads of the same directory.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::application::contracts::directory::{DirectoryEntry, DirectoryListing, EntryKind};

/// Cap per-refresh animated changes so extreme filesystem storms settle without unbounded work.
pub const MAX_ANIMATED_CHANGES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrivalKind {
    Created,
    Renamed { relocated: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitKind {
    Deleted,
    Relocated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletedGhost {
    /// Zero-based index among visible entries in the prior displayed order.
    pub prior_index: usize,
    /// Surviving entry immediately before this item in the prior displayed order,
    /// or `None` when the item sat at the top of the list.
    pub after_path: Option<PathBuf>,
    pub kind: ExitKind,
    pub entry: DirectoryEntry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListingDiff {
    /// Visible entries did not change; leave any active animation untouched.
    Unchanged,
    /// Bounded set of changes suitable for in-box row and tile transitions.
    Animated {
        arrivals: HashMap<PathBuf, ArrivalKind>,
        deletions: Vec<DeletedGhost>,
    },
    /// Extreme storm; skip transitions and clear stale highlights.
    Bulk,
}

#[derive(Clone, Copy)]
enum RemovedOutcome {
    Deleted,
    RenamedInPlace,
    RenamedRelocated,
}

pub fn compute(
    previous_directory: &Path,
    previous_ordered: &[DirectoryEntry],
    current: &DirectoryListing,
    current_order: Option<&[usize]>,
    show_hidden: bool,
) -> ListingDiff {
    if previous_directory != current.directory {
        return ListingDiff::Unchanged;
    }

    let is_visible = |entry: &DirectoryEntry| show_hidden || !entry.hidden;
    let current_visible = |path: &Path| current.entry(path).is_some_and(is_visible);

    let previous_paths: HashSet<&Path> = previous_ordered
        .iter()
        .filter(|entry| is_visible(entry))
        .map(|entry| entry.path.as_path())
        .collect();

    let mut removed: Vec<(usize, Option<PathBuf>, &DirectoryEntry)> = Vec::new();
    let mut last_survivor: Option<PathBuf> = None;
    for (prior_index, entry) in previous_ordered
        .iter()
        .filter(|entry| is_visible(entry))
        .enumerate()
    {
        if current_visible(&entry.path) {
            last_survivor = Some(entry.path.clone());
        } else {
            removed.push((prior_index, last_survivor.clone(), entry));
        }
    }

    let mut added: Vec<(Option<PathBuf>, &DirectoryEntry)> = Vec::new();
    let mut last_current_survivor: Option<PathBuf> = None;
    let count = current_order.map_or_else(|| current.display_len(show_hidden), <[usize]>::len);
    for index in 0..count {
        let entry = if let Some(order) = current_order {
            order
                .get(index)
                .and_then(|target| current.entries().get(*target))
        } else {
            current.display_entry(index, show_hidden)
        };
        let Some(entry) = entry.filter(|entry| is_visible(entry)) else {
            continue;
        };
        if previous_paths.contains(entry.path.as_path()) {
            last_current_survivor = Some(entry.path.clone());
        } else {
            added.push((last_current_survivor.clone(), entry));
        }
    }

    if removed.is_empty() && added.is_empty() {
        return ListingDiff::Unchanged;
    }

    if removed.len() > MAX_ANIMATED_CHANGES * 2 || added.len() > MAX_ANIMATED_CHANGES * 2 {
        return ListingDiff::Bulk;
    }

    let (arrivals, deletions) = pair_renames(removed, &added);
    let change_count = arrivals.len() + deletions.len();
    if change_count == 0 {
        ListingDiff::Unchanged
    } else if change_count > MAX_ANIMATED_CHANGES {
        ListingDiff::Bulk
    } else {
        ListingDiff::Animated {
            arrivals,
            deletions,
        }
    }
}

fn pair_renames<'a>(
    removed: Vec<(usize, Option<PathBuf>, &'a DirectoryEntry)>,
    added: &[(Option<PathBuf>, &'a DirectoryEntry)],
) -> (HashMap<PathBuf, ArrivalKind>, Vec<DeletedGhost>) {
    let mut removed_by_inode: HashMap<(u64, EntryKind), Vec<usize>> = HashMap::new();
    for (idx, (_, _, entry)) in removed.iter().enumerate() {
        if let Some(ino) = entry.inode.filter(|ino| *ino != 0) {
            removed_by_inode
                .entry((ino, entry.kind))
                .or_default()
                .push(idx);
        }
    }

    let mut added_by_inode: HashMap<(u64, EntryKind), Vec<usize>> = HashMap::new();
    for (idx, (_, entry)) in added.iter().enumerate() {
        if let Some(ino) = entry.inode.filter(|ino| *ino != 0) {
            added_by_inode
                .entry((ino, entry.kind))
                .or_default()
                .push(idx);
        }
    }

    let mut outcomes = vec![RemovedOutcome::Deleted; removed.len()];
    let mut arrivals: HashMap<PathBuf, ArrivalKind> = HashMap::with_capacity(added.len());

    for (added_idx, (new_after_path, entry)) in added.iter().enumerate() {
        let matched_rem = entry.inode.filter(|ino| *ino != 0).and_then(|ino| {
            let key = (ino, entry.kind);
            let rem_indices = removed_by_inode.get(&key)?;
            let add_indices = added_by_inode.get(&key)?;
            (rem_indices.len() == 1 && add_indices.len() == 1 && add_indices[0] == added_idx)
                .then_some(rem_indices[0])
        });

        let kind = if let Some(rem_idx) = matched_rem {
            let relocated = &removed[rem_idx].1 != new_after_path;
            outcomes[rem_idx] = if relocated {
                RemovedOutcome::RenamedRelocated
            } else {
                RemovedOutcome::RenamedInPlace
            };
            ArrivalKind::Renamed { relocated }
        } else {
            ArrivalKind::Created
        };
        arrivals.insert(entry.path.clone(), kind);
    }

    let deletions = removed
        .into_iter()
        .enumerate()
        .filter_map(
            |(idx, (prior_index, after_path, entry))| match outcomes[idx] {
                RemovedOutcome::RenamedInPlace => None,
                RemovedOutcome::Deleted => Some(DeletedGhost {
                    prior_index,
                    after_path,
                    kind: ExitKind::Deleted,
                    entry: entry.clone(),
                }),
                RemovedOutcome::RenamedRelocated => Some(DeletedGhost {
                    prior_index,
                    after_path,
                    kind: ExitKind::Relocated,
                    entry: entry.clone(),
                }),
            },
        )
        .collect();
    (arrivals, deletions)
}
