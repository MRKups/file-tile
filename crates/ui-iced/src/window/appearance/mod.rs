//! Shared renderer-owned visual tokens converted directly from the active preset.

use iced::widget::{button, container, rule, text_input};
use iced::{Border, Color, Shadow, Theme, Vector};

use crate::application::preferences::appearance::{
    ResolvedAppearance, Rgb8, SurfaceTexture, palette,
};
mod scrollbars;
pub(crate) use scrollbars::{WIDTH as SCROLLBAR_WIDTH, default_style as scrollable, scrollbar};
mod inspector;
pub(crate) use crate::widgets::scroll_area::scroll_area;
mod picker;
pub(crate) use picker::picker;
pub mod surface_texture;

pub(crate) use super::typography::{CONTROL_SIZE, HEADING_SIZE, SMALL_SIZE, TITLE_SIZE};
pub const MODAL_SCRIM: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.42);

#[derive(Debug, Clone, Copy)]
pub struct Palettes {
    pub window: Palette,
    pub boxes: Palette,
}

/// Renderer-owned conversion, copied explicitly into retained widget descriptions.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub typography: crate::window::typography::Style,
    pub surface_texture: SurfaceTexture,
    pub background: Color,
    pub contrast_background: Color,
    pub text: Color,
    pub on_accent: Color,
    pub path_text: Color,
    pub muted: Color,
    pub accent: Color,
    pub border: Color,
    pub selected_border: Color,
    pub tool_active: Color,
    pub tool_active_icon: Color,
    pub tool_hover: Color,
    pub navigation: Color,
    pub row_hover: Color,
    pub row_selected: Color,
    pub grid: Color,
    pub grid_hover: Color,
    pub grid_lit: Color,
    pub grid_invalid: Color,
    pub grid_border: Color,
    pub preview: Color,
    pub preview_invalid: Color,
    pub error: Color,
    pub remove_fill: Color,
    pub mode_text: Color,
    pub path_border: Color,
    pub editing_overlay: Color,
    pub tab_text: Color,
    pub empty_title: Color,
    pub empty_help: Color,
    pub folder_icon: Color,
    pub file_icon: Color,
    pub image_icon: Color,
}

pub fn resolve(appearance: ResolvedAppearance) -> Palettes {
    let accent = crate::platform::system_accent_color().map(|(r, g, b)| Rgb8::new(r, g, b));
    let resolved = palette::resolve_with_accent(appearance, accent);
    let mut boxes: Palette = resolved.boxes.into();
    boxes.surface_texture = appearance.surface_texture;
    Palettes {
        window: resolved.window.into(),
        boxes,
    }
}

pub(crate) fn system_appearance(
    mode: iced::theme::Mode,
) -> crate::application::preferences::appearance::SystemAppearance {
    match mode {
        iced::theme::Mode::Dark => {
            crate::application::preferences::appearance::SystemAppearance::Dark
        }
        iced::theme::Mode::Light | iced::theme::Mode::None => {
            crate::application::preferences::appearance::SystemAppearance::Light
        }
    }
}

pub fn color(rgb: Rgb8) -> Color {
    Color::from_rgb8(rgb.red, rgb.green, rgb.blue)
}

fn rgba(value: palette::Rgba) -> Color {
    Color::from_rgba(value.red, value.green, value.blue, value.alpha)
}

impl From<palette::Palette> for Palette {
    fn from(p: palette::Palette) -> Self {
        Self {
            typography: Default::default(),
            surface_texture: SurfaceTexture::default(),
            background: Color {
                a: p.background_alpha,
                ..color(p.background)
            },
            contrast_background: color(p.contrast_background),
            text: color(p.text),
            on_accent: color(p.on_accent),
            path_text: color(p.path_text),
            muted: color(p.muted),
            accent: color(p.accent),
            border: color(p.border),
            selected_border: color(p.selected_border),
            tool_active: rgba(p.tool_active),
            tool_active_icon: color(p.tool_active_icon),
            tool_hover: rgba(p.tool_hover),
            navigation: color(p.navigation),
            row_hover: rgba(p.row_hover),
            row_selected: rgba(p.row_selected),
            grid: rgba(p.grid),
            grid_hover: rgba(p.grid_hover),
            grid_lit: rgba(p.grid_lit),
            grid_invalid: rgba(p.grid_invalid),
            grid_border: rgba(p.grid_border),
            preview: color(p.preview),
            preview_invalid: color(p.preview_invalid),
            error: color(p.error),
            remove_fill: rgba(p.remove_fill),
            mode_text: color(p.mode_text),
            path_border: color(p.path_border),
            editing_overlay: color(p.editing_overlay),
            tab_text: color(p.tab_text),
            empty_title: color(p.empty_title),
            empty_help: color(p.empty_help),
            folder_icon: color(p.folder_icon),
            file_icon: color(p.file_icon),
            image_icon: color(p.image_icon),
        }
    }
}

impl Palettes {
    pub(crate) fn with_typography(
        mut self,
        preferences: crate::application::preferences::typography::TypographyPreferences,
    ) -> Self {
        self.window.typography = crate::window::typography::Style(preferences);
        self.boxes.typography = crate::window::typography::Style(preferences);
        self
    }
}

impl Palette {
    pub fn panel(self) -> container::Style {
        container::Style {
            background: Some(self.background.into()),
            text_color: Some(self.text),
            border: Border {
                color: self.border,
                width: 1.0,
                radius: 10.0.into(),
            },
            shadow: Shadow::default(),
            ..container::Style::default()
        }
    }

    pub fn modal_panel(self) -> container::Style {
        container::Style {
            background: Some(self.contrast_background.into()),
            text_color: Some(self.text),
            border: Border {
                color: self.border,
                width: 1.0,
                radius: 16.0.into(),
            },
            shadow: Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.18),
                offset: Vector::new(0.0, 10.0),
                blur_radius: 32.0,
            },
            ..container::Style::default()
        }
    }

    pub fn entry_panel(self, selected: bool, hovered: bool, interactive: bool) -> container::Style {
        container::Style {
            background: if interactive && selected {
                Some(self.row_selected.into())
            } else if interactive && hovered {
                Some(self.row_hover.into())
            } else {
                None
            },
            border: Border {
                radius: 5.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        }
    }

    pub fn path_field(self, status: text_input::Status, invalid: bool) -> text_input::Style {
        let focused = matches!(status, text_input::Status::Focused { .. });
        text_input::Style {
            background: if focused {
                self.row_hover
            } else {
                Color::TRANSPARENT
            }
            .into(),
            border: Border {
                color: if invalid {
                    self.error
                } else {
                    self.path_border
                },
                width: if focused || invalid { 1.0 } else { 0.0 },
                radius: 4.0.into(),
            },
            icon: self.navigation,
            placeholder: self.muted,
            value: self.path_text,
            selection: self.row_selected,
        }
    }

    pub fn grid_cell(self, lit: bool) -> container::Style {
        container::Style {
            background: Some(if lit { self.grid_lit } else { self.grid }.into()),
            border: Border {
                color: self.grid_border,
                width: 1.0,
                radius: 10.0.into(),
            },
            ..container::Style::default()
        }
    }

    pub fn configure_scrim(self) -> container::Style {
        container::Style {
            background: Some(
                Color {
                    a: 0.87,
                    ..self.contrast_background
                }
                .into(),
            ),
            border: Border {
                radius: 10.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        }
    }

    pub fn rule(self) -> rule::Style {
        rule::Style {
            color: self.border,
            radius: 0.0.into(),
            fill_mode: rule::FillMode::Full,
            snap: true,
        }
    }

    pub fn text_button(self, status: button::Status) -> button::Style {
        button::Style {
            text_color: self.navigation,
            background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                .then_some(self.row_hover.into()),
            ..button::Style::default()
        }
    }

    pub fn icon(self, icon: crate::window::icons::Icon) -> Color {
        use crate::window::icons::Icon;
        match icon {
            Icon::Folder => self.folder_icon,
            Icon::Image => self.image_icon,
            _ => self.file_icon,
        }
    }
}

/// Stock widget fallback. Explicit component palettes remain independent of it.
pub fn theme_for(palette: Palette) -> Theme {
    Theme::custom(
        "file-tile",
        iced::theme::Palette {
            background: palette.contrast_background,
            text: palette.text,
            primary: palette.accent,
            success: palette.tool_active_icon,
            warning: palette.preview_invalid,
            danger: palette.error,
        },
    )
}

#[cfg(test)]
pub fn theme() -> Theme {
    theme_for(resolve(Default::default()).window)
}
