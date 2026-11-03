//! Theme-derived Configure surfaces and controls, including their popup menus.
use super::{Palette, palette};
use crate::{Desktop, dialogs::app_settings::style};
use iced::{
    Border, Color, Shadow, Vector,
    widget::{button, container, overlay::menu, pick_list, text_input},
};

impl Desktop {
    pub(crate) fn configure_palette(&self) -> Palette {
        let accent = crate::platform::system_accent_color()
            .map(|(r, g, b)| crate::application::preferences::appearance::Rgb8::new(r, g, b));
        let mut palette: Palette =
            palette::configure_with_accent(self.preview_appearance(), accent).into();
        palette.typography = crate::window::typography::Style(self.application.typography());
        palette
    }

    pub(crate) fn box_palette(&self) -> Palette {
        super::resolve(self.preview_appearance())
            .with_typography(self.application.typography())
            .boxes
    }
}

impl Palette {
    pub fn inspector_panel(self) -> container::Style {
        container::Style {
            background: Some(self.contrast_background.into()),
            border: Border {
                radius: 14.0.into(),
                ..self.panel().border
            },
            shadow: Shadow {
                color: Color::BLACK.scale_alpha(0.12),
                offset: Vector::new(0.0, 8.0),
                blur_radius: 20.0,
            },
            ..self.panel()
        }
    }

    pub fn inspector_field(self, status: text_input::Status, invalid: bool) -> text_input::Style {
        let mut field = style::field(self, status, invalid);
        field.background = self.row_hover.into();
        field.border.radius = 6.0.into();
        field
    }

    pub fn inspector_picker(self, _: pick_list::Status) -> pick_list::Style {
        pick_list::Style {
            text_color: self.text,
            placeholder_color: self.muted,
            handle_color: self.muted,
            background: self.row_hover.into(),
            border: Border {
                color: self.border,
                width: 1.0,
                radius: 6.0.into(),
            },
        }
    }

    pub fn picker_menu(self) -> menu::Style {
        menu::Style {
            background: self.contrast_background.into(),
            text_color: self.text,
            selected_text_color: self.text,
            selected_background: self.row_selected.into(),
            border: Border {
                radius: 6.0.into(),
                color: self.border,
                width: 1.0,
            },
            shadow: Shadow::default(),
        }
    }

    pub fn inspector_button(self, status: button::Status, primary: bool) -> button::Style {
        style::button_style(self, status, primary, false)
    }

    pub fn tooltip(self) -> container::Style {
        let panel = self.panel();
        container::Style {
            background: Some(self.contrast_background.into()),
            border: Border {
                radius: 6.0.into(),
                ..panel.border
            },
            ..panel
        }
    }
}
