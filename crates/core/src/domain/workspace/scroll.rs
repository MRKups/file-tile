//! Durable ordinal scroll anchor updates and proposal baseline comparisons.

use std::collections::HashMap;
use std::path::Path;

use super::{FolderBox, FolderView, Tabs, ViewId, Workspace};

impl Workspace {
    /// Updates a view's durable `top_item` anchor in memory without adding an Undo step.
    pub fn set_view_top_item(&mut self, view: ViewId, top_item: u32) -> bool {
        let Some(item) = self.view_mut(view) else {
            return false;
        };
        if item.top_item == top_item {
            return false;
        }
        item.top_item = top_item;
        true
    }

    /// Compares durable workspace baselines while ignoring `FolderView.top_item` so
    /// live momentum scrolling does not invalidate async tab or scope checks.
    pub fn same_proposal_baseline(&self, other: &Self) -> bool {
        self.next_identity == other.next_identity
            && self.launchers == other.launchers
            && self.boxes.len() == other.boxes.len()
            && self
                .boxes
                .iter()
                .zip(&other.boxes)
                .all(|(lhs, rhs)| same_box_baseline(lhs, rhs))
    }
}

pub(super) fn carry_live_scroll(previous: &[FolderBox], next: &mut [FolderBox]) {
    let live: HashMap<ViewId, (&Path, u32)> = previous
        .iter()
        .flat_map(FolderBox::views)
        .map(|view| (view.view, (view.navigation.current(), view.top_item)))
        .collect();
    for item in next {
        for view in item.views_mut() {
            if let Some(&(folder, top_item)) = live.get(&view.view)
                && view.navigation.current() == folder
            {
                view.top_item = top_item;
            }
        }
    }
}

pub(super) fn carry_proposal_scroll(previous: &[FolderBox], next: &mut [FolderBox]) {
    carry_live_scroll(previous, next);
    for item in next {
        if let Some(tabs) = &mut item.tabs
            && tabs.views.len() == 1
            && let Some(prev_box) = previous
                .iter()
                .find(|prev| prev.id == item.id && prev.tabs.is_none())
            && tabs.views[0].navigation.current() == prev_box.standalone.navigation.current()
        {
            tabs.views[0].top_item = prev_box.standalone.top_item;
        }
    }
}

fn same_view_baseline(lhs: &FolderView, rhs: &FolderView) -> bool {
    lhs.view == rhs.view && lhs.navigation == rhs.navigation
}

fn same_tabs_baseline(lhs: &Tabs, rhs: &Tabs) -> bool {
    lhs.visible == rhs.visible
        && lhs.active == rhs.active
        && lhs.views.len() == rhs.views.len()
        && lhs
            .views
            .iter()
            .zip(&rhs.views)
            .all(|(a, b)| same_view_baseline(a, b))
}

fn same_box_baseline(lhs: &FolderBox, rhs: &FolderBox) -> bool {
    lhs.id == rhs.id
        && lhs.rect == rhs.rect
        && lhs.settings == rhs.settings
        && same_view_baseline(&lhs.standalone, &rhs.standalone)
        && match (&lhs.tabs, &rhs.tabs) {
            (None, None) => true,
            (Some(a), Some(b)) => same_tabs_baseline(a, b),
            _ => false,
        }
}
