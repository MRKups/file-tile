//! Opaque Settings chrome derived from the current appearance preview.
use iced::widget::{button, container, overlay::menu, pick_list, slider, text_input};
use iced::{Border, Theme};

use crate::window::appearance::{self, Palette};

pub(super) fn panel(palette: Palette) -> container::Style {
    let mut panel = palette.panel();
    panel.background = Some(palette.contrast_background.into());
    panel.border.radius = 16.0.into();
    // The scrim/border separate the panel without full-panel software shadow work.
    panel
}

pub(crate) fn field(
    palette: Palette,
    status: text_input::Status,
    invalid: bool,
) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused { .. });
    let mut style = palette.path_field(status, invalid);
    style.background = if focused {
        palette.row_hover
    } else {
        palette.contrast_background
    }
    .into();
    style.value = palette.text;
    style.border = Border {
        color: if invalid {
            palette.error
        } else if focused {
            palette.selected_border
        } else {
            palette.border
        },
        width: if focused { 2.0 } else { 1.0 },
        radius: 8.0.into(),
    };
    style
}

pub(crate) fn button_style(
    palette: Palette,
    status: button::Status,
    primary: bool,
    selected: bool,
) -> button::Style {
    if status == button::Status::Disabled {
        return button::Style {
            background: Some(palette.row_hover.into()),
            text_color: palette.muted,
            border: Border {
                color: palette.border,
                width: 1.0,
                radius: 8.0.into(),
            },
            ..button::Style::default()
        };
    }
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: if primary {
            Some(
                iced::Color {
                    a: if hovered { 0.88 } else { 1.0 },
                    ..palette.accent
                }
                .into(),
            )
        } else if selected {
            Some(palette.tool_active.into())
        } else if hovered {
            Some(palette.row_hover.into())
        } else {
            None
        },
        text_color: if primary {
            palette.on_accent
        } else if selected {
            palette.tool_active_icon
        } else {
            palette.text
        },
        border: Border {
            color: if primary {
                palette.accent
            } else {
                palette.border
            },
            width: 1.0,
            radius: 8.0.into(),
        },
        ..button::Style::default()
    }
}

pub(super) fn slider_style(palette: Palette) -> impl Fn(&Theme, slider::Status) -> slider::Style {
    let theme = appearance::theme_for(palette);
    move |_, status| slider::default(&theme, status)
}

pub(crate) fn picker(palette: Palette, status: pick_list::Status) -> pick_list::Style {
    let active = !matches!(status, pick_list::Status::Active);
    pick_list::Style {
        text_color: palette.text,
        placeholder_color: palette.muted,
        handle_color: palette.muted,
        background: if active {
            palette.row_hover
        } else {
            palette.contrast_background
        }
        .into(),
        border: Border {
            color: palette.border,
            width: 1.0,
            radius: 8.0.into(),
        },
    }
}

pub(crate) fn picker_menu(palette: Palette) -> menu::Style {
    menu::Style {
        background: palette.contrast_background.into(),
        text_color: palette.text,
        selected_text_color: palette.text,
        selected_background: palette.row_selected.into(),
        border: Border {
            color: palette.border,
            width: 1.0,
            radius: 8.0.into(),
        },
        shadow: Default::default(),
    }
}
