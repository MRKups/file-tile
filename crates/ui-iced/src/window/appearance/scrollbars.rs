//! Shared capsule thumbs with transparent tracks and unchanged pointer targets.
use super::{Palette, theme_for};
use iced::{Border, Color, Theme, widget::scrollable};

pub(crate) const WIDTH: f32 = 10.0;

pub(crate) fn scrollbar() -> scrollable::Scrollbar {
    scrollable::Scrollbar::new().width(WIDTH).scroller_width(5)
}

pub(crate) fn default_style(_: &Theme, status: scrollable::Status) -> scrollable::Style {
    let palette = super::resolve(Default::default()).boxes;
    style(&theme_for(palette), palette.text, status)
}

impl Palette {
    pub fn scrollable(self) -> impl Fn(&Theme, scrollable::Status) -> scrollable::Style {
        let theme = theme_for(self);
        move |_, status| style(&theme, self.text, status)
    }
}

pub(super) fn style(theme: &Theme, color: Color, status: scrollable::Status) -> scrollable::Style {
    let (horizontal, vertical) = match status {
        scrollable::Status::Hovered {
            is_horizontal_scrollbar_hovered,
            is_vertical_scrollbar_hovered,
            ..
        } => (
            is_horizontal_scrollbar_hovered,
            is_vertical_scrollbar_hovered,
        ),
        scrollable::Status::Dragged {
            is_horizontal_scrollbar_dragged,
            is_vertical_scrollbar_dragged,
            ..
        } => (
            is_horizontal_scrollbar_dragged,
            is_vertical_scrollbar_dragged,
        ),
        _ => (false, false),
    };
    let rail = |active| scrollable::Rail {
        background: None,
        border: Border::default(),
        scroller: scrollable::Scroller {
            background: color.scale_alpha(if active { 0.55 } else { 0.28 }).into(),
            border: Border {
                radius: 5.0.into(),
                ..Border::default()
            },
        },
    };
    scrollable::Style {
        vertical_rail: rail(vertical),
        horizontal_rail: rail(horizontal),
        gap: None,
        ..scrollable::default(theme, status)
    }
}
