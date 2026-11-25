//! Bounded, session-only workspace history. App/window preferences are separate.

use super::*;
use crate::domain::NavigationIntent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoOutcome {
    Empty,
    CancelledGesture,
    Restored,
}

#[derive(Debug, Clone, Default)]
pub(super) struct UndoHistory {
    snapshots: Vec<(Vec<FolderBox>, Vec<LauncherTile>)>,
}

impl Workspace {
    pub(crate) fn propose_scope(&self, view: ViewId, scope: crate::domain::Scope) -> Option<Self> {
        if !self
            .view(view)
            .is_some_and(|item| self.configuring(item.id))
        {
            return None;
        }
        let mut proposal = self.snapshot();
        let item = proposal.view_mut(view)?;
        let before_current = item.navigation.current().to_path_buf();
        item.navigation.set_scope(scope);
        if item.navigation.current() != before_current {
            item.top_item = 0;
        }
        (!proposal.same_proposal_baseline(self)).then_some(proposal)
    }
    pub const UNDO_LIMIT: usize = 50;

    pub(super) fn remember(&mut self) {
        let history = self
            .undo
            .get_or_insert_with(|| Box::new(UndoHistory::default()));
        history
            .snapshots
            .push((self.boxes.clone(), self.launchers.clone()));
        if history.snapshots.len() > Self::UNDO_LIMIT {
            history.snapshots.remove(0);
        }
    }

    pub fn can_undo(&self) -> bool {
        self.gesture.is_some()
            || self
                .undo
                .as_ref()
                .is_some_and(|history| !history.snapshots.is_empty())
    }

    pub fn undo(&mut self) -> UndoOutcome {
        if self.gesture.take().is_some() {
            return UndoOutcome::CancelledGesture;
        }
        let Some((mut boxes, launchers)) = self
            .undo
            .as_mut()
            .and_then(|history| history.snapshots.pop())
        else {
            return UndoOutcome::Empty;
        };
        scroll::carry_live_scroll(&self.boxes, &mut boxes);
        self.boxes = boxes;
        self.launchers = launchers;
        self.selected = None;
        // Do not rewind identity allocation. Late reads cannot target a new box
        // that reused an identity discarded by Undo.
        UndoOutcome::Restored
    }

    pub(crate) fn commit_navigation(
        &mut self,
        view: ViewId,
        path: PathBuf,
        intent: NavigationIntent,
    ) -> bool {
        if intent == NavigationIntent::Configure
            && let Some(box_index) = self
                .boxes
                .iter()
                .position(|item| item.has_visible_tabs() && item.view(view).is_some())
        {
            let before = self.boxes[box_index].clone();
            let mut updated = before.clone();
            updated.configure_default_folder(path);
            if updated == before {
                return false;
            }
            self.remember();
            self.boxes[box_index] = updated;
            return true;
        }
        let Some(item) = self.view(view) else {
            return false;
        };
        let before = item.navigation.clone();
        let mut navigation = before.clone();
        navigation.commit(path.clone(), intent);
        if navigation == before {
            return false;
        }
        let resets_scroll = matches!(
            intent,
            NavigationIntent::Open
                | NavigationIntent::Back
                | NavigationIntent::Forward
                | NavigationIntent::Configure
        );
        if resets_scroll {
            self.remember();
        }
        if intent == NavigationIntent::Initialize
            && let Some(history) = self.undo.as_mut()
        {
            // Initial canonicalization completes creation, not another Undo step.
            // A layout snapshot taken while that read ran must retain the resolved
            // start/location when its geometry is later undone.
            for (snapshot, _) in history.snapshots.iter_mut() {
                if let Some(item) = snapshot
                    .iter_mut()
                    .find_map(|item| item.view_mut(view))
                    .filter(|item| item.navigation == before)
                {
                    item.navigation.commit(path.clone(), intent);
                }
            }
        }
        if let Some(item) = self.view_mut(view) {
            item.navigation = navigation;
            if resets_scroll {
                item.top_item = 0;
            }
        }
        true
    }
}
