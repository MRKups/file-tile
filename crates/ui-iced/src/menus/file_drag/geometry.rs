//! Screen-space source-box item dimensions and pointer grab offsets for lifted drag previews.

use std::path::{Path, PathBuf};

use iced::{Point, Rectangle, Size, Vector};

use crate::domain::{FileView, FolderBox, ViewId};
use crate::{
    Desktop,
    boxes::{file_columns, file_grid, file_list, folder_box, navigation_header, tab_rail},
};

#[derive(Debug, Clone, Copy)]
pub(super) struct SourceLayout {
    pub item_size: Size,
    pub grab_offset: Vector,
    pub pressed_slot: usize,
}

pub(super) fn measure_source(
    desktop: &Desktop,
    view: ViewId,
    pressed_path: &Path,
    selected_paths: &[PathBuf],
    origin: Point,
) -> SourceLayout {
    let geometry = desktop.geometry();
    let ty = geometry.typography;
    let pressed_slot = selected_paths
        .iter()
        .position(|path| path == pressed_path)
        .unwrap_or(0);
    let fallback_h = ty.row(file_list::ROW_HEIGHT);
    let fallback = SourceLayout {
        item_size: Size::new(220.0, fallback_h),
        grab_offset: Vector::new(24.0, fallback_h * 0.5),
        pressed_slot,
    };

    let Some(box_item) = desktop
        .application
        .workspace()
        .boxes()
        .iter()
        .find(|item| item.active_view().view == view)
    else {
        return fallback;
    };

    let primary_index = desktop
        .application
        .ordered_entries(view)
        .position(|entry| entry.path == pressed_path);
    let default_sy = desktop
        .views
        .get(&view)
        .map_or(0.0, |state| state.scroll.offset);
    let (index, scroll_y, right_preview) = if let Some(idx) = primary_index {
        (idx, default_sy, false)
    } else if let Some(idx) = desktop.column_previews.entry_index(view, pressed_path) {
        (idx, desktop.column_previews.scroll_y(view), true)
    } else {
        (0, default_sy, false)
    };
    let scroll_x = desktop.views.get(&view).map_or(0.0, |state| state.scroll_x);

    let raw_rect = geometry.rect(box_item.rect);
    let box_bounds = Rectangle {
        x: raw_rect.x + geometry.viewport.x - geometry.offset.x,
        y: raw_rect.y + geometry.viewport.y - geometry.offset.y,
        ..raw_rect
    };

    let row = SourceRow {
        index,
        scroll_x,
        scroll_y,
        right_preview,
    };
    let item_rect = item_bounds(box_item, box_bounds, row, ty);
    let grab_offset = Vector::new(
        (origin.x - item_rect.x).clamp(0.0, item_rect.width),
        (origin.y - item_rect.y).clamp(0.0, item_rect.height),
    );
    SourceLayout {
        item_size: item_rect.size(),
        grab_offset,
        pressed_slot,
    }
}

#[derive(Clone, Copy)]
struct SourceRow {
    index: usize,
    scroll_x: f32,
    scroll_y: f32,
    right_preview: bool,
}

fn item_bounds(
    box_item: &FolderBox,
    box_bounds: Rectangle,
    row: SourceRow,
    ty: crate::window::typography::Style,
) -> Rectangle {
    let settings = box_item.settings;
    let rail_width = tab_rail::width(box_item, box_bounds.width);
    let pane_x = box_bounds.x + rail_width - row.scroll_x;
    let pane_width = (box_bounds.width - rail_width).max(1.0);
    let compact = folder_box::is_compact(pane_width, box_bounds.height, settings, ty)
        || pane_width < navigation_header::minimum_width(settings);
    let content_width = if compact {
        pane_width.max(navigation_header::minimum_width(settings))
    } else {
        pane_width
    };
    let header_h = navigation_header::height(content_width, settings, ty);

    match settings.file_view {
        FileView::List => {
            let item_h = ty.row(file_list::ROW_HEIGHT);
            let item_w = (content_width - 18.0).max(80.0);
            Rectangle {
                x: pane_x + 9.0,
                y: box_bounds.y + header_h + row.index as f32 * item_h - row.scroll_y,
                width: item_w,
                height: item_h,
            }
        }
        FileView::Columns => {
            let two_cols = content_width > file_columns::TWO_COLUMN_MIN_WIDTH;
            let half_w = ((content_width - 1.0) * 0.5).max(1.0);
            let (col_left, col_width) = if two_cols && row.right_preview {
                (half_w + 1.0, half_w)
            } else if two_cols {
                (0.0, half_w)
            } else {
                (0.0, content_width)
            };
            let col_header_h = if compact {
                0.0
            } else {
                file_columns::header_height(ty)
            };
            let item_h = ty.row(file_list::ROW_HEIGHT);
            let item_w = (col_width - 16.0).max(60.0);
            Rectangle {
                x: pane_x + col_left + 8.0,
                y: box_bounds.y + header_h + col_header_h + row.index as f32 * item_h
                    - row.scroll_y,
                width: item_w,
                height: item_h,
            }
        }
        FileView::Grid => {
            let cols = file_grid::columns(content_width, ty);
            let pitch_h = ty.row(file_grid::TILE_HEIGHT);
            let item_w = ((content_width - 18.0 - 6.0 * (cols - 1) as f32) / cols as f32).max(64.0);
            let item_h = (pitch_h - 6.0).max(40.0);
            let col_idx = row.index % cols;
            let row_idx = row.index / cols;
            Rectangle {
                x: pane_x + 9.0 + col_idx as f32 * (item_w + 6.0),
                y: box_bounds.y + header_h + row_idx as f32 * pitch_h - row.scroll_y,
                width: item_w,
                height: item_h,
            }
        }
    }
}
