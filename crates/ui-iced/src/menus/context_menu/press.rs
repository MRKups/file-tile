//! Primary activation and right-click menu triggers before button capture/blur probes.

use super::target;
use crate::Message;
use crate::domain::ViewId;
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, overlay, renderer,
    widget::{Operation, Tree, tree},
};
use iced::{Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector, mouse};

fn pressed(event: &Event, button: mouse::Button) -> bool {
    matches!(event, Event::Mouse(mouse::Event::ButtonPressed(actual)) if *actual == button)
}

enum Mode<'a> {
    Triggers {
        activation: Option<(ViewId, Box<Message>)>,
        context: Option<Box<dyn Fn(Point) -> Message + 'a>>,
        background: bool,
    },
    WindowCoordinates {
        menu_open: bool,
    },
    Scrollbars(target::Scrollbars),
}

/// Register a custom scrollable's hit exclusions without dispatching input to it.
pub(crate) fn scrollbars(
    vertical: f32,
    horizontal: f32,
    content: Element<'_, Message>,
) -> Element<'_, Message> {
    Element::new(Press {
        mode: Mode::Scrollbars(target::Scrollbars {
            vertical,
            horizontal,
        }),
        content,
    })
}

struct Press<'a> {
    mode: Mode<'a>,
    content: Element<'a, Message>,
}

pub(crate) fn view<'a>(
    activation: Option<(ViewId, Message)>,
    context: Option<Box<dyn Fn(Point) -> Message + 'a>>,
    content: Element<'a, Message>,
) -> Element<'a, Message> {
    Element::new(Press {
        mode: Mode::Triggers {
            activation: activation.map(|(view, message)| (view, Box::new(message))),
            context,
            background: false,
        },
        content,
    })
}

pub(crate) fn background<'a>(
    view: ViewId,
    context: Option<Box<dyn Fn(Point) -> Message + 'a>>,
    content: Element<'a, Message>,
) -> Element<'a, Message> {
    background_with_focus(view, Message::FolderFocused(view), context, content)
}

pub(crate) fn background_with_focus<'a>(
    view: ViewId,
    focus: Message,
    context: Option<Box<dyn Fn(Point) -> Message + 'a>>,
    content: Element<'a, Message>,
) -> Element<'a, Message> {
    Element::new(Press {
        mode: Mode::Triggers {
            activation: context.as_ref().map(|_| (view, Box::new(focus))),
            context,
            background: true,
        },
        content,
    })
}

/// Mount outside all scrollables: their cursors are translated into content space.
pub(crate) fn window_coordinates(
    menu_open: bool,
    content: Element<'_, Message>,
) -> Element<'_, Message> {
    Element::new(Press {
        mode: Mode::WindowCoordinates { menu_open },
        content,
    })
}

impl Widget<Message, Theme, Renderer> for Press<'_> {
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }
    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }
    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }
    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
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
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        if let Mode::Triggers {
            context: Some(context),
            ..
        } = &self.mode
        {
            let mut target = target::Target(context(Point::ORIGIN));
            operation.custom(None, layout.bounds(), &mut target);
        }
        if let Mode::Scrollbars(mut bars) = self.mode {
            operation.custom(None, layout.bounds(), &mut bars);
        }
        self.content
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
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
        if matches!(self.mode, Mode::Scrollbars(_)) {
            self.content.as_widget_mut().update(
                tree, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
            return;
        }
        let Mode::Triggers {
            activation,
            context,
            background,
        } = &self.mode
        else {
            let mut messages = Vec::new();
            let mut child = Shell::new(&mut messages);
            self.content.as_widget_mut().update(
                tree, event, layout, cursor, renderer, clipboard, &mut child, viewport,
            );
            let right_press = pressed(event, mouse::Button::Right);
            let target = if right_press
                && matches!(self.mode, Mode::WindowCoordinates { menu_open: true })
            {
                cursor.position().and_then(|point| {
                    let mut query = target::Hit::new(point);
                    self.content
                        .as_widget_mut()
                        .operate(tree, layout, renderer, &mut query);
                    query.message.map(Box::new)
                })
            } else {
                None
            };
            shell.merge(child, |message| {
                if right_press && let Some(identity) = target::dismissal_identity(&message) {
                    return Message::ReplaceContextMenu(identity, target.clone());
                }
                cursor
                    .position()
                    .map_or(message.clone(), |point| target::at_point(message, point))
            });
            return;
        };
        let right_press = pressed(event, mouse::Button::Right);
        let left_press = pressed(event, mouse::Button::Left);
        if *background && (right_press || left_press) {
            self.content.as_widget_mut().update(
                tree, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
            let mut hit = target::Hit::new(cursor.position().unwrap_or(Point::ORIGIN));
            self.content
                .as_widget_mut()
                .operate(tree, layout, renderer, &mut hit);
            if hit.over_scrollbar {
                return;
            }
            if left_press {
                if !shell.is_event_captured()
                    && cursor.is_over(layout.bounds())
                    && cursor
                        .position()
                        .is_some_and(|point| viewport.contains(point))
                    && let Some((_, action)) = activation
                {
                    shell.publish(action.as_ref().clone());
                    shell.capture_event();
                }
                return;
            }
        }
        if !shell.is_event_captured()
            && right_press
            && cursor.is_over(layout.bounds())
            && let Some(context) = context
            && let Some(point) = cursor.position().filter(|point| viewport.contains(*point))
        {
            shell.publish(context(point));
            shell.capture_event();
            return;
        }
        if *background && right_press {
            return;
        }
        if !shell.is_event_captured()
            && left_press
            && cursor.is_over(layout.bounds())
            && let Some((view, activation)) = activation
        {
            // Both messages are delivered before the subscription's focus probe.
            // FocusTab suppresses the old card's blur; the typed action still owns
            // permissions, cancellation, budget admission, Undo, and saving.
            shell.publish(Message::FocusTab(*view));
            shell.publish(activation.as_ref().clone());
        }
        self.content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
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
        self.content
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
    }
}
