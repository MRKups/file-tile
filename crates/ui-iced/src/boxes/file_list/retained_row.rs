use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, overlay, renderer,
    widget::{Operation, Tree, tree},
};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector, mouse};

use super::{Message, RowKey, ViewId};

#[derive(Clone, Copy, PartialEq)]
struct Identity {
    view: ViewId,
    key: RowKey,
    interactive: bool,
}

struct RetainedRow<'a> {
    identity: Identity,
    content: Element<'a, Message>,
    demand: Option<Demand>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Demand {
    pub revision: u64,
    pub paths: Vec<std::path::PathBuf>,
    pub settings: crate::domain::BoxSettings,
}
struct State {
    identity: Identity,
    sent: Option<Demand>,
}

pub(super) fn view(
    view: ViewId,
    key: RowKey,
    interactive: bool,
    content: Element<'_, Message>,
) -> Element<'_, Message> {
    with_demand(view, key, interactive, content, None)
}

pub(crate) fn with_demand(
    view: ViewId,
    key: RowKey,
    interactive: bool,
    content: Element<'_, Message>,
    demand: Option<Demand>,
) -> Element<'_, Message> {
    Element::new(RetainedRow {
        identity: Identity {
            view,
            key,
            interactive,
        },
        content,
        demand,
    })
}

impl Widget<Message, Theme, Renderer> for RetainedRow<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            identity: self.identity,
            sent: None,
        })
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        // Iced 0.14.2's keyed Column calls Widget::diff directly, bypassing
        // Tree::diff's tag check, and can retain different keys at a slot.
        // Keep its children uniform here and reconcile the actual content safely.
        let state = tree.state.downcast_mut::<State>();
        if state.identity != self.identity {
            state.identity = self.identity;
            state.sent = None;
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
        if matches!(
            event,
            Event::Window(iced::window::Event::RedrawRequested(_))
        ) {
            let state = tree.state.downcast_mut::<State>();
            if state.sent != self.demand {
                state.sent = self.demand.clone();
                if let Some(demand) = &self.demand {
                    shell.publish(Message::Action(crate::application::Action::MetadataNeeded(
                        self.identity.view,
                        demand.paths.clone(),
                    )));
                }
            }
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
