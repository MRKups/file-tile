//! Retain Iced scrolling/content behavior; own bounded thumb geometry and idle fade.
mod activity;
mod geometry;
mod input;

use crate::Message;
use activity::Activity;
use geometry::Sample;
use iced::advanced::Renderer as _;
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, overlay, renderer,
    widget::{Id, Operation, Tree, operation::Scrollable, tree},
};
use iced::{
    Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector, mouse, widget::scrollable,
    window,
};
use std::rc::Rc;
use std::time::Instant;

#[derive(Clone, Copy)]
pub(crate) struct ScrollViewport {
    offset: scrollable::AbsoluteOffset,
    bounds: Rectangle,
}

impl ScrollViewport {
    pub fn absolute_offset(&self) -> scrollable::AbsoluteOffset {
        self.offset
    }

    pub fn bounds(&self) -> Rectangle {
        self.bounds
    }
}

type ScrollHandler<'a> = Rc<dyn Fn(ScrollViewport) -> Message + 'a>;

pub(crate) struct Area<'a> {
    inner: scrollable::Scrollable<'a, Message>,
    direction: scrollable::Direction,
    style: scrollable::StyleFn<'a, Theme>,
    on_scroll: Option<ScrollHandler<'a>>,
}

#[derive(Default)]
struct State {
    sample: Sample,
    activity: Activity,
    drag: Option<input::Drag>,
}

pub(crate) fn scroll_area<'a>(content: impl Into<Element<'a, Message>>) -> Area<'a> {
    Area {
        inner: scrollable::Scrollable::new(content).style(|theme, status| {
            let mut style = scrollable::default(theme, status);
            for rail in [&mut style.vertical_rail, &mut style.horizontal_rail] {
                rail.background = None;
                rail.border = Default::default();
                rail.scroller.background = iced::Color::TRANSPARENT.into();
                rail.scroller.border = Default::default();
            }
            style.gap = None;
            style
        }),
        direction: scrollable::Direction::Vertical(crate::window::appearance::scrollbar()),
        style: Box::new(crate::window::appearance::scrollable),
        on_scroll: None,
    }
}

impl<'a> Area<'a> {
    pub fn id(mut self, id: impl Into<Id>) -> Self {
        self.inner = self.inner.id(id);
        self
    }
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.inner = self.inner.width(width);
        self
    }
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.inner = self.inner.height(height);
        self
    }
    pub fn direction(mut self, direction: scrollable::Direction) -> Self {
        self.direction = direction;
        self.inner = self.inner.direction(direction);
        self
    }
    pub fn on_scroll(mut self, handler: impl Fn(ScrollViewport) -> Message + 'a) -> Self {
        let shared: ScrollHandler<'a> = Rc::new(handler);
        let for_inner = Rc::clone(&shared);
        self.inner = self.inner.on_scroll(move |viewport| {
            for_inner(ScrollViewport {
                offset: viewport.absolute_offset(),
                bounds: viewport.bounds(),
            })
        });
        self.on_scroll = Some(shared);
        self
    }
    pub fn style(
        mut self,
        style: impl Fn(&Theme, scrollable::Status) -> scrollable::Style + 'a,
    ) -> Self {
        self.style = Box::new(style);
        self
    }

    fn sample(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer) {
        let mut probe = Probe::default();
        self.inner
            .operate(&mut tree.children[0], layout, renderer, &mut probe);
        let state = tree.state.downcast_mut::<State>();
        if probe.sample.offset != state.sample.offset {
            state.activity.wake(Instant::now());
        }
        state.sample = probe.sample;
    }
}

#[derive(Default)]
struct Probe {
    sample: Sample,
}
impl Operation for Probe {
    fn traverse(&mut self, _: &mut dyn FnMut(&mut dyn Operation)) {}
    // Only the wrapped scroller, never its nested content.
    fn scrollable(
        &mut self,
        _: Option<&Id>,
        bounds: Rectangle,
        content: Rectangle,
        offset: Vector,
        _: &mut dyn Scrollable,
    ) {
        self.sample = Sample {
            bounds,
            content,
            offset,
        };
    }
}

impl Widget<Message, Theme, Renderer> for Area<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(
            &self.inner as &dyn Widget<Message, Theme, Renderer>,
        )]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.inner as &dyn Widget<Message, Theme, Renderer>]);
    }
    fn size(&self) -> Size<Length> {
        self.inner.size()
    }
    fn size_hint(&self) -> Size<Length> {
        self.inner.size_hint()
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let node = self.inner.layout(&mut tree.children[0], renderer, limits);
        self.sample(tree, Layout::new(&node), renderer);
        node
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.inner
            .operate(&mut tree.children[0], layout, renderer, operation);
        self.sample(tree, layout, renderer);
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.sample(tree, layout, renderer);
        let now = if let Event::Window(window::Event::RedrawRequested(now)) = event {
            *now
        } else {
            Instant::now()
        };
        let before_offset = tree.state.downcast_ref::<State>().sample.offset;
        let input = tree.state.downcast_mut::<State>().rail_input(
            event,
            cursor,
            viewport,
            self.direction,
            now,
        );
        let mut rail_scrolled = false;
        if let Some(input) = input {
            if let Some(mut offset) = input.offset {
                self.inner
                    .operate(&mut tree.children[0], layout, renderer, &mut offset);
                rail_scrolled = true;
            }
            shell.capture_event();
            shell.request_redraw();
        } else {
            self.inner.update(
                &mut tree.children[0],
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }
        self.sample(tree, layout, renderer);
        let state = tree.state.downcast_mut::<State>();
        if rail_scrolled
            && state.sample.offset != before_offset
            && let Some(on_scroll) = &self.on_scroll
        {
            shell.publish(on_scroll(ScrollViewport {
                offset: scrollable::AbsoluteOffset {
                    x: state.sample.offset.x,
                    y: state.sample.offset.y,
                },
                bounds: state.sample.bounds,
            }));
        }
        let hovered = cursor.position().is_some_and(|point| {
            viewport.contains(point)
                && state
                    .sample
                    .rails(self.direction)
                    .iter()
                    .any(|rail| rail.target.contains(point))
        });
        if state.activity.hovered != hovered {
            state.activity.wake(now);
            shell.request_redraw();
        }
        state.activity.hovered = hovered;
        if !state.sample.rails(self.direction).is_empty() {
            state
                .activity
                .update(now, hovered || state.drag.is_some(), shell);
        }
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.inner.draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
        let state = tree.state.downcast_ref::<State>();
        // Layout may move a nested scroller since its last event. Translate the
        // cached measurement to today's layout without storing rendering state.
        let sample = Sample {
            bounds: layout.bounds(),
            ..state.sample
        };
        let Some(clip) = sample.bounds.intersection(viewport) else {
            return;
        };
        renderer.with_layer(clip, |renderer| {
            for rail in sample.rails(self.direction) {
                let hovered = cursor
                    .position()
                    .is_some_and(|point| clip.contains(point) && rail.target.contains(point));
                let dragged = state
                    .drag
                    .is_some_and(|drag| drag.horizontal == rail.horizontal);
                let status = if dragged {
                    scrollable::Status::Dragged {
                        is_horizontal_scrollbar_dragged: rail.horizontal,
                        is_vertical_scrollbar_dragged: !rail.horizontal,
                        is_horizontal_scrollbar_disabled: false,
                        is_vertical_scrollbar_disabled: false,
                    }
                } else {
                    scrollable::Status::Hovered {
                        is_horizontal_scrollbar_hovered: hovered && rail.horizontal,
                        is_vertical_scrollbar_hovered: hovered && !rail.horizontal,
                        is_horizontal_scrollbar_disabled: false,
                        is_vertical_scrollbar_disabled: false,
                    }
                };
                let style = (self.style)(theme, status);
                let scroller = if rail.horizontal {
                    style.horizontal_rail.scroller
                } else {
                    style.vertical_rail.scroller
                };
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: rail.thumb,
                        border: scroller.border,
                        ..Default::default()
                    },
                    scroller.background.scale_alpha(if hovered || dragged {
                        1.0
                    } else {
                        state.activity.opacity
                    }),
                );
            }
        });
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        if state.drag.is_some() {
            mouse::Interaction::Grabbing
        } else if cursor.position().is_some_and(|point| {
            viewport.contains(point)
                && state
                    .sample
                    .rails(self.direction)
                    .iter()
                    .any(|rail| rail.target.contains(point))
        }) {
            mouse::Interaction::Pointer
        } else {
            self.inner
                .mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
        }
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.inner.overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a> From<Area<'a>> for Element<'a, Message> {
    fn from(area: Area<'a>) -> Self {
        Element::new(area)
    }
}
