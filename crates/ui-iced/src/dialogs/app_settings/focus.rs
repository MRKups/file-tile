//! Reveal focused fields inside the form without scrolling the fixed actions.

use iced::advanced::widget::{
    Id, Operation,
    operation::{Focusable, Outcome, Scrollable},
};
use iced::{Rectangle, Task, Vector};

use super::FOLDER_FIELD;
use crate::Message;

pub(super) const SCROLL_ID: &str = "app-settings-form";

pub(super) fn field(id: &'static str) -> Task<Message> {
    iced::widget::operation::focus(id).chain(iced::advanced::widget::operate(reveal()))
}

pub(super) fn reveal<T: 'static>() -> impl Operation<T> {
    #[derive(Default)]
    struct Reveal {
        focused: Option<Rectangle>,
        focused_id: Option<Id>,
        fields: Vec<(Id, Rectangle)>,
        viewport: Option<(Rectangle, Rectangle, Vector)>,
    }

    impl<T: 'static> Operation<T> for Reveal {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<T>)) {
            operate(self);
        }

        fn focusable(&mut self, id: Option<&Id>, bounds: Rectangle, state: &mut dyn Focusable) {
            let fixed = [
                "app-settings-cancel",
                "app-settings-apply",
                "custom-themes-new",
                "custom-themes-cancel",
                "custom-themes-done",
                "font-explorer-search",
                "font-explorer-size",
                "font-explorer-weight",
                "font-explorer-cancel",
                "font-explorer-done",
            ];
            if state.is_focused() && !fixed.iter().any(|name| id == Some(&Id::new(name))) {
                self.focused = Some(bounds.expand(3.0));
                self.focused_id = id.cloned();
            }
        }

        fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
            if let Some(id) = id
                && id == &Id::new(FOLDER_FIELD)
            {
                self.fields.push((id.clone(), bounds.expand(3.0)));
            }
        }

        fn scrollable(
            &mut self,
            id: Option<&Id>,
            bounds: Rectangle,
            content: Rectangle,
            translation: Vector,
            _: &mut dyn Scrollable,
        ) {
            if id == Some(&Id::new(SCROLL_ID)) {
                self.viewport = Some((bounds, content, translation));
            }
        }

        fn finish(&self) -> Outcome<T> {
            let (Some(focused), Some((bounds, content, translation))) =
                (self.focused, self.viewport)
            else {
                return Outcome::None;
            };
            // Keep inline validation with its focused field, not below the fold.
            let focused = self
                .fields
                .iter()
                .find(|(id, _)| Some(id) == self.focused_id.as_ref())
                .map_or(focused, |(_, bounds)| *bounds);
            if focused.x + focused.width <= bounds.x || focused.x >= bounds.x + bounds.width {
                return Outcome::None;
            }
            let top = focused.y - translation.y;
            let bottom = top + focused.height;
            let adjustment = if top < bounds.y {
                top - bounds.y
            } else if bottom > bounds.y + bounds.height {
                bottom - bounds.y - bounds.height
            } else {
                return Outcome::None;
            };
            let offset =
                (translation.y + adjustment).clamp(0.0, (content.height - bounds.height).max(0.0));
            Outcome::Chain(Box::new(
                iced::advanced::widget::operation::scrollable::scroll_to(
                    SCROLL_ID.into(),
                    iced::advanced::widget::operation::scrollable::AbsoluteOffset {
                        x: None,
                        y: Some(offset),
                    },
                ),
            ))
        }
    }

    Reveal::default()
}
