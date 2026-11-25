//! Pure ordinal item-anchor conversion and per-view scroll synchronization.

use std::collections::HashMap;
use std::num::NonZeroU32;

use iced::{Task, widget::operation};

use crate::application::preferences::typography::TypographyPreferences;
use crate::application::{Action, LoadState};
use crate::boxes::{file_grid, file_list, folder_box};
use crate::domain::{BoxId, BoxSettings, FileView, ViewId};
use crate::window::typography;
use crate::{Desktop, Message, ViewPresentation};

const BOUNDARY_EPSILON: f32 = 1e-2;

struct RescaleTarget {
    view: ViewId,
    saved_top_item: u32,
    settings: BoxSettings,
    old_scroller: folder_box::ScrollerGeometry,
    new_scroller: folder_box::ScrollerGeometry,
}

fn metrics(
    scroller: folder_box::ScrollerGeometry,
    layout: FileView,
    ty: typography::Style,
) -> (usize, f32) {
    if layout == FileView::Grid {
        (
            file_grid::columns(scroller.content_width, ty),
            ty.row(file_grid::TILE_HEIGHT),
        )
    } else {
        (1, ty.row(file_list::ROW_HEIGHT))
    }
}

pub(crate) fn max_scroll_offset(
    scroller: folder_box::ScrollerGeometry,
    layout: FileView,
    visible_count: usize,
    ty: typography::Style,
) -> f32 {
    if visible_count == 0 {
        return 0.0;
    }
    let (cols, pitch) = metrics(scroller, layout, ty);
    let total_rows = visible_count.div_ceil(cols);
    let raw_content_h = total_rows as f32 * pitch;
    let content_h = if scroller.compact {
        raw_content_h + scroller.nav_height + scroller.footer_height
    } else {
        raw_content_h
    };
    (content_h - scroller.viewport_height).max(0.0)
}

pub(crate) fn offset_to_top_item(
    offset_y: f32,
    scroller: folder_box::ScrollerGeometry,
    layout: FileView,
    visible_count: usize,
    ty: typography::Style,
) -> u32 {
    if visible_count == 0 || !offset_y.is_finite() {
        return 0;
    }
    let (cols, pitch) = metrics(scroller, layout, ty);
    let adjusted_y = if scroller.compact {
        (offset_y - scroller.nav_height).max(0.0)
    } else {
        offset_y.max(0.0)
    };
    let row_idx = ((adjusted_y + BOUNDARY_EPSILON) / pitch).floor().max(0.0) as usize;
    let item_idx = row_idx.saturating_mul(cols).min(visible_count - 1);
    u32::try_from(item_idx).unwrap_or(u32::MAX)
}

pub(crate) fn top_item_to_offset(
    top_item: u32,
    scroller: folder_box::ScrollerGeometry,
    layout: FileView,
    visible_count: usize,
    ty: typography::Style,
) -> f32 {
    if visible_count == 0 || top_item == 0 {
        return 0.0;
    }
    let (cols, pitch) = metrics(scroller, layout, ty);
    let clamped_item = (top_item as usize).min(visible_count - 1);
    let row_idx = clamped_item / cols;
    let max_y = max_scroll_offset(scroller, layout, visible_count, ty);
    let raw_y = if scroller.compact {
        row_idx as f32 * pitch + scroller.nav_height
    } else {
        row_idx as f32 * pitch
    };
    raw_y.clamp(0.0, max_y).round()
}

impl Desktop {
    pub(crate) fn seed_missing_view_presentations(&mut self) {
        let missing: Vec<(ViewId, u32)> = self
            .application
            .workspace()
            .all_views()
            .filter(|item| !self.views.contains_key(&item.view))
            .map(|item| (item.view, item.top_item))
            .collect();
        for (view, top_item) in missing {
            self.views.insert(
                view,
                ViewPresentation {
                    pending_scroll_restore: NonZeroU32::new(top_item),
                    ..ViewPresentation::default()
                },
            );
        }
    }

    pub(crate) fn apply_pending_active_restores(&mut self) -> Task<Message> {
        let active: Vec<ViewId> = self
            .application
            .workspace()
            .active_views()
            .map(|item| item.view)
            .collect();
        Task::batch(
            active
                .into_iter()
                .map(|view| self.apply_pending_scroll_restore(view)),
        )
    }

    pub(crate) fn reseed_unloaded_scroll(&mut self, view: ViewId, saved_top_item: u32) {
        let state = self.views.entry(view).or_default();
        state.pending_scroll_restore = NonZeroU32::new(saved_top_item);
        state.scroll = file_list::ScrollWindow::default();
        state.scroll_x = 0.0;
    }

    pub(crate) fn rescale_box_scrollers(
        &mut self,
        scrollers_before: &HashMap<BoxId, (folder_box::ScrollerGeometry, BoxSettings)>,
        old_typography: TypographyPreferences,
    ) -> Task<Message> {
        let geometry = self.geometry();
        let new_ty = geometry.typography;
        let old_ty = typography::Style(old_typography);
        let scale_changed = old_typography.size != new_ty.0.size;
        if scale_changed {
            let unloaded: Vec<(ViewId, u32)> = self
                .application
                .workspace()
                .all_views()
                .filter(|item| {
                    self.application
                        .load(item.view)
                        .and_then(LoadState::listing)
                        .is_none()
                })
                .map(|item| (item.view, item.top_item))
                .collect();
            for (view, top_item) in unloaded {
                self.reseed_unloaded_scroll(view, top_item);
            }
        }
        let mut inactive_changed = Vec::new();
        let mut targets = Vec::new();
        for item in self.application.workspace().boxes() {
            let Some(&(old_scroller, old_settings)) = scrollers_before.get(&item.id) else {
                continue;
            };
            let comparable =
                folder_box::scroller_geometry_with_settings(geometry, item, old_settings);
            let box_changed = scale_changed
                || (old_scroller.compact != comparable.compact)
                || (old_scroller.content_width - comparable.content_width).abs() > f32::EPSILON
                || (old_scroller.viewport_height - comparable.viewport_height).abs() > f32::EPSILON
                || (old_scroller.nav_height - comparable.nav_height).abs() > f32::EPSILON;
            if !box_changed {
                continue;
            }
            let active = item.active_view();
            for tab in item.views().filter(|tab| tab.view != active.view) {
                inactive_changed.push((tab.view, tab.top_item));
            }
            targets.push(RescaleTarget {
                view: active.view,
                saved_top_item: active.top_item,
                settings: item.settings,
                old_scroller,
                new_scroller: folder_box::scroller_geometry(geometry, item),
            });
        }
        for (view, top_item) in inactive_changed {
            self.reseed_unloaded_scroll(view, top_item);
        }
        let mut tasks = Vec::new();
        for target in targets {
            tasks.push(self.rescale_single_active_scroller(target, old_ty, new_ty, scale_changed));
        }
        Task::batch(tasks)
    }

    fn rescale_single_active_scroller(
        &mut self,
        target: RescaleTarget,
        old_ty: typography::Style,
        new_ty: typography::Style,
        scale_changed: bool,
    ) -> Task<Message> {
        let Some(listing) = self
            .application
            .load(target.view)
            .and_then(LoadState::listing)
        else {
            self.reseed_unloaded_scroll(target.view, target.saved_top_item);
            return Task::none();
        };
        let visible_count = listing.display_len(target.settings.show_hidden);
        let Some(state) = self.views.get_mut(&target.view) else {
            return Task::none();
        };
        if state.pending_scroll_restore.is_some() {
            state.scroll.height = target.new_scroller.viewport_height;
            return Task::none();
        }
        let grid_reflow = target.settings.file_view == FileView::Grid
            && file_grid::columns(target.old_scroller.content_width, old_ty)
                != file_grid::columns(target.new_scroller.content_width, new_ty);
        let needs_reanchor = scale_changed
            || grid_reflow
            || target.old_scroller.compact != target.new_scroller.compact
            || (target.new_scroller.compact
                && target.old_scroller.nav_height != target.new_scroller.nav_height);
        let revision = state.listing_revision;
        if needs_reanchor {
            let index = offset_to_top_item(
                state.scroll.offset,
                target.old_scroller,
                target.settings.file_view,
                visible_count,
                old_ty,
            );
            let y = top_item_to_offset(
                index,
                target.new_scroller,
                target.settings.file_view,
                visible_count,
                new_ty,
            );
            state.scroll = file_list::ScrollWindow {
                offset: y,
                height: target.new_scroller.viewport_height,
            };
            self.sync_view_top_item(target.view);
            operation::scroll_to(
                file_list::scroll_id(target.view),
                operation::AbsoluteOffset { x: 0.0, y },
            )
            .chain(file_list::observe_scroll(target.view, revision))
        } else {
            let max_y = max_scroll_offset(
                target.new_scroller,
                target.settings.file_view,
                visible_count,
                new_ty,
            );
            state.scroll.offset = state.scroll.offset.clamp(0.0, max_y);
            state.scroll.height = target.new_scroller.viewport_height;
            self.sync_view_top_item(target.view);
            file_list::observe_scroll(target.view, revision)
        }
    }

    pub(crate) fn sync_view_top_item(&mut self, view: ViewId) {
        let Some(presentation) = self.views.get(&view) else {
            return;
        };
        if presentation.pending_scroll_restore.is_some() {
            return;
        }
        let Some(listing) = self.application.load(view).and_then(LoadState::listing) else {
            return;
        };
        let Some(item) = self.application.workspace().box_for_view(view) else {
            return;
        };
        let Some(view_ref) = self.application.workspace().view(view) else {
            return;
        };
        let geometry = self.geometry();
        let scroller = folder_box::scroller_geometry(geometry, item);
        let visible_count = listing.display_len(view_ref.settings.show_hidden);
        let next_top_item = offset_to_top_item(
            presentation.scroll.offset,
            scroller,
            view_ref.settings.file_view,
            visible_count,
            geometry.typography,
        );
        if next_top_item != view_ref.top_item {
            let _ = self
                .application
                .update(Action::SyncViewScroll(view, next_top_item));
        }
    }

    pub(crate) fn cancel_pending_scroll_restore(&mut self, view: ViewId) {
        if self
            .application
            .load(view)
            .and_then(LoadState::listing)
            .is_none()
        {
            return;
        }
        let canceled = self
            .views
            .get_mut(&view)
            .and_then(|state| state.pending_scroll_restore.take())
            .is_some();
        if canceled {
            self.sync_view_top_item(view);
        }
    }
}
