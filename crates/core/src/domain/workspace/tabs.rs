//! Independent navigation views and atomic tab proposals. No I/O or widget state.

use super::{BoxId, BoxSettings, FolderBox, Mode, Navigation, ViewId, Workspace};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderView {
    pub view: ViewId,
    pub navigation: Navigation,
    pub top_item: u32,
}

impl FolderView {
    pub const fn new(view: ViewId, navigation: Navigation) -> Self {
        Self {
            view,
            navigation,
            top_item: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ViewRef<'a> {
    pub id: BoxId,
    pub view: ViewId,
    pub navigation: &'a Navigation,
    pub settings: BoxSettings,
    pub top_item: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tabs {
    pub(super) views: Vec<FolderView>,
    pub(super) active: usize,
    pub(super) visible: bool,
}

impl Tabs {
    pub fn restore(
        views: Vec<FolderView>,
        active: ViewId,
        visible: bool,
    ) -> Result<Self, &'static str> {
        if views.is_empty() || views.len() > Workspace::MAX_TABS {
            return Err("tab count is outside its limit");
        }
        let active = views
            .iter()
            .position(|view| view.view == active)
            .ok_or("active tab is missing")?;
        Ok(Self {
            views,
            active,
            visible,
        })
    }
    pub fn views(&self) -> &[FolderView] {
        &self.views
    }
    pub fn visible(&self) -> bool {
        self.visible
    }
    pub fn active_view(&self) -> &FolderView {
        &self.views[self.active]
    }
}

impl FolderBox {
    pub fn has_visible_tabs(&self) -> bool {
        self.tabs.as_ref().is_some_and(Tabs::visible)
    }
    pub fn default_folder(&self) -> &std::path::Path {
        self.standalone.navigation.start()
    }
    pub(super) fn configure_default_folder(&mut self, path: std::path::PathBuf) {
        let old_default = self.standalone.navigation.start().to_path_buf();
        let standalone_prev = self.standalone.navigation.current().to_path_buf();
        self.standalone.navigation = Navigation::new(path.clone());
        if self.standalone.navigation.current() != standalone_prev {
            self.standalone.top_item = 0;
        }
        if let Some(tabs) = &mut self.tabs {
            for tab in &mut tabs.views {
                let untouched_at_old_default = tab.navigation.current() == old_default
                    && tab.navigation.back_history().is_empty()
                    && tab.navigation.forward_history().is_empty()
                    && tab.navigation.scope().kind() == crate::domain::ScopeKind::Free;
                if untouched_at_old_default {
                    let prev_current = tab.navigation.current().to_path_buf();
                    tab.navigation = Navigation::new(path.clone());
                    if tab.navigation.current() != prev_current {
                        tab.top_item = 0;
                    }
                } else {
                    tab.navigation.set_default_start(path.clone());
                }
            }
        }
    }
    pub fn active_view(&self) -> &FolderView {
        match self.tabs.as_ref().filter(|tabs| tabs.visible) {
            Some(tabs) => tabs.active_view(),
            None => &self.standalone,
        }
    }
    pub fn views(&self) -> impl Iterator<Item = &FolderView> {
        std::iter::once(&self.standalone).chain(self.tabs.iter().flat_map(|tabs| &tabs.views))
    }
    pub fn visible_views(&self) -> impl Iterator<Item = &FolderView> {
        let (standalone, tabs) = match self.tabs.as_ref().filter(|tabs| tabs.visible) {
            Some(tabs) => (None, Some(tabs.views.iter())),
            None => (Some(&self.standalone), None),
        };
        standalone.into_iter().chain(tabs.into_iter().flatten())
    }
    pub(super) fn views_mut(&mut self) -> impl Iterator<Item = &mut FolderView> {
        std::iter::once(&mut self.standalone)
            .chain(self.tabs.iter_mut().flat_map(|tabs| &mut tabs.views))
    }
    pub fn view(&self, id: ViewId) -> Option<&FolderView> {
        self.views().find(|view| view.view == id)
    }
    pub(super) fn view_mut(&mut self, id: ViewId) -> Option<&mut FolderView> {
        if self.standalone.view == id {
            return Some(&mut self.standalone);
        }
        self.tabs
            .as_mut()?
            .views
            .iter_mut()
            .find(|view| view.view == id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabCommand {
    Visible(bool),
    Add,
    Activate(ViewId),
    Close(ViewId),
    Reorder { view: ViewId, position: usize },
}

impl Workspace {
    pub fn box_for_view(&self, view: ViewId) -> Option<&FolderBox> {
        self.boxes.iter().find(|item| item.view(view).is_some())
    }
    pub fn can_change_tabs(&self, id: BoxId, command: TabCommand) -> bool {
        self.boxes.iter().any(|item| item.id == id)
            && match command {
                TabCommand::Visible(_) => self.configuring(id),
                TabCommand::Reorder { .. } => self.mode == Mode::Layout,
                _ => self.mode == Mode::Browse || self.configuring(id),
            }
    }
    pub fn same_durable_state(&self, other: &Self) -> bool {
        self.boxes == other.boxes
            && self.launchers == other.launchers
            && self.next_identity == other.next_identity
    }

    /// Validate and clone a proposed saved state, without consuming identities or Undo.
    pub fn propose_tabs(
        &self,
        id: BoxId,
        command: TabCommand,
    ) -> Result<Option<Self>, &'static str> {
        let index = self
            .boxes
            .iter()
            .position(|item| item.id == id)
            .ok_or("box no longer exists")?;
        if !self.can_change_tabs(id, command) {
            return Err("tab action is unavailable in this mode");
        }
        let mut item = self.boxes[index].clone();
        let mut identity = self.next_identity;
        let changed = match command {
            TabCommand::Visible(visible) => {
                if let Some(tabs) = &mut item.tabs {
                    if tabs.visible == visible {
                        false
                    } else {
                        tabs.visible = visible;
                        true
                    }
                } else if visible {
                    self.admit_view(&mut identity)?;
                    let mut view = item.standalone.clone();
                    view.view = ViewId(identity);
                    item.tabs = Some(Tabs {
                        views: vec![view],
                        active: 0,
                        visible: true,
                    });
                    true
                } else {
                    false
                }
            }
            other => Self::change_tab(&mut item, other, &mut identity, self.all_views().count())?,
        };
        if !changed {
            return Ok(None);
        }
        let mut proposal = self.snapshot();
        proposal.boxes[index] = item;
        proposal.next_identity = identity;
        Ok(Some(proposal))
    }

    fn admit_view(&self, identity: &mut u64) -> Result<(), &'static str> {
        if self.all_views().count() >= Self::MAX_VIEWS {
            return Err("workspace view limit reached");
        }
        *identity = identity
            .checked_add(1)
            .ok_or("view identities are exhausted")?;
        Ok(())
    }

    fn change_tab(
        item: &mut FolderBox,
        command: TabCommand,
        identity: &mut u64,
        count: usize,
    ) -> Result<bool, &'static str> {
        let default_start = item.standalone.navigation.start().to_path_buf();
        let tabs = item
            .tabs
            .as_mut()
            .filter(|tabs| tabs.visible)
            .ok_or("vertical tabs are not enabled")?;
        match command {
            TabCommand::Add => {
                if tabs.views.len() >= Self::MAX_TABS {
                    return Err("box tab limit reached");
                }
                if count >= Self::MAX_VIEWS {
                    return Err("workspace view limit reached");
                }
                *identity = identity
                    .checked_add(1)
                    .ok_or("view identities are exhausted")?;
                let navigation = Navigation::new(default_start);
                tabs.views
                    .push(FolderView::new(ViewId(*identity), navigation));
                tabs.active = tabs.views.len() - 1;
                Ok(true)
            }
            TabCommand::Activate(view) => {
                let index = tabs
                    .views
                    .iter()
                    .position(|item| item.view == view)
                    .ok_or("tab no longer exists")?;
                if index == tabs.active {
                    return Ok(false);
                }
                tabs.active = index;
                Ok(true)
            }
            TabCommand::Close(view) => {
                let index = tabs
                    .views
                    .iter()
                    .position(|item| item.view == view)
                    .ok_or("tab no longer exists")?;
                if tabs.views.len() == 1 {
                    return Err("the last tab cannot be closed");
                }
                let active = tabs.active_view().view;
                tabs.views.remove(index);
                tabs.active = if active == view {
                    index.min(tabs.views.len() - 1)
                } else {
                    tabs.views
                        .iter()
                        .position(|item| item.view == active)
                        .ok_or("active tab is missing")?
                };
                Ok(true)
            }
            TabCommand::Reorder { view, position } => {
                let index = tabs
                    .views
                    .iter()
                    .position(|item| item.view == view)
                    .ok_or("tab no longer exists")?;
                if position >= tabs.views.len() {
                    return Err("tab drop is outside the rail");
                }
                if index == position {
                    return Ok(false);
                }
                let active = tabs.active_view().view;
                let view = tabs.views.remove(index);
                tabs.views.insert(position, view);
                tabs.active = tabs
                    .views
                    .iter()
                    .position(|item| item.view == active)
                    .ok_or("active tab is missing")?;
                Ok(true)
            }
            TabCommand::Visible(_) => Err("invalid tab command"),
        }
    }

    /// Only the application may accept a current, budget-checked proposal.
    pub(crate) fn accept_tab_proposal(&mut self, mut proposal: Self) {
        self.remember();
        super::scroll::carry_proposal_scroll(&self.boxes, &mut proposal.boxes);
        self.boxes = proposal.boxes;
        self.next_identity = proposal.next_identity;
        self.gesture = None;
    }
}
