//! Match complete box widget trees by container identity when removal/Undo changes order.

use crate::Message;
use crate::domain::BoxId;
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, overlay, renderer,
    widget::{Operation, Tree, tree},
};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector, mouse};

struct Boxes<'a> {
    identities: Vec<BoxId>,
    content: Element<'a, Message>,
}

pub(super) fn view(identities: Vec<BoxId>, content: Element<'_, Message>) -> Element<'_, Message> {
    Element::new(Boxes {
        identities,
        content,
    })
}

impl Widget<Message, Theme, Renderer> for Boxes<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Vec<BoxId>>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(self.identities.clone())
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        let identities = tree.state.downcast_mut::<Vec<BoxId>>();
        let content_tree = &mut tree.children[0];
        if *identities != self.identities {
            let mut previous: std::collections::HashMap<_, _> = identities
                .iter()
                .copied()
                .zip(std::mem::take(&mut content_tree.children))
                .collect();
            let mut children = self.content.as_widget().children();
            for (index, id) in self.identities.iter().enumerate() {
                if let Some(previous) = previous.remove(id) {
                    children[index] = previous;
                }
            }
            content_tree.children = children;
            *identities = self.identities.clone();
        }
        // Always use Tree::diff, including its tag check.
        content_tree.diff(&self.content);
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
    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}
