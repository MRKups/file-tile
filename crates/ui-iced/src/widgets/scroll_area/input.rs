//! Bounded thumb interaction updates Iced's existing scroll state, not a second offset.
use super::*;
use iced::touch;

#[derive(Clone, Copy)]
pub(super) struct Drag {
    pub horizontal: bool,
    grabbed: f32,
    finger: Option<touch::Finger>,
}

pub(super) struct SetOffset {
    horizontal: bool,
    offset: f32,
}
impl Operation for SetOffset {
    fn traverse(&mut self, _: &mut dyn FnMut(&mut dyn Operation)) {}
    fn scrollable(
        &mut self,
        _: Option<&Id>,
        _: Rectangle,
        _: Rectangle,
        _: Vector,
        state: &mut dyn Scrollable,
    ) {
        state.scroll_to(scrollable::AbsoluteOffset {
            x: self.horizontal.then_some(self.offset),
            y: (!self.horizontal).then_some(self.offset),
        });
    }
}

pub(super) struct Input {
    pub offset: Option<SetOffset>,
}

impl State {
    pub(super) fn rail_input(
        &mut self,
        event: &Event,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        direction: scrollable::Direction,
        now: Instant,
    ) -> Option<Input> {
        let state = self;
        if matches!(event, Event::Window(window::Event::Unfocused)) {
            state.drag = None;
            return None;
        }
        // Ancestor scrollers translate the cursor, not the event's raw window
        // position. A retained drag may leave their visible content viewport.
        let point = if state.drag.is_some() {
            cursor.land().position()
        } else {
            cursor.position()
        };
        let (point, pressed, released, finger) = match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                (point, true, false, None)
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                (point, false, true, None)
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => (point, false, false, None),
            Event::Touch(touch::Event::FingerPressed { id, .. }) => (point, true, false, Some(*id)),
            Event::Touch(touch::Event::FingerMoved { id, .. }) => (point, false, false, Some(*id)),
            Event::Touch(
                touch::Event::FingerLifted { id, .. } | touch::Event::FingerLost { id, .. },
            ) => (point, false, true, Some(*id)),
            _ => return None,
        };
        let rails = state.sample.rails(direction);
        if let Some(drag) = state.drag {
            if drag.finger != finger {
                return None;
            }
            if released {
                state.drag = None;
                state.activity.wake(now);
                return Some(Input { offset: None });
            }
        } else if pressed
            && let Some(point) = point.filter(|point| viewport.contains(*point))
            && let Some(rail) = rails.iter().find(|rail| rail.target.contains(point))
        {
            let coordinate = rail.coordinate(point);
            let grabbed = if (rail.thumb_start()..=rail.thumb_start() + rail.length())
                .contains(&coordinate)
            {
                ((coordinate - rail.thumb_start()) / rail.length()).clamp(0.0, 1.0)
            } else {
                0.5
            };
            state.drag = Some(Drag {
                horizontal: rail.horizontal,
                grabbed,
                finger,
            });
        }
        let drag = state.drag?;
        let Some(rail) = rails
            .into_iter()
            .find(|rail| rail.horizontal == drag.horizontal)
        else {
            state.drag = None;
            return None;
        };
        state.activity.wake(now);
        Some(Input {
            offset: point.map(|point| SetOffset {
                horizontal: drag.horizontal,
                offset: rail.offset(point, drag.grabbed),
            }),
        })
    }
}
