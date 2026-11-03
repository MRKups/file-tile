//! Window-space anchors and keyboard reveal inside the bounded popup.
use super::{Message, anchor_id};
use iced::{
    Rectangle, Task, Vector,
    advanced::widget::{
        Id, Operation,
        operation::{Focusable, Outcome, Scrollable},
    },
};
pub(super) const SCROLL_ID: &str = "sort-menu-scroll";
pub(super) fn anchor(view: crate::domain::ViewId, generation: u64) -> Task<Message> {
    iced::advanced::widget::operate(Anchor {
        id: anchor_id(view).into(),
        bounds: None,
        offset: Vector::ZERO,
        pending_offset: Vector::ZERO,
    })
    .map(move |bounds| Message::SortAnchor(generation, bounds))
}
struct Anchor {
    id: Id,
    bounds: Option<Rectangle>,
    offset: Vector,
    pending_offset: Vector,
}
impl Operation<Option<Rectangle>> for Anchor {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Option<Rectangle>>)) {
        let previous = self.offset;
        self.offset += std::mem::replace(&mut self.pending_offset, Vector::ZERO);
        operate(self);
        self.offset = previous;
    }
    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        if id == Some(&self.id) {
            self.bounds = Some(Rectangle {
                x: bounds.x - self.offset.x,
                y: bounds.y - self.offset.y,
                ..bounds
            });
        }
    }
    fn scrollable(
        &mut self,
        _: Option<&Id>,
        _: Rectangle,
        _: Rectangle,
        translation: Vector,
        _: &mut dyn Scrollable,
    ) {
        self.pending_offset = translation;
    }
    fn finish(&self) -> Outcome<Option<Rectangle>> {
        Outcome::Some(self.bounds)
    }
}
pub(super) fn reveal() -> Task<Message> {
    iced::advanced::widget::operate(Reveal::default())
}
#[derive(Default)]
struct Reveal {
    focused: Option<Rectangle>,
    viewport: Option<(Rectangle, Rectangle, Vector)>,
}
impl Operation<Message> for Reveal {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Message>)) {
        operate(self);
    }
    fn focusable(&mut self, _: Option<&Id>, bounds: Rectangle, state: &mut dyn Focusable) {
        if state.is_focused() {
            self.focused = Some(bounds.expand(2.0));
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
        if id == Some(&Id::from(SCROLL_ID)) {
            self.viewport = Some((bounds, content, translation));
        }
    }
    fn finish(&self) -> Outcome<Message> {
        let (Some(focus), Some((bounds, content, offset))) = (self.focused, self.viewport) else {
            return Outcome::None;
        };
        let top = focus.y - offset.y;
        let bottom = top + focus.height;
        let change = if top < bounds.y {
            top - bounds.y
        } else if bottom > bounds.y + bounds.height {
            bottom - bounds.y - bounds.height
        } else {
            return Outcome::None;
        };
        Outcome::Chain(Box::new(
            iced::advanced::widget::operation::scrollable::scroll_to(
                SCROLL_ID.into(),
                iced::advanced::widget::operation::scrollable::AbsoluteOffset {
                    x: None,
                    y: Some(
                        (offset.y + change).clamp(0.0, (content.height - bounds.height).max(0.0)),
                    ),
                },
            ),
        ))
    }
}
