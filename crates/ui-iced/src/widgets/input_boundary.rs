//! Retain background widgets while excluding modal-obscured input and focus.

use crate::{Desktop, Message};
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, overlay, renderer,
    widget::{Id, Operation, Tree, operation::Scrollable, tree},
};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector, mouse};

impl Desktop {
    pub(crate) fn blocked_input(
        &mut self,
        event: &Event,
        status: iced::event::Status,
    ) -> Option<iced::Task<Message>> {
        if !matches!(event, Event::Mouse(_) | Event::Keyboard(_)) {
            return None;
        }
        // Keep routing aligned with visible overlays, with recovery first.
        if self.storage_details && self.application.storage_detail().is_some() {
            Some(self.recovery_input(event.clone()))
        } else if self.native_menu.error().is_some() {
            Some(match event {
                Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab),
                    repeat: false,
                    ..
                }) => iced::widget::operation::focus("native-menu-close"),
                _ => iced::Task::none(),
            })
        } else if self.workspace_management.is_some() {
            Some(self.workspace_management_input(event.clone()))
        } else if self.about.is_some() {
            Some(self.about_input(event.clone()))
        } else if self.app_settings.is_some() {
            Some(self.app_settings_input(event.clone(), status))
        } else if self.sort_menu.is_some() {
            if matches!(
                event,
                Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    ..
                })
            ) {
                Some(self.dismiss_sort(true))
            } else {
                Some(crate::menus::sort_menu::input(event.clone()))
            }
        } else if self.tab_menu.is_some() {
            Some(self.tab_menu_input(event.clone()))
        } else if self.sidebar_menu.is_some() {
            Some(self.sidebar_menu_input(event.clone()))
        } else if self.entry_menu.is_some() {
            Some(self.entry_menu_input(event.clone()))
        } else if let Some(menu) = &self.folder_menu {
            if matches!(
                event,
                Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    ..
                })
            ) {
                Some(self.context_menu_message(Message::DismissFolderMenu(menu.generation)))
            } else {
                Some(crate::menus::context_menu::input(event.clone()))
            }
        } else {
            None
        }
    }
}

struct State;
struct Boundary<'a> {
    blocked: bool,
    content: Element<'a, Message>,
}

/// Mount this wrapper at the same slot in both modal and ordinary states.
pub(crate) fn view<'a>(
    blocked: bool,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    Element::new(Boundary {
        blocked,
        content: content.into(),
    })
}

/// Focus, editor, and opaque custom-state operations cannot reach the background.
/// Scroll measurements/adjustments still reconcile background listing results.
struct NonInputOperation<'a> {
    operation: &'a mut dyn Operation,
}

impl Operation for NonInputOperation<'_> {
    fn custom(&mut self, id: Option<&Id>, bounds: Rectangle, state: &mut dyn std::any::Any) {
        // Only ephemeral context triggers are exposed, never editor/focus state.
        if crate::menus::context_menu::is_target(state) {
            self.operation.custom(id, bounds, state);
        }
    }
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        self.operation.traverse(&mut |operation| {
            operate(&mut NonInputOperation { operation });
        });
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        self.operation.container(id, bounds);
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        content_bounds: Rectangle,
        translation: Vector,
        state: &mut dyn Scrollable,
    ) {
        self.operation
            .scrollable(id, bounds, content_bounds, translation, state);
    }

    fn text(&mut self, id: Option<&Id>, bounds: Rectangle, text: &str) {
        self.operation.text(id, bounds, text);
    }
}

impl Widget<Message, Theme, Renderer> for Boundary<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State)
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[self.content.as_widget()]);
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
        if self.blocked {
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                layout,
                renderer,
                &mut NonInputOperation { operation },
            );
        } else {
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                layout,
                renderer,
                operation,
            );
        }
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
        if self.blocked
            && matches!(
                event,
                Event::Keyboard(_) | Event::Mouse(_) | Event::Touch(_) | Event::InputMethod(_)
            )
        {
            return;
        }
        if self.blocked {
            let mut messages = Vec::new();
            let mut background = Shell::new(&mut messages);
            self.content.as_widget_mut().update(
                &mut tree.children[0],
                event,
                layout,
                mouse::Cursor::Unavailable,
                renderer,
                clipboard,
                &mut background,
                viewport,
            );
            // A retained focused editor can request IME during redraw even
            // though it did not receive an input event. Only the modal owns IME.
            *background.input_method_mut() = iced::advanced::InputMethod::Disabled;
            shell.merge(background, std::convert::identity);
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
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            if self.blocked {
                mouse::Cursor::Unavailable
            } else {
                cursor
            },
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
        if self.blocked {
            mouse::Interaction::None
        } else {
            self.content.as_widget().mouse_interaction(
                &tree.children[0],
                layout,
                cursor,
                viewport,
                renderer,
            )
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
        if self.blocked {
            None
        } else {
            self.content.as_widget_mut().overlay(
                &mut tree.children[0],
                layout,
                renderer,
                viewport,
                translation,
            )
        }
    }
}
