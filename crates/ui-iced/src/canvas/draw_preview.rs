use iced::widget::{container, pin, space};
use iced::{Border, Color, Element};

use crate::domain::GridRect;

use crate::{Message, canvas::geometry::CanvasGeometry, window::appearance as a};

pub fn origin(
    geometry: CanvasGeometry,
    rect: GridRect,
    palette: a::Palette,
) -> Element<'static, Message> {
    let rect = geometry.rect(rect);
    pin(container(space())
        .width(rect.width)
        .height(rect.height)
        .style(move |_| container::Style {
            background: Some(
                Color {
                    a: 0.72,
                    ..palette.contrast_background
                }
                .into(),
            ),
            ..container::Style::default()
        }))
    .x(rect.x)
    .y(rect.y)
    .into()
}

pub fn view(
    geometry: CanvasGeometry,
    rect: GridRect,
    valid: bool,
    palette: a::Palette,
) -> Element<'static, Message> {
    let rect = geometry.rect(rect);
    let color = if valid {
        palette.preview
    } else {
        palette.preview_invalid
    };
    highlight(rect, color)
}

pub fn layout(
    geometry: CanvasGeometry,
    rect: GridRect,
    valid: bool,
    palette: a::Palette,
) -> Element<'static, Message> {
    let color = if valid {
        palette.selected_border
    } else {
        palette.preview_invalid
    };
    highlight(geometry.rect(rect), color)
}

fn highlight(rect: iced::Rectangle, color: Color) -> Element<'static, Message> {
    pin(container(space())
        .width(rect.width)
        .height(rect.height)
        .style(move |_| container::Style {
            background: Some(Color { a: 0.14, ..color }.into()),
            border: Border {
                color,
                width: 3.0,
                radius: 9.0.into(),
            },
            ..container::Style::default()
        }))
    .x(rect.x)
    .y(rect.y)
    .into()
}
