use std::path::PathBuf;

mod configuration;
mod editing;
mod launchers;
mod shape;
mod tabs;
pub use configuration::{BoxSetting, BoxSettings, FileView};
pub use launchers::{ItemPlacement, LauncherConfig, LauncherIcon, LauncherTarget, LauncherTile};
pub use shape::{DrawPreview, DrawRejection, ItemKind};
pub use tabs::{FolderView, TabCommand, Tabs, ViewRef};
mod undo;
use editing::LayoutGesture;
pub use editing::{GridDelta, LayoutKind, LayoutOutcome, LayoutPreview, ResizeHandle};
use undo::UndoHistory;
pub use undo::UndoOutcome;

use super::{Cell, GridExtent, GridRect, Navigation};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BoxId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ViewId(pub u64);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Browse,
    Layout,
    Configure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderBox {
    pub id: BoxId,
    pub rect: GridRect,
    pub settings: BoxSettings,
    pub standalone: FolderView,
    pub tabs: Option<Tabs>,
}

#[derive(Debug, Clone, Copy)]
enum DrawKind {
    Folder,
    Automatic,
}

#[derive(Debug, Clone, Copy)]
struct DrawGesture {
    kind: DrawKind,
    first: Cell,
    last: Cell,
    extent: GridExtent,
}

#[derive(Debug, Clone)]
enum Gesture {
    Draw(DrawGesture),
    // Keep the saved workspace handoff and tagged application messages compact.
    Layout(Box<LayoutGesture>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawOutcome {
    None,
    Invalid(DrawRejection),
    Created(ViewId),
    CreatedLauncher(BoxId),
    IdentityExhausted,
    CapacityReached,
    ViewCapacityReached,
}

#[derive(Debug, Clone, Default)]
pub struct Workspace {
    boxes: Vec<FolderBox>,
    launchers: Vec<LauncherTile>,
    mode: Mode,
    selected: Option<BoxId>,
    gesture: Option<Gesture>,
    next_identity: u64,
    // Allocate history only when a committed user change needs it. Saved
    // handoffs have no history allocation and stay small enough for UI messages.
    undo: Option<Box<UndoHistory>>,
}

impl Workspace {
    /// Shared capacity for folder boxes and shortcut tiles.
    pub const MAX_BOXES: usize = 256;
    pub const MAX_TABS: usize = 16;
    pub const MAX_VIEWS: usize = 1024;

    /// Immutable durable handoff, without gesture, mode, or selection state.
    pub fn snapshot(&self) -> Self {
        Self {
            boxes: self.boxes.clone(),
            launchers: self.launchers.clone(),
            next_identity: self.next_identity,
            ..Self::default()
        }
    }

    /// Build restored state through the same geometry and identity boundary.
    /// Callers construct a fresh workspace, so restoration has no active gesture.
    pub(crate) fn restore_box(&mut self, item: FolderBox) -> Result<(), &'static str> {
        if self.item_count() >= Self::MAX_BOXES
            || self
                .launchers
                .iter()
                .any(|other| other.id == item.id || other.rect.intersects(item.rect))
        {
            return Err("box conflicts with a launcher or exceeds the shared item limit");
        }
        Self::validate_box(&item, &self.boxes)?;
        self.next_identity = item
            .views()
            .fold(self.next_identity.max(item.id.0), |maximum, view| {
                maximum.max(view.view.0)
            });
        self.boxes.push(item);
        Ok(())
    }

    pub(crate) fn validate_saved(&self) -> Result<(), &'static str> {
        for (index, item) in self.boxes.iter().enumerate() {
            Self::validate_box(item, &self.boxes[..index])?;
        }
        self.validate_launchers()
    }

    fn validate_box(item: &FolderBox, previous: &[FolderBox]) -> Result<(), &'static str> {
        if previous.len() >= Self::MAX_BOXES {
            return Err("workspace box limit exceeded");
        }
        if item.id.0 == 0 || item.views().any(|view| view.view.0 == 0) {
            return Err("box and view identities must be positive");
        }
        if previous.iter().any(|old| old.id == item.id) {
            return Err("duplicate box or view identity");
        }
        let mut identities: std::collections::HashSet<_> = previous
            .iter()
            .flat_map(FolderBox::views)
            .map(|view| view.view)
            .collect();
        for view in item.views() {
            if !view.navigation.scope().allows(view.navigation.current()) {
                return Err("current folder is outside its scope");
            }
            if !identities.insert(view.view) {
                return Err("duplicate view identity");
            }
        }
        if identities.len() > Self::MAX_VIEWS {
            return Err("workspace view limit exceeded");
        }
        let rect = item.rect;
        let limit = u32::from(GridExtent::MAX_DIMENSION);
        if rect.width == 0
            || rect.height == 0
            || u32::from(rect.origin.column) + u32::from(rect.width) > limit
            || u32::from(rect.origin.row) + u32::from(rect.height) > limit
        {
            return Err("box bounds are outside the canvas");
        }
        if previous.iter().any(|old| old.rect.intersects(rect)) {
            return Err("box overlaps an earlier restored box");
        }
        Ok(())
    }

    pub fn boxes(&self) -> &[FolderBox] {
        &self.boxes
    }
    #[cfg(test)]
    pub(crate) fn box_mut(&mut self, id: BoxId) -> Option<&mut FolderBox> {
        self.boxes.iter_mut().find(|item| item.id == id)
    }
    pub fn mode(&self) -> Mode {
        self.mode
    }
    pub fn selected(&self) -> Option<BoxId> {
        self.selected
    }
    pub fn view(&self, id: ViewId) -> Option<ViewRef<'_>> {
        self.boxes.iter().find_map(|item| {
            item.view(id).map(|view| ViewRef {
                id: item.id,
                view: view.view,
                navigation: &view.navigation,
                settings: item.settings,
            })
        })
    }
    pub fn view_mut(&mut self, id: ViewId) -> Option<&mut FolderView> {
        self.boxes.iter_mut().find_map(|item| item.view_mut(id))
    }
    pub fn active_views(&self) -> impl Iterator<Item = &FolderView> {
        self.boxes.iter().map(FolderBox::active_view)
    }
    pub fn all_views(&self) -> impl Iterator<Item = &FolderView> {
        self.boxes.iter().flat_map(FolderBox::views)
    }
    pub fn visible_views(&self) -> impl Iterator<Item = &FolderView> {
        self.boxes.iter().flat_map(FolderBox::visible_views)
    }
    pub fn prune_removed_path(&mut self, source: &std::path::Path) {
        for view in self.boxes.iter_mut().flat_map(FolderBox::views_mut) {
            view.navigation.prune_removed_path(source);
        }
    }
    pub fn is_active(&self, view: ViewId) -> bool {
        self.active_views().any(|item| item.view == view)
    }

    pub fn select_mode(&mut self, mode: Mode) {
        self.mode = if mode != Mode::Browse && self.mode == mode {
            Mode::Browse
        } else {
            mode
        };
        self.gesture = None;
        if self.mode == Mode::Configure {
            self.selected = None;
        }
    }

    pub fn escape(&mut self) {
        self.gesture = None;
        if self.mode == Mode::Configure && self.selected.is_some() {
            self.selected = None;
        } else {
            self.mode = Mode::Browse;
        }
    }
    pub fn cancel_pointer(&mut self) {
        self.gesture = None;
    }

    /// Explicit folder drawing. Normal workspace input uses size-based item drawing.
    pub fn begin_draw(&mut self, cell: Cell, extent: GridExtent) {
        self.begin_drawing(cell, extent, DrawKind::Folder);
    }
    pub fn begin_item_draw(&mut self, cell: Cell, extent: GridExtent) {
        self.begin_drawing(cell, extent, DrawKind::Automatic);
    }
    fn begin_drawing(&mut self, cell: Cell, extent: GridExtent, kind: DrawKind) {
        if self.mode != Mode::Layout {
            return;
        }
        let cell = extent.clamp(cell);
        let hit = self.placements().find(|item| item.rect.contains(cell));
        if let Some(item) = hit {
            self.selected = Some(item.id);
            self.gesture = None;
        } else {
            self.selected = None;
            self.gesture = Some(Gesture::Draw(DrawGesture {
                kind,
                first: cell,
                last: cell,
                extent,
            }));
        }
    }

    pub fn draw_to(&mut self, cell: Cell) {
        if let Some(Gesture::Draw(gesture)) = &mut self.gesture {
            gesture.last = gesture.extent.clamp(cell);
        }
    }

    pub fn draw_preview(&self) -> Option<DrawPreview> {
        let Some(Gesture::Draw(gesture)) = self.gesture.as_ref() else {
            return None;
        };
        let rect = GridRect::between(gesture.first, gesture.last, gesture.extent);
        let overlaps = self.placements().any(|item| item.rect.intersects(rect));
        let outcome = if overlaps {
            Err(DrawRejection::Overlap)
        } else {
            shape::classify_draw(rect, matches!(gesture.kind, DrawKind::Automatic))
        };
        Some(DrawPreview { rect, outcome })
    }

    pub fn finish_draw(&mut self, start: PathBuf) -> DrawOutcome {
        if self.mode != Mode::Layout || !matches!(self.gesture, Some(Gesture::Draw(_))) {
            return DrawOutcome::None;
        }
        let preview = self.draw_preview();
        self.gesture = None;
        match preview {
            None => DrawOutcome::None,
            Some(DrawPreview {
                outcome: Err(rejection),
                ..
            }) => DrawOutcome::Invalid(rejection),
            Some(DrawPreview {
                rect,
                outcome: Ok(kind),
            }) => {
                if self.item_count() >= Self::MAX_BOXES {
                    return DrawOutcome::CapacityReached;
                }
                if kind == ItemKind::FolderBox && self.all_views().count() >= Self::MAX_VIEWS {
                    return DrawOutcome::ViewCapacityReached;
                }
                let Some(identity) = self.next_identity.checked_add(1) else {
                    return DrawOutcome::IdentityExhausted;
                };
                self.next_identity = identity;
                self.remember();
                let id = BoxId(identity);
                match kind {
                    ItemKind::Launcher => {
                        self.launchers.push(LauncherTile {
                            id,
                            rect,
                            config: LauncherConfig::default(),
                        });
                        self.selected = Some(id);
                        DrawOutcome::CreatedLauncher(id)
                    }
                    ItemKind::FolderBox => {
                        let view = ViewId(identity);
                        self.boxes.push(FolderBox {
                            id,
                            rect,
                            settings: BoxSettings::default(),
                            standalone: FolderView {
                                view,
                                navigation: Navigation::new(start),
                            },
                            tabs: None,
                        });
                        self.selected = Some(id);
                        DrawOutcome::Created(view)
                    }
                }
            }
        }
    }
}
