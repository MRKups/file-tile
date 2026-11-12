//! Drag-to-reorder gesture for top-section `Pinned Folders` rows in a box sidebar.

use std::path::PathBuf;

use iced::widget::{container, pin, space, stack, text};
use iced::{Border, Element, Point, Rectangle, Task};

use crate::application::Action;
use crate::domain::{BoxId, Mode};
use crate::{Desktop, Message, window::appearance as a};

use super::{
    END_INSET, ROW_HEIGHT, SECTION_DIVIDER_HEIGHT, SECTION_HEADER_HEIGHT, folder_display_name,
    insertion_marker, offset, rail_width,
};

const DRAG_THRESHOLD: f32 = 6.0;

pub(crate) struct Drag {
    pub id: BoxId,
    pub from: usize,
    pub path: PathBuf,
    pub origin: Point,
    pub pointer: Point,
    pub active: bool,
    target: Option<usize>,
    marker: Option<Rectangle>,
}

impl Drag {
    pub(crate) fn active_from(&self, id: BoxId) -> Option<usize> {
        (self.active && self.id == id).then_some(self.from)
    }
}

impl Desktop {
    pub(crate) fn cancel_sidebar_drag_for(&mut self, action: &Action) -> bool {
        if matches!(action, Action::Escape)
            && self.sidebar_drag.as_ref().is_some_and(|drag| drag.active)
        {
            self.sidebar_drag = None;
            return true;
        }
        if matches!(
            action,
            Action::SelectMode(_)
                | Action::Escape
                | Action::CancelPointer
                | Action::Undo
                | Action::RemoveBox(_)
                | Action::RemoveSelected
                | Action::ChangeTabs(..)
                | Action::RequestClose
                | Action::OpenAppSettings
                | Action::OpenWorkspaceManagement
                | Action::PinFolder { .. }
                | Action::UnpinFolder(_)
                | Action::ReorderPinnedFolder { .. }
                | Action::SetBoxSetting(..)
        ) {
            self.sidebar_drag = None;
        }
        false
    }

    pub(crate) fn press_sidebar_pin(
        &mut self,
        id: BoxId,
        index: usize,
        path: PathBuf,
    ) -> Task<Message> {
        self.sidebar_drag = None;
        if !self.workspace_input_enabled() {
            return Task::none();
        }
        let mode = self.application.workspace().mode();
        if mode == Mode::Configure && self.application.workspace().configuring(id) {
            return self.activate_sidebar_location(id, path);
        }
        if mode != Mode::Browse
            || self.application.pinned_folders().get(index) != Some(&path)
            || !self
                .application
                .workspace()
                .boxes()
                .iter()
                .any(|item| item.id == id && item.has_sidebar())
        {
            return Task::none();
        }
        let Some(origin) = self.pointer else {
            return self.activate_sidebar_location(id, path);
        };
        self.sidebar_drag = Some(Drag {
            id,
            from: index,
            path,
            origin,
            pointer: origin,
            active: false,
            target: None,
            marker: None,
        });
        Task::none()
    }

    pub(crate) fn move_sidebar_drag(&mut self, point: Point) -> bool {
        let geometry = self.geometry();
        let pinned_len = self.application.pinned_folders().len();
        let loc_len = self.application.locations().len();
        let box_item = self.sidebar_drag.as_ref().and_then(|drag| {
            self.application
                .workspace()
                .boxes()
                .iter()
                .find(|item| item.id == drag.id && item.has_sidebar())
                .cloned()
        });
        let Some(drag) = &mut self.sidebar_drag else {
            return false;
        };
        drag.pointer = point;
        if !drag.active && point.distance(drag.origin) >= DRAG_THRESHOLD {
            drag.active = true;
        }
        if !drag.active {
            return false;
        }
        drag.target = None;
        drag.marker = None;
        let Some(item) = box_item else {
            return true;
        };
        if pinned_len == 0 || drag.from >= pinned_len || !geometry.viewport.contains(point) {
            return true;
        }
        let raw = geometry.rect(item.rect);
        let bounds = Rectangle {
            x: raw.x + geometry.viewport.x - geometry.offset.x,
            y: raw.y + geometry.viewport.y - geometry.offset.y,
            ..raw
        };
        let width = rail_width(&item, bounds.width);
        let min_y = bounds.y + END_INSET;
        let max_y = (bounds.y + bounds.height - END_INSET).max(min_y);
        if point.x < bounds.x || point.x > bounds.x + width || point.y < min_y || point.y > max_y {
            return true;
        }
        let ty = geometry.typography;
        let stored = self.sidebar_offsets.get(&item.id).copied().unwrap_or(0.0);
        let clamped = offset(pinned_len, loc_len, bounds.height, stored, ty);
        let content_y = point.y - min_y + clamped;
        let header_h = ty.row(SECTION_HEADER_HEIGHT);
        let row_h = ty.row(ROW_HEIGHT);
        let top_end = header_h + pinned_len as f32 * row_h;
        let tail = if loc_len == 0 {
            row_h * 2.0
        } else {
            ty.row(SECTION_DIVIDER_HEIGHT)
        };
        if content_y > top_end + tail {
            return true;
        }
        let insert = ((content_y - header_h) / row_h)
            .clamp(0.0, pinned_len as f32)
            .round() as usize;
        drag.target = Some(insert.saturating_sub(usize::from(drag.from < insert)));
        drag.marker = Rectangle {
            x: bounds.x + 7.0,
            y: (min_y + header_h + insert as f32 * row_h - clamped - 2.0)
                .clamp(min_y, (max_y - 4.0).max(min_y)),
            width: (width - 14.0).max(1.0),
            height: 4.0,
        }
        .intersection(&geometry.viewport);
        true
    }

    pub(crate) fn finish_sidebar_drag(&mut self) -> Task<Message> {
        let Some(drag) = self.sidebar_drag.take() else {
            return Task::none();
        };
        if !drag.active {
            return self.activate_sidebar_location(drag.id, drag.path);
        }
        match drag.target {
            Some(to)
                if to != drag.from
                    && self.application.pinned_folders().get(drag.from) == Some(&drag.path) =>
            {
                self.dispatch(Action::ReorderPinnedFolder {
                    from: drag.from,
                    to,
                })
            }
            _ => Task::none(),
        }
    }
}

pub(crate) fn view(desktop: &Desktop, palette: a::Palette) -> Element<'_, Message> {
    let Some(drag) = desktop.sidebar_drag.as_ref().filter(|drag| drag.active) else {
        return space().into();
    };
    let label = folder_display_name(&drag.path, desktop.application.home());
    let preview = container(
        palette
            .typography
            .small(label)
            .wrapping(text::Wrapping::None),
    )
    .padding([8, 12])
    .width(160)
    .clip(true)
    .style(move |_| container::Style {
        background: Some(palette.contrast_background.into()),
        text_color: Some(palette.tab_text),
        border: Border {
            color: palette.selected_border,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..container::Style::default()
    });
    let preview = pin(preview)
        .x((drag.pointer.x + 18.0).min((desktop.size.width - 172.0).max(12.0)))
        .y((drag.pointer.y + 18.0).min((desktop.size.height - 48.0).max(12.0)));
    let marker = drag
        .marker
        .map_or_else(|| space().into(), |rect| insertion_marker(rect, palette));
    stack![marker, preview].into()
}
