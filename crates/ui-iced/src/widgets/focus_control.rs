//! Keyboard ownership and contained keyboard-only fills for ordinary controls.

use crate::{Message, window::appearance as a};
use iced::advanced::Renderer as _;
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, overlay, renderer,
    widget::{Id, Operation, Tree, operation::Focusable, tree},
};
use iced::{
    Border, Color, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector, keyboard,
    mouse,
};

#[derive(Default)]
struct Focus {
    focused: bool,
    keyboard: bool,
}
struct State {
    id: Id,
    focus: Focus,
}
impl Focusable for Focus {
    fn is_focused(&self) -> bool {
        self.focused
    }
    fn focus(&mut self) {
        self.focused = true;
        self.keyboard = true;
    }
    fn unfocus(&mut self) {
        self.focused = false;
        self.keyboard = false;
    }
}

struct Control<'a> {
    id: Id,
    action: Option<Message>,
    content: Element<'a, Message>,
    keys: Vec<(keyboard::key::Named, keyboard::Modifiers, Message)>,
    exclusive_keys: bool,
    focus_radius: iced::border::Radius,
    focus_fill: Color,
}

pub(crate) fn themed<'a>(
    id: impl Into<Id>,
    action: Message,
    content: impl Into<Element<'a, Message>>,
    palette: a::Palette,
) -> Element<'a, Message> {
    themed_optional(id, Some(action), content, palette)
}

/// Disabled controls retain their child tree but cannot receive focus or activate.
pub(crate) fn themed_optional<'a>(
    id: impl Into<Id>,
    action: Option<Message>,
    content: impl Into<Element<'a, Message>>,
    palette: a::Palette,
) -> Element<'a, Message> {
    Element::new(Control {
        id: id.into(),
        action,
        content: content.into(),
        keys: Vec::new(),
        exclusive_keys: false,
        focus_radius: 6.0.into(),
        focus_fill: palette.tool_hover,
    })
}

/// Toolbar focus stays inside the rounded face, without a selection outline.
pub(crate) fn themed_tool<'a>(
    id: impl Into<Id>,
    action: Message,
    content: impl Into<Element<'a, Message>>,
    palette: a::Palette,
) -> Element<'a, Message> {
    themed_highlight_keys_radius(id, action, content, Vec::new(), 9.0.into(), palette)
}

pub(crate) fn themed_keys<'a>(
    id: impl Into<Id>,
    action: Message,
    content: impl Into<Element<'a, Message>>,
    keys: Vec<(keyboard::key::Named, keyboard::Modifiers, Message)>,
    palette: a::Palette,
) -> Element<'a, Message> {
    themed_highlight_keys_radius(id, action, content, keys, 6.0.into(), palette)
}

/// Let only this focus owner handle keys, not a child's hover-based shortcuts.
pub(crate) fn exclusive_keys<'a>(
    id: impl Into<Id>,
    action: Message,
    content: impl Into<Element<'a, Message>>,
    keys: Vec<(keyboard::key::Named, keyboard::Modifiers, Message)>,
    palette: a::Palette,
) -> Element<'a, Message> {
    Element::new(Control {
        id: id.into(),
        action: Some(action),
        content: content.into(),
        keys,
        exclusive_keys: true,
        focus_radius: 6.0.into(),
        focus_fill: palette.tool_hover,
    })
}

pub(crate) fn themed_highlight_keys<'a>(
    id: impl Into<Id>,
    action: Message,
    content: impl Into<Element<'a, Message>>,
    keys: Vec<(keyboard::key::Named, keyboard::Modifiers, Message)>,
    palette: a::Palette,
) -> Element<'a, Message> {
    themed_highlight_keys_radius(id, action, content, keys, 0.0.into(), palette)
}

pub(crate) fn themed_highlight_keys_radius<'a>(
    id: impl Into<Id>,
    action: Message,
    content: impl Into<Element<'a, Message>>,
    keys: Vec<(keyboard::key::Named, keyboard::Modifiers, Message)>,
    focus_radius: iced::border::Radius,
    palette: a::Palette,
) -> Element<'a, Message> {
    Element::new(Control {
        id: id.into(),
        action: Some(action),
        content: content.into(),
        keys,
        exclusive_keys: false,
        focus_radius,
        focus_fill: palette.tool_hover,
    })
}

impl Widget<Message, Theme, Renderer> for Control<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State {
            id: self.id.clone(),
            focus: Focus::default(),
        })
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        if tree.state.downcast_ref::<State>().id != self.id {
            tree.state = self.state();
            tree.children = self.children();
        } else {
            tree.diff_children(&[self.content.as_widget()]);
        }
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let focus = &mut tree.state.downcast_mut::<State>().focus;
        if self.action.is_some() {
            operation.focusable(Some(&self.id), layout.bounds(), focus);
        } else {
            focus.unfocus();
        }
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
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
        let focus = &mut tree.state.downcast_mut::<State>().focus;
        if self.action.is_none() {
            focus.unfocus();
            if matches!(
                event,
                Event::Mouse(_) | Event::Keyboard(_) | Event::Touch(_)
            ) {
                return;
            }
        }
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
            let focused = cursor.is_over(layout.bounds());
            if focus.focused != focused || focus.keyboard {
                shell.request_redraw();
            }
            focus.focused = focused;
            focus.keyboard = false;
        }
        if focus.is_focused()
            && matches!(event, Event::Keyboard(keyboard::Event::KeyPressed { .. }))
            && !focus.keyboard
        {
            focus.keyboard = true;
            shell.request_redraw();
        }
        if focus.is_focused()
            && let Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(key),
                modifiers,
                repeat: false,
                ..
            }) = event
            && let Some((_, _, action)) = self
                .keys
                .iter()
                .find(|(binding, flags, _)| binding == key && flags == modifiers)
        {
            shell.publish(action.clone());
            shell.capture_event();
            return;
        }
        if focus.is_focused()
            && let Some(action) = &self.action
            && matches!(event, Event::Keyboard(keyboard::Event::KeyPressed { key: keyboard::Key::Named(keyboard::key::Named::Enter | keyboard::key::Named::Space), modifiers, repeat: false, .. }) if modifiers.is_empty())
        {
            shell.publish(action.clone());
            shell.capture_event();
            return;
        }
        if self.exclusive_keys && matches!(event, Event::Keyboard(_)) {
            // Leave unmatched keys available to the modal/parent, but never to
            // a hovered child. Pointer events still follow the ordinary path.
            return;
        }
        self.content.as_widget_mut().update(
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
        let focus = &tree.state.downcast_ref::<State>().focus;
        // Pointer clicks retain keyboard ownership, not a persistent highlight.
        // Keyboard traversal gets an inset fill, never an outline.
        if self.action.is_some() && focus.is_focused() && focus.keyboard {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: layout.bounds(),
                    border: Border {
                        radius: self.focus_radius,
                        ..Border::default()
                    },
                    ..renderer::Quad::default()
                },
                self.focus_fill,
            );
        }
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}
