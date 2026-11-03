//! macOS-style pill toggle switch with widget-local slide transition.

use std::time::{Duration, Instant};

use iced::advanced::Renderer as _;
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, mouse, renderer,
    widget::{Tree, tree},
};
use iced::{
    Border, Color, Element, Event, Length, Rectangle, Renderer, Shadow, Size, Theme, Vector, window,
};

use crate::{Message, window::appearance::Palette};

const WIDTH: f32 = 32.0;
const HEIGHT: f32 = 18.0;
const INSET: f32 = 2.0;
const PRESS_STRETCH: f32 = 2.5;
const SLIDE_DURATION: Duration = Duration::from_millis(140);

pub(crate) fn toggle_switch<'a>(toggled: bool, palette: Palette) -> ToggleSwitch<'a> {
    ToggleSwitch {
        identity: 0,
        toggled,
        on_toggle: None,
        palette,
    }
}

pub(crate) struct ToggleSwitch<'a> {
    identity: u64,
    toggled: bool,
    on_toggle: Option<Box<dyn Fn(bool) -> Message + 'a>>,
    palette: Palette,
}

impl<'a> ToggleSwitch<'a> {
    /// Resets transition state without animating when a retained slot switches owner.
    pub(crate) fn identity(mut self, identity: u64) -> Self {
        self.identity = identity;
        self
    }

    pub(crate) fn on_toggle(mut self, on_toggle: impl Fn(bool) -> Message + 'a) -> Self {
        self.on_toggle = Some(Box::new(on_toggle));
        self
    }

    pub(crate) fn on_toggle_maybe(
        mut self,
        on_toggle: Option<impl Fn(bool) -> Message + 'a>,
    ) -> Self {
        self.on_toggle = on_toggle.map(|handler| Box::new(handler) as _);
        self
    }
}

struct State {
    identity: u64,
    target: bool,
    from_pos: f32,
    current_pos: f32,
    started_at: Option<Instant>,
    pressed: bool,
    hovered: bool,
}

impl State {
    fn new(identity: u64, toggled: bool) -> Self {
        let pos = target_position(toggled);
        Self {
            identity,
            target: toggled,
            from_pos: pos,
            current_pos: pos,
            started_at: None,
            pressed: false,
            hovered: false,
        }
    }

    fn advance(&mut self, now: Instant, shell: &mut Shell<'_, Message>) {
        let Some(started_at) = self.started_at else {
            return;
        };
        let target_pos = target_position(self.target);
        let elapsed = now.saturating_duration_since(started_at);
        let raw = (elapsed.as_secs_f32() / SLIDE_DURATION.as_secs_f32()).clamp(0.0, 1.0);
        if raw >= 1.0 {
            self.current_pos = target_pos;
            self.from_pos = target_pos;
            self.started_at = None;
        } else {
            let eased = 1.0 - (1.0 - raw).powi(3);
            self.current_pos = self.from_pos + (target_pos - self.from_pos) * eased;
            shell.request_redraw();
        }
    }
}

fn target_position(toggled: bool) -> f32 {
    if toggled { 1.0 } else { 0.0 }
}

impl Widget<Message, Theme, Renderer> for ToggleSwitch<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::new(self.identity, self.toggled))
    }

    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        if state.identity != self.identity {
            *state = State::new(self.identity, self.toggled);
            return;
        }
        if state.target != self.toggled {
            state.from_pos = state.current_pos;
            state.target = self.toggled;
            state.started_at = Some(Instant::now());
        }
        if self.on_toggle.is_none() {
            state.pressed = false;
            state.hovered = false;
        }
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(WIDTH), Length::Fixed(HEIGHT))
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, WIDTH, HEIGHT)
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
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        match event {
            Event::Window(window::Event::RedrawRequested(now)) => {
                state.advance(*now, shell);
            }
            Event::Mouse(mouse_event) if self.on_toggle.is_some() => {
                let over = cursor.is_over(layout.bounds());
                let hovered = over && !matches!(mouse_event, mouse::Event::CursorLeft);
                if state.hovered != hovered {
                    state.hovered = hovered;
                    shell.request_redraw();
                }
                match mouse_event {
                    mouse::Event::ButtonPressed(mouse::Button::Left) if over => {
                        state.pressed = true;
                        shell.capture_event();
                        shell.request_redraw();
                    }
                    mouse::Event::ButtonReleased(mouse::Button::Left) if state.pressed => {
                        state.pressed = false;
                        shell.capture_event();
                        shell.request_redraw();
                        if over && let Some(on_toggle) = &self.on_toggle {
                            shell.publish(on_toggle(!self.toggled));
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let enabled = self.on_toggle.is_some();
        let pos = state.current_pos.clamp(0.0, 1.0);
        let colors = switch_colors(self.palette, pos, enabled, enabled && state.hovered);

        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: Border {
                    color: colors.track_border,
                    width: 1.0,
                    radius: (bounds.height * 0.5).into(),
                },
                ..renderer::Quad::default()
            },
            colors.track_fill,
        );

        let stretch = if enabled && state.pressed {
            PRESS_STRETCH
        } else {
            0.0
        };
        let knob_height = (bounds.height - INSET * 2.0).max(0.0);
        let knob_width = knob_height + stretch;
        let travel = (bounds.width - INSET * 2.0 - knob_width).max(0.0);
        let knob_bounds = Rectangle {
            x: bounds.x + INSET + pos * travel,
            y: bounds.y + INSET,
            width: knob_width,
            height: knob_height,
        };

        renderer.fill_quad(
            renderer::Quad {
                bounds: knob_bounds,
                border: Border {
                    color: colors.knob_border,
                    width: 0.5,
                    radius: (knob_height * 0.5).into(),
                },
                shadow: colors.knob_shadow,
                snap: state.started_at.is_none() && !state.pressed,
            },
            colors.knob_fill,
        );
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if self.on_toggle.is_some() && cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::None
        }
    }
}

impl<'a> From<ToggleSwitch<'a>> for Element<'a, Message> {
    fn from(widget: ToggleSwitch<'a>) -> Self {
        Element::new(widget)
    }
}

struct SwitchColors {
    track_fill: Color,
    track_border: Color,
    knob_fill: Color,
    knob_border: Color,
    knob_shadow: Shadow,
}

fn switch_colors(palette: Palette, pos: f32, enabled: bool, hovered: bool) -> SwitchColors {
    let dark = luminance(palette.contrast_background) < 0.25;
    let (off_fill, off_border) = if dark {
        let base = mix(palette.border, palette.muted, 0.14);
        (
            if hovered {
                mix(base, palette.text, 0.10)
            } else {
                base
            },
            mix(palette.border, palette.muted, 0.26),
        )
    } else {
        (
            mix(
                palette.contrast_background,
                palette.border,
                if hovered { 0.86 } else { 0.70 },
            ),
            mix(palette.border, palette.muted, 0.14),
        )
    };
    let on_fill = if hovered {
        mix(palette.accent, palette.text, 0.10)
    } else {
        palette.accent
    };
    let mut track_fill = mix(off_fill, on_fill, pos);
    let mut track_border = mix(off_border, on_fill, pos);
    if !enabled {
        track_fill = mix(palette.contrast_background, track_fill, 0.45);
        track_border = mix(palette.contrast_background, track_border, 0.45);
    }
    let knob_fill = if enabled {
        Color::WHITE
    } else {
        mix(
            palette.contrast_background,
            Color::WHITE,
            if dark { 0.42 } else { 0.82 },
        )
    };
    SwitchColors {
        track_fill,
        track_border,
        knob_fill,
        knob_border: Color::BLACK.scale_alpha(if enabled { 0.08 } else { 0.05 }),
        knob_shadow: Shadow {
            color: Color::BLACK.scale_alpha(if enabled { 0.22 } else { 0.08 }),
            offset: Vector::new(0.0, 1.0),
            blur_radius: if enabled { 2.5 } else { 1.5 },
        },
    }
}

fn mix(base: Color, tint: Color, weight: f32) -> Color {
    let weight = weight.clamp(0.0, 1.0);
    let inv = 1.0 - weight;
    Color {
        r: base.r * inv + tint.r * weight,
        g: base.g * inv + tint.g * weight,
        b: base.b * inv + tint.b * weight,
        a: base.a * inv + tint.a * weight,
    }
}

fn luminance(color: Color) -> f32 {
    let channel = |v: f32| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
}
