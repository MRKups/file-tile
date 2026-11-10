//! Passive bottom-center keystroke overlay for Browse type-to-jump navigation.

use std::time::Instant;

use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, overlay, renderer,
    widget::{Operation, Tree},
};
use iced::widget::{container, pin, row, space, text};
use iced::{
    Alignment, Border, Color, Element, Event, Font, Length, Padding, Rectangle, Renderer, Shadow,
    Size, Theme, Vector, font, mouse, window,
};

use crate::{Desktop, Message, typography::CAPTION_SIZE, window::appearance as a};

const WINDOW_BOTTOM_OFFSET: f32 = 28.0;
const WINDOW_SIDE_MARGIN: f32 = 24.0;
const MAX_QUERY_WIDTH: f32 = 320.0;
const COUNT_FONT_SIZE: f32 = CAPTION_SIZE;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Placement {
    Window,
    InBox {
        bounds: Rectangle,
        bottom_offset: f32,
    },
    Hidden,
}

pub(crate) fn view(desktop: &Desktop, palette: a::Palette) -> Element<'_, Message> {
    if desktop.modal_open() {
        return space().into();
    }
    let placement = desktop.type_jump_hud_placement();
    let (glow_clip, matched_boxes) = desktop.type_jump_matched_boxes();
    render(
        desktop
            .type_jump
            .hud_props(placement, glow_clip, matched_boxes, palette),
    )
}

pub(crate) struct Props<'a> {
    pub query: &'a str,
    pub match_count: usize,
    pub placement: Placement,
    pub glow_clip: Option<Rectangle>,
    pub matched_boxes: Vec<Rectangle>,
    pub fade: f32,
    pub fade_start: Instant,
    pub fading: bool,
    pub on_fade_start: fn(Instant) -> Message,
    pub palette: a::Palette,
}

fn argon_medium() -> Font {
    Font {
        weight: font::Weight::Medium,
        ..Font::with_name("Monaspace Argon")
    }
}

pub(crate) fn render<'a>(props: Option<Props<'a>>) -> Element<'a, Message> {
    let Some(props) = props else {
        return space().into();
    };
    let fade = props.fade.clamp(0.0, 1.0);
    if props.query.is_empty() || fade <= 0.001 {
        return space().into();
    }
    let content = place_card(&props, fade);

    Element::new(PassiveHud {
        content,
        glow_clip: props.glow_clip,
        matched_boxes: props.matched_boxes,
        fade,
        accent: props.palette.accent,
        fade_start: props.fade_start,
        fading: props.fading,
        on_fade_start: props.on_fade_start,
    })
}

fn place_card<'a>(props: &Props<'a>, fade: f32) -> Element<'a, Message> {
    match props.placement {
        Placement::Hidden => space().into(),
        Placement::Window => {
            let hud_card = build_card(props, fade, MAX_QUERY_WIDTH);
            container(hud_card)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::End)
                .padding(Padding {
                    top: 0.0,
                    right: WINDOW_SIDE_MARGIN,
                    bottom: WINDOW_BOTTOM_OFFSET,
                    left: WINDOW_SIDE_MARGIN,
                })
                .into()
        }
        Placement::InBox {
            bounds,
            bottom_offset,
        } => {
            let side_margin = (bounds.width * 0.06).clamp(8.0, 16.0);
            let max_query = (bounds.width - side_margin * 2.0 - 48.0).clamp(32.0, MAX_QUERY_WIDTH);
            let hud_card = build_card(props, fade, max_query);
            pin(container(hud_card)
                .width(bounds.width)
                .height(bounds.height)
                .align_x(Alignment::Center)
                .align_y(Alignment::End)
                .padding(Padding {
                    top: 0.0,
                    right: side_margin,
                    bottom: bottom_offset,
                    left: side_margin,
                })
                .clip(true))
            .x(bounds.x)
            .y(bounds.y)
            .into()
        }
    }
}

fn build_card<'a>(props: &Props<'a>, fade: f32, max_query_width: f32) -> Element<'a, Message> {
    let palette = props.palette;
    let ty = palette.typography;
    let matched = props.match_count > 0;
    let query_color = if matched { palette.text } else { palette.muted }.scale_alpha(fade);
    let count_color = if matched {
        palette.muted
    } else {
        palette.error
    }
    .scale_alpha(0.85 * fade);
    let query_label = container(
        text(props.query)
            .font(argon_medium())
            .size(ty.size(a::CONTROL_SIZE))
            .color(query_color)
            .align_x(iced::alignment::Horizontal::Right)
            .wrapping(text::Wrapping::None),
    )
    .max_width(max_query_width)
    .clip(true);

    let count_label = text(props.match_count.to_string())
        .font(argon_medium())
        .size(ty.size(COUNT_FONT_SIZE))
        .color(count_color)
        .wrapping(text::Wrapping::None);
    let content_row = row![query_label, count_label]
        .spacing(10.0)
        .align_y(Alignment::Center);

    let border_color = if matched {
        palette.border.scale_alpha(fade)
    } else {
        palette.error.scale_alpha(0.55 * fade)
    };
    container(content_row)
        .padding(Padding::from([6.0, 12.0]))
        .style(move |_| container::Style {
            background: Some(palette.contrast_background.scale_alpha(0.88 * fade).into()),
            border: Border {
                color: border_color,
                width: 1.0,
                radius: 8.0.into(),
            },
            shadow: Shadow {
                color: Color::BLACK.scale_alpha(0.14 * fade),
                offset: Vector::new(0.0, 4.0),
                blur_radius: 14.0,
            },
            ..container::Style::default()
        })
        .into()
}

struct PassiveHud<'a> {
    content: Element<'a, Message>,
    glow_clip: Option<Rectangle>,
    matched_boxes: Vec<Rectangle>,
    fade: f32,
    accent: Color,
    fade_start: Instant,
    fading: bool,
    on_fade_start: fn(Instant) -> Message,
}

impl Widget<Message, Theme, Renderer> for PassiveHud<'_> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[self.content.as_widget()]);
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
        _tree: &mut Tree,
        _layout: Layout<'_>,
        _renderer: &Renderer,
        _operation: &mut dyn Operation,
    ) {
    }

    fn update(
        &mut self,
        _tree: &mut Tree,
        event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        if let Event::Window(window::Event::RedrawRequested(now)) = event
            && !self.fading
        {
            if *now >= self.fade_start {
                shell.publish((self.on_fade_start)(*now));
            } else {
                shell.request_redraw_at(self.fade_start);
            }
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        mouse::Interaction::None
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if let Some(clip) = self.glow_clip
            && !self.matched_boxes.is_empty()
            && let Some(layer) = clip.intersection(viewport)
        {
            use iced::advanced::Renderer as _;
            renderer.with_layer(layer, |renderer| {
                for &bounds in &self.matched_boxes {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds,
                            border: Border {
                                color: self.accent.scale_alpha(0.72 * self.fade),
                                width: 1.5,
                                radius: 10.0.into(),
                            },
                            shadow: Shadow {
                                color: self.accent.scale_alpha(0.30 * self.fade),
                                offset: Vector::ZERO,
                                blur_radius: 16.0,
                            },
                            snap: false,
                        },
                        self.accent.scale_alpha(0.055 * self.fade),
                    );
                }
            });
        }
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            mouse::Cursor::Unavailable,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        _tree: &'b mut Tree,
        _layout: Layout<'b>,
        _renderer: &Renderer,
        _viewport: &Rectangle,
        _translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        None
    }
}
