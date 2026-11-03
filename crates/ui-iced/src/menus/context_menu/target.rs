//! Context-only hit testing. Never deliver a replacement click to background widgets.

use crate::Message;
use iced::advanced::widget::{Id, Operation, operation::Scrollable};
use iced::{Point, Rectangle, Vector};
use std::any::Any;

/// Ephemeral trigger data, not the child widget's mutable interaction state.
pub(super) struct Target(pub Message);

#[derive(Clone, Copy)]
pub(super) struct Scrollbars {
    pub vertical: f32,
    pub horizontal: f32,
}

impl Default for Scrollbars {
    fn default() -> Self {
        // Match the shared invisible pointer target, not the narrower thumb.
        Self {
            vertical: crate::window::appearance::SCROLLBAR_WIDTH,
            horizontal: crate::window::appearance::SCROLLBAR_WIDTH,
        }
    }
}

pub(super) struct Hit {
    point: Point,
    translation: Vector,
    next_translation: Vector,
    skip: bool,
    scrollbars: Scrollbars,
    pub message: Option<Message>,
    pub over_scrollbar: bool,
}

impl Hit {
    pub fn new(point: Point) -> Self {
        Self {
            point,
            translation: Vector::ZERO,
            next_translation: Vector::ZERO,
            skip: false,
            scrollbars: Scrollbars::default(),
            message: None,
            over_scrollbar: false,
        }
    }

    fn contains(&self, bounds: Rectangle) -> bool {
        bounds.contains(self.point + self.translation)
    }
}

impl Operation for Hit {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        let skip = std::mem::take(&mut self.skip);
        let delta = std::mem::replace(&mut self.next_translation, Vector::ZERO);
        if !skip {
            self.translation += delta;
            operate(self);
            self.translation -= delta;
        }
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        self.skip = !self.contains(bounds);
        // These non-modal panels cover the canvas but offer no item context menu.
        let occludes = [
            crate::dialogs::box_settings::CARD_ID,
            crate::boxes::launchers::EDITOR_ID,
            crate::boxes::file_columns::EARLIER_MENU_ID,
            crate::menus::file_prompt::PANEL_ID,
            crate::menus::file_prompt::BLOCK_ID,
        ]
        .into_iter()
        .any(|panel| id == Some(&Id::from(panel)));
        if occludes && !self.skip {
            self.message = None;
            self.skip = true;
        }
    }

    fn scrollable(
        &mut self,
        _: Option<&Id>,
        bounds: Rectangle,
        content: Rectangle,
        translation: Vector,
        _: &mut dyn Scrollable,
    ) {
        let bars = std::mem::take(&mut self.scrollbars);
        let vertical = if content.height > bounds.height {
            bars.vertical
        } else {
            0.0
        };
        let horizontal = if content.width > bounds.width {
            bars.horizontal
        } else {
            0.0
        };
        let cursor = self.point + self.translation;
        let over_vertical = cursor.x >= bounds.x + bounds.width - vertical
            && cursor.y < bounds.y + bounds.height - horizontal;
        let over_horizontal = cursor.y >= bounds.y + bounds.height - horizontal
            && cursor.x < bounds.x + bounds.width - vertical;
        self.skip = !self.contains(bounds) || over_vertical || over_horizontal;
        if self.contains(bounds) && (over_vertical || over_horizontal) {
            self.over_scrollbar = true;
            self.message = None;
        }
        self.next_translation = translation;
    }

    fn custom(&mut self, _: Option<&Id>, bounds: Rectangle, state: &mut dyn Any) {
        if let Some(bars) = state.downcast_ref::<Scrollbars>() {
            self.scrollbars = *bars;
        }
        if self.contains(bounds)
            && let Some(Target(message)) = state.downcast_ref::<Target>()
        {
            self.message = Some(at_point(message.clone(), self.point));
        }
    }
}

pub(super) fn at_point(message: Message, point: Point) -> Message {
    match message {
        Message::OpenFolderMenu(view, _) => Message::OpenFolderMenu(view, point),
        Message::OpenPreviewFolderMenu(view, _) => Message::OpenPreviewFolderMenu(view, point),
        Message::OpenEntryMenu(view, path, Some(_)) => {
            Message::OpenEntryMenu(view, path, Some(point))
        }
        Message::OpenTabMenu(id, view, Some(_)) => Message::OpenTabMenu(id, view, Some(point)),
        message => message,
    }
}

pub(super) fn dismissal_identity(message: &Message) -> Option<super::Identity> {
    match message {
        Message::DismissEntryMenu(generation) => Some(super::Identity::Entry(*generation)),
        Message::DismissTabMenu(generation) => Some(super::Identity::Tab(*generation)),
        Message::DismissFolderMenu(generation) => Some(super::Identity::Folder(*generation)),
        _ => None,
    }
}
