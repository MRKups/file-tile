//! Empty-workspace canvas guidance and interactive Layout-mode onboarding control.

use std::time::Instant;

use iced::advanced::Renderer as _;
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, renderer,
    widget::{Id, Operation, Tree, operation::Focusable, tree},
};
use iced::widget::container;
use iced::{
    Border, Color, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector,
    keyboard, mouse, touch, window,
};

use crate::application::Action;
use crate::application::contracts::storage::PersistenceState;
use crate::domain::Mode;
use crate::{Message, window::appearance as a};

const ONBOARDING_ID: &str = "empty-workspace-draw";
const PADDING_X: f32 = 24.0;
const PADDING_Y: f32 = 18.0;
const SPACING: f32 = 9.0;
const RADIUS: f32 = 12.0;
const HOVER_IN_SECS: f32 = 0.10;
const HOVER_OUT_SECS: f32 = 0.13;
const PRESS_IN_SECS: f32 = 0.055;
const PRESS_OUT_SECS: f32 = 0.09;

pub fn view(
    size: Size,
    persistence: &PersistenceState,
    interactive: bool,
    palette: a::Palette,
) -> Element<'static, Message> {
    let (title, help, action) = match persistence {
        PersistenceState::Loading(_) => {
            ("Loading workspace…", "Checking saved configuration.", None)
        }
        PersistenceState::Recovery { .. } => (
            "Workspace needs attention.",
            "Open Details for recovery options.",
            None,
        ),
        PersistenceState::Ready { .. } | PersistenceState::Off => (
            "Draw your workspace.",
            "Choose Layout to draw folder boxes and shortcut tiles.",
            interactive.then_some(Message::Action(Action::SelectMode(Mode::Layout))),
        ),
    };
    let ty = palette.typography;
    let prompt = Element::new(Prompt {
        id: Id::new(ONBOARDING_ID),
        title: ty.heading(title).into(),
        help: ty.text(help).into(),
        action,
        palette,
    });
    container(prompt)
        .padding(12)
        .center_x(size.width)
        .center_y(size.height)
        .into()
}

#[derive(Default)]
struct Focus {
    focused: bool,
    keyboard: bool,
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

#[derive(Default)]
struct State {
    focus: Focus,
    armed: bool,
    pointer_hovered: bool,
    highlighted: bool,
    pressing: bool,
    pointer_hover: f32,
    highlight: f32,
    press: f32,
    last_tick: Option<Instant>,
}

impl State {
    fn reset(&mut self) {
        self.focus.unfocus();
        self.armed = false;
        self.pointer_hovered = false;
        self.highlighted = false;
        self.pressing = false;
        self.pointer_hover = 0.0;
        self.highlight = 0.0;
        self.press = 0.0;
        self.last_tick = None;
    }

    fn advance(&mut self, now: Instant, over: bool, shell: &mut Shell<'_, Message>) {
        let target_pointer = over;
        let target_highlight = over || (self.focus.is_focused() && self.focus.keyboard);
        let target_press = self.armed && over;
        if target_pointer != self.pointer_hovered
            || target_highlight != self.highlighted
            || target_press != self.pressing
        {
            if target_pointer && self.pointer_hover == 0.0 {
                self.pointer_hover = 0.25;
            }
            if target_highlight && self.highlight == 0.0 {
                self.highlight = 0.25;
            }
            self.pointer_hovered = target_pointer;
            self.highlighted = target_highlight;
            self.pressing = target_press;
            if self.last_tick.is_none() {
                self.last_tick = Some(now);
            }
            shell.request_redraw();
        }
        let dt = self
            .last_tick
            .map_or(0.0, |prev| {
                now.saturating_duration_since(prev).as_secs_f32()
            })
            .min(0.05);
        if dt > 0.0 {
            self.pointer_hover = step(
                self.pointer_hover,
                target_pointer,
                dt,
                HOVER_IN_SECS,
                HOVER_OUT_SECS,
            );
            self.highlight = step(
                self.highlight,
                target_highlight,
                dt,
                HOVER_IN_SECS,
                HOVER_OUT_SECS,
            );
            self.press = step(self.press, target_press, dt, PRESS_IN_SECS, PRESS_OUT_SECS);
        }
        if settled(self.pointer_hover, target_pointer)
            && settled(self.highlight, target_highlight)
            && settled(self.press, target_press)
        {
            self.pointer_hover = goal(target_pointer);
            self.highlight = goal(target_highlight);
            self.press = goal(target_press);
            self.last_tick = None;
        } else {
            self.last_tick = Some(now);
            shell.request_redraw();
        }
    }
}

struct Prompt {
    id: Id,
    title: Element<'static, Message>,
    help: Element<'static, Message>,
    action: Option<Message>,
    palette: a::Palette,
}

impl Prompt {
    fn handle_event(
        &self,
        state: &mut State,
        event: &Event,
        over: bool,
        now: Instant,
        shell: &mut Shell<'_, Message>,
    ) -> bool {
        match event {
            Event::Window(
                window::Event::Unfocused | window::Event::Resized(_) | window::Event::Rescaled(_),
            )
            | Event::Mouse(mouse::Event::CursorLeft)
            | Event::Touch(touch::Event::FingerLost { .. }) => {
                state.armed = false;
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                if over {
                    state.focus.focused = true;
                    state.focus.keyboard = false;
                    state.armed = true;
                    state.pointer_hover = 1.0;
                    state.highlight = 1.0;
                    state.press = state.press.max(0.45);
                    state.last_tick = Some(now);
                    shell.capture_event();
                    shell.request_redraw();
                } else {
                    if state.focus.focused || state.focus.keyboard {
                        shell.request_redraw();
                    }
                    state.focus.unfocus();
                    state.armed = false;
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. }) => {
                if state.armed {
                    state.armed = false;
                    state.last_tick = Some(now);
                    shell.request_redraw();
                    if over {
                        if let Some(action) = &self.action {
                            shell.publish(action.clone());
                        }
                        shell.capture_event();
                    }
                }
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                repeat: false,
                ..
            }) if state.focus.is_focused() => {
                if !state.focus.keyboard {
                    state.focus.keyboard = true;
                    shell.request_redraw();
                }
                if matches!(
                    key,
                    keyboard::Key::Named(keyboard::key::Named::Enter | keyboard::key::Named::Space)
                ) && modifiers.is_empty()
                    && let Some(action) = &self.action
                {
                    shell.publish(action.clone());
                    shell.capture_event();
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    fn draw_chrome(
        &self,
        renderer: &mut Renderer,
        bounds: Rectangle,
        pointer_h: f32,
        h: f32,
        p: f32,
    ) {
        let inset_x = 2.5 * p;
        let quad_bounds = Rectangle {
            x: bounds.x + inset_x,
            y: bounds.y + 2.5 * p,
            width: (bounds.width - inset_x * 2.0).max(1.0),
            height: (bounds.height - 3.0 * p).max(1.0),
        };
        let fill = mix_color(self.palette.tool_hover, self.palette.tool_active, p).scale_alpha(h);
        let border_tint = mix_color(
            self.palette.grid_border,
            Color {
                a: 0.48,
                ..self.palette.selected_border
            },
            p,
        )
        .scale_alpha(pointer_h);
        renderer.fill_quad(
            renderer::Quad {
                bounds: quad_bounds,
                border: Border {
                    color: border_tint,
                    width: if pointer_h > 0.0 { 1.0 } else { 0.0 },
                    radius: (RADIUS - 0.5 * p).into(),
                },
                ..renderer::Quad::default()
            },
            fill,
        );
    }
}

impl Widget<Message, Theme, Renderer> for Prompt {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.title), Tree::new(&self.help)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[self.title.as_widget(), self.help.as_widget()]);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let limits = limits.width(Length::Shrink).height(Length::Shrink);
        let pad = Size::new(PADDING_X * 2.0, PADDING_Y * 2.0);
        let content_limits = limits.shrink(pad);
        let title = self
            .title
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, &content_limits)
            .move_to(Point::new(PADDING_X, PADDING_Y));
        let title_size = title.size();
        let help_limits = content_limits.shrink(Size::new(0.0, title_size.height + SPACING));
        let help = self
            .help
            .as_widget_mut()
            .layout(&mut tree.children[1], renderer, &help_limits)
            .move_to(Point::new(
                PADDING_X,
                PADDING_Y + title_size.height + SPACING,
            ));
        let help_size = help.size();
        let total = Size::new(
            title_size.width.max(help_size.width) + pad.width,
            title_size.height + SPACING + help_size.height + pad.height,
        );
        let size = limits.resolve(Length::Shrink, Length::Shrink, total);
        layout::Node::with_children(size, vec![title, help])
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let state = tree.state.downcast_mut::<State>();
        if self.action.is_some() {
            operation.focusable(Some(&self.id), layout.bounds(), &mut state.focus);
        } else {
            state.focus.unfocus();
        }
        let title = &mut self.title;
        let help = &mut self.help;
        let child_trees = &mut tree.children;
        operation.traverse(&mut |operation| {
            let mut layouts = layout.children();
            if let (Some(child_tree), Some(child_layout)) = (child_trees.get_mut(0), layouts.next())
            {
                title
                    .as_widget_mut()
                    .operate(child_tree, child_layout, renderer, operation);
            }
            if let (Some(child_tree), Some(child_layout)) = (child_trees.get_mut(1), layouts.next())
            {
                help.as_widget_mut()
                    .operate(child_tree, child_layout, renderer, operation);
            }
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        if self.action.is_none() {
            state.reset();
            return;
        }
        let now = if let Event::Window(window::Event::RedrawRequested(now)) = event {
            *now
        } else {
            Instant::now()
        };
        let bounds = layout.bounds();
        let over = cursor
            .position()
            .is_some_and(|point| viewport.contains(point) && bounds.contains(point));
        if self.handle_event(state, event, over, now, shell) {
            return;
        }
        state.advance(now, over, shell);
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        if bounds.intersection(viewport).is_none() {
            return;
        }
        let state = tree.state.downcast_ref::<State>();
        let (pointer_h, h, p) = if self.action.is_some() {
            (
                smoothstep(state.pointer_hover),
                smoothstep(state.highlight),
                smoothstep(state.press),
            )
        } else {
            (0.0, 0.0, 0.0)
        };
        if h > 0.0 {
            self.draw_chrome(renderer, bounds, pointer_h, h, p);
        }
        let hovered_title = mix_color(self.palette.empty_title, self.palette.text, h);
        let title_color = mix_color(hovered_title, self.palette.tool_active_icon, 0.28 * p);
        let help_color = mix_color(
            self.palette.empty_help,
            self.palette.text,
            (0.55 + 0.15 * p) * h,
        );
        let paint = |renderer: &mut Renderer| {
            let mut children = layout.children();
            if let Some(title_layout) = children.next() {
                self.title.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    &renderer::Style {
                        text_color: title_color,
                    },
                    title_layout,
                    cursor,
                    viewport,
                );
            }
            if let Some(help_layout) = children.next() {
                self.help.as_widget().draw(
                    &tree.children[1],
                    renderer,
                    theme,
                    &renderer::Style {
                        text_color: help_color,
                    },
                    help_layout,
                    cursor,
                    viewport,
                );
            }
        };
        if p > 0.0 {
            renderer.with_translation(Vector::new(0.0, 1.5 * p), paint);
        } else {
            paint(renderer);
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if self.action.is_some()
            && cursor
                .position()
                .is_some_and(|point| viewport.contains(point) && layout.bounds().contains(point))
        {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::None
        }
    }
}

fn goal(active: bool) -> f32 {
    if active { 1.0 } else { 0.0 }
}

fn settled(value: f32, active: bool) -> bool {
    (value - goal(active)).abs() <= 0.005
}

fn step(current: f32, active: bool, dt: f32, in_secs: f32, out_secs: f32) -> f32 {
    if active {
        (current + dt / in_secs).clamp(0.0, 1.0)
    } else {
        (current - dt / out_secs).clamp(0.0, 1.0)
    }
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mix_color(from: Color, to: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let alpha = from.a + (to.a - from.a) * t;
    if alpha <= f32::EPSILON {
        return Color::TRANSPARENT;
    }
    let inv = 1.0 - t;
    Color {
        r: ((from.r * from.a * inv + to.r * to.a * t) / alpha).clamp(0.0, 1.0),
        g: ((from.g * from.a * inv + to.g * to.a * t) / alpha).clamp(0.0, 1.0),
        b: ((from.b * from.a * inv + to.b * to.a * t) / alpha).clamp(0.0, 1.0),
        a: alpha.clamp(0.0, 1.0),
    }
}
