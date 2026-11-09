//! Selectable type-to-jump candidate resolution and spatial active-box ordering.

use crate::application::contracts::directory::{DirectoryEntry, DirectoryListing, EntryKind};
use crate::application::{Application, LoadState};
use crate::domain::{BoxId, Scope, ScopeKind, ViewId};

/// One selectable entry in a pane's display order paired with its unfiltered row/tile index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JumpCandidate<'a> {
    /// Unfiltered index in `Application::ordered_entries` or `Columns` preview order,
    /// preserving the rows/tiles occupied by dimmed outside-scope folders.
    pub display_index: usize,
    /// Directory entry record.
    pub entry: &'a DirectoryEntry,
}

/// Returns true when `entry` can be selected in `listing` under `scope`.
///
/// Matches `Desktop::visible_entry` and `file_list::entry_allowed`: non-directory entries
/// are always selectable inside an allowed current folder, `ScopeKind::Free` allows all
/// child directories (including unresolved directory symlinks), and restrictive scopes
/// require a resolved target allowed by `scope`.
pub fn entry_selectable(listing: &DirectoryListing, scope: &Scope, entry: &DirectoryEntry) -> bool {
    entry.kind != EntryKind::Directory
        || scope.kind() == ScopeKind::Free
        || listing
            .entry_target(entry)
            .is_some_and(|target| scope.allows(target))
}

/// Resolves selectable candidates from an explicit `ordered_indices` slice into `listing.entries()`,
/// retaining each entry's `display_index` in `ordered_indices` for scroll geometry.
pub fn listing_jump_candidates<'a>(
    listing: &'a DirectoryListing,
    ordered_indices: &[usize],
    show_hidden: bool,
    scope: &Scope,
) -> Vec<JumpCandidate<'a>> {
    ordered_indices
        .iter()
        .filter_map(|&raw_index| listing.entries().get(raw_index))
        .filter(|entry| show_hidden || !entry.hidden)
        .enumerate()
        .filter(|(_, entry)| entry_selectable(listing, scope, entry))
        .map(|(display_index, entry)| JumpCandidate {
            display_index,
            entry,
        })
        .collect()
}

impl Application {
    /// Returns selectable entries for `view` in current display order, keeping each item's
    /// unfiltered `display_index` in `ordered_entries(view)`.
    pub fn jump_candidates(&self, view: ViewId) -> Vec<JumpCandidate<'_>> {
        if !self.workspace.can_browse(view) {
            return Vec::new();
        }
        let Some(view_ref) = self.workspace.view(view) else {
            return Vec::new();
        };
        let Some(listing) = self.load(view).and_then(LoadState::listing) else {
            return Vec::new();
        };
        let show_hidden = view_ref.settings.show_hidden;
        let scope = view_ref.navigation.scope();
        self.ordered_entries(view)
            .enumerate()
            .filter(|(_, entry)| {
                (show_hidden || !entry.hidden) && entry_selectable(listing, scope, entry)
            })
            .map(|(display_index, entry)| JumpCandidate {
                display_index,
                entry,
            })
            .collect()
    }

    /// Returns open browsable boxes sorted spatially by `(row, column, box_id)` paired with
    /// each box's active `ViewId`.
    pub fn browsable_box_views(&self) -> Vec<(BoxId, ViewId)> {
        let mut items: Vec<_> = self
            .workspace
            .boxes()
            .iter()
            .filter_map(|item| {
                let view = item.active_view().view;
                self.workspace.can_browse(view).then_some((
                    item.rect.origin.row,
                    item.rect.origin.column,
                    item.id.0,
                    item.id,
                    view,
                ))
            })
            .collect();
        items.sort_unstable_by_key(|&(row, column, raw_id, _, _)| (row, column, raw_id));
        items
            .into_iter()
            .map(|(_, _, _, box_id, view_id)| (box_id, view_id))
            .collect()
    }
}
