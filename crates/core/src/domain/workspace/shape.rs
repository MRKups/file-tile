//! Pure grid-shape classification and resize bounds for folder boxes and shortcut tiles.

use super::{GridDelta, ResizeHandle};
use crate::domain::{GridExtent, GridRect};

const MAX_LAUNCHER_SPAN: u16 = 2;
const MIN_FOLDER_BOX_SHORT_SIDE: u16 = 2;
const MIN_FOLDER_BOX_LONG_SIDE: u16 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    FolderBox,
    Launcher,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawRejection {
    Overlap,
    ThinStrip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawPreview {
    pub rect: GridRect,
    pub outcome: Result<ItemKind, DrawRejection>,
}

impl DrawPreview {
    pub const fn is_valid(self) -> bool {
        self.outcome.is_ok()
    }
}

pub(super) const fn is_launcher_shape(rect: GridRect) -> bool {
    rect.width >= 1
        && rect.width <= MAX_LAUNCHER_SPAN
        && rect.height >= 1
        && rect.height <= MAX_LAUNCHER_SPAN
}

const fn is_folder_box_shape(rect: GridRect) -> bool {
    rect.width >= MIN_FOLDER_BOX_SHORT_SIDE
        && rect.height >= MIN_FOLDER_BOX_SHORT_SIDE
        && (rect.width >= MIN_FOLDER_BOX_LONG_SIDE || rect.height >= MIN_FOLDER_BOX_LONG_SIDE)
}

const fn is_thin_strip(rect: GridRect) -> bool {
    (rect.width == 1 && rect.height > MAX_LAUNCHER_SPAN)
        || (rect.height == 1 && rect.width > MAX_LAUNCHER_SPAN)
}

pub(super) const fn classify_draw(
    rect: GridRect,
    automatic: bool,
) -> Result<ItemKind, DrawRejection> {
    if rect.width == 0 || rect.height == 0 || is_thin_strip(rect) {
        return Err(DrawRejection::ThinStrip);
    }
    if automatic && is_launcher_shape(rect) {
        Ok(ItemKind::Launcher)
    } else {
        Ok(ItemKind::FolderBox)
    }
}

/// Validates a single resize step while allowing legacy saved small boxes to expand.
pub(super) const fn allows_resize(kind: ItemKind, previous: GridRect, candidate: GridRect) -> bool {
    if candidate.width == 0 || candidate.height == 0 {
        return false;
    }
    match kind {
        ItemKind::Launcher => is_launcher_shape(candidate),
        ItemKind::FolderBox => {
            if is_folder_box_shape(candidate) {
                return true;
            }
            !is_folder_box_shape(previous)
                && !is_thin_strip(candidate)
                && candidate.width >= previous.width
                && candidate.height >= previous.height
        }
    }
}

pub(super) fn resize_target(
    kind: ItemKind,
    original: GridRect,
    handle: ResizeHandle,
    delta: GridDelta,
    extent: GridExtent,
) -> [i64; 4] {
    let [left, top, right, bottom] = [
        original.origin.column,
        original.origin.row,
        original.right(),
        original.bottom(),
    ]
    .map(i64::from);
    let (min_w, max_w, min_h, max_h) = match kind {
        ItemKind::Launcher => {
            let max = i64::from(MAX_LAUNCHER_SPAN);
            (1, max, 1, max)
        }
        ItemKind::FolderBox => {
            let max = i64::from(GridExtent::MAX_DIMENSION);
            let mut min_w = i64::from(original.width.min(MIN_FOLDER_BOX_SHORT_SIDE));
            let mut min_h = i64::from(original.height.min(MIN_FOLDER_BOX_SHORT_SIDE));
            if is_folder_box_shape(original) {
                let horizontal_only = !handle.north() && !handle.south();
                let vertical_only = !handle.west() && !handle.east();
                if horizontal_only && original.height < MIN_FOLDER_BOX_LONG_SIDE {
                    min_w = i64::from(MIN_FOLDER_BOX_LONG_SIDE);
                }
                if vertical_only && original.width < MIN_FOLDER_BOX_LONG_SIDE {
                    min_h = i64::from(MIN_FOLDER_BOX_LONG_SIDE);
                }
            }
            (min_w, max, min_h, max)
        }
    };
    let max_col = i64::from(extent.columns());
    let max_row = i64::from(extent.rows());
    [
        if handle.west() {
            let lo = (right - max_w).max(0);
            let hi = (right - min_w).max(lo);
            (left + i64::from(delta.columns)).max(lo).min(hi)
        } else {
            left
        },
        if handle.north() {
            let lo = (bottom - max_h).max(0);
            let hi = (bottom - min_h).max(lo);
            (top + i64::from(delta.rows)).max(lo).min(hi)
        } else {
            top
        },
        if handle.east() {
            let lo = left + min_w;
            let hi = (left + max_w).min(max_col).max(lo);
            (right + i64::from(delta.columns)).max(lo).min(hi)
        } else {
            right
        },
        if handle.south() {
            let lo = top + min_h;
            let hi = (top + max_h).min(max_row).max(lo);
            (bottom + i64::from(delta.rows)).max(lo).min(hi)
        } else {
            bottom
        },
    ]
}
