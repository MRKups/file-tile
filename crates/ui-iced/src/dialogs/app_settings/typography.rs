//! Local typography controls. Accepted chrome does not change until Apply.
use super::{font_picker, style, view::setting};
use crate::application::preferences::typography::{
    TextSize, TextWeight, Typeface, TypographyPreferences,
};
use crate::{
    Message,
    widgets::focus_control,
    window::{appearance as a, system_font::SystemFont},
};
use iced::widget::{column, pick_list, row, slider};
use iced::{Element, Fill, keyboard};

pub(super) fn view(
    preferences: TypographyPreferences,
    picker_open: bool,
    picker_cursor: Option<usize>,
    width: f32,
    palette: a::Palette,
    system_font: &SystemFont,
) -> Element<'static, Message> {
    let ty = palette.typography;
    let family = font_picker::view(
        preferences.typeface,
        picker_open,
        picker_cursor,
        palette,
        system_font,
    );
    let size = size_control(
        "app-settings-text-size",
        preferences.size,
        move |size| {
            Message::AppTypographyChanged(TypographyPreferences {
                size,
                ..preferences
            })
        },
        palette,
    );
    let weight = weight_control(
        "app-settings-text-weight",
        system_font.weights_for(preferences.typeface),
        preferences.weight,
        move |weight| {
            Message::AppTypographyChanged(TypographyPreferences {
                weight,
                ..preferences
            })
        },
        palette,
    );
    let mut content = column![
        ty.heading("Typography").color(palette.text),
        setting("Font", "", family, width, palette),
        setting("Text size", "", size, width, palette),
        setting("Weight", "", weight, width, palette),
    ]
    .spacing(24);
    if let Some(notice) = fallback_notice(preferences.typeface, system_font) {
        content = content.push(ty.caption(notice).color(palette.error));
    }
    content.into()
}

pub(super) fn fallback_notice(typeface: Typeface, system_font: &SystemFont) -> Option<String> {
    if typeface == Typeface::SystemDefault
        && let Some(error) = &system_font.error
    {
        return Some(error.clone());
    }
    None
}

pub(super) fn size_control(
    id: &'static str,
    current: TextSize,
    on_change: impl Fn(TextSize) -> Message + Copy + 'static,
    palette: a::Palette,
) -> Element<'static, Message> {
    let value = current.get();
    let edit = move |raw| on_change(TextSize::new(raw).unwrap_or(current));
    let keys = [
        (
            keyboard::key::Named::ArrowLeft,
            value.saturating_sub(1).max(TextSize::MIN),
        ),
        (
            keyboard::key::Named::ArrowDown,
            value.saturating_sub(1).max(TextSize::MIN),
        ),
        (
            keyboard::key::Named::ArrowRight,
            value.saturating_add(1).min(TextSize::MAX),
        ),
        (
            keyboard::key::Named::ArrowUp,
            value.saturating_add(1).min(TextSize::MAX),
        ),
        (keyboard::key::Named::Home, TextSize::MIN),
        (keyboard::key::Named::End, TextSize::MAX),
    ]
    .into_iter()
    .map(|(key, next)| (key, keyboard::Modifiers::empty(), edit(next)))
    .collect();
    row![
        focus_control::exclusive_keys(
            id,
            edit(value),
            slider(TextSize::MIN..=TextSize::MAX, value, edit)
                .step(1_u8)
                .style(style::slider_style(palette)),
            keys,
            palette,
        ),
        palette
            .typography
            .control(format!("{value} pt"))
            .color(palette.text)
            .width(48)
            .align_x(iced::Right),
    ]
    .spacing(12)
    .align_y(iced::Center)
    .padding([8, 0])
    .into()
}

pub(super) fn weight_control(
    id: &'static str,
    options: Vec<TextWeight>,
    selected: TextWeight,
    edit: impl Fn(TextWeight) -> Message + Copy + 'static,
    palette: a::Palette,
) -> Element<'static, Message> {
    let index = options
        .iter()
        .position(|value| *value == selected)
        .unwrap_or(0);
    let keys = [
        (keyboard::key::Named::ArrowUp, index.saturating_sub(1)),
        (
            keyboard::key::Named::ArrowDown,
            (index + 1).min(options.len() - 1),
        ),
    ]
    .into_iter()
    .map(|(key, idx)| (key, keyboard::Modifiers::empty(), edit(options[idx])))
    .collect();
    focus_control::exclusive_keys(
        id,
        edit(options[(index + 1) % options.len()]),
        a::picker(
            palette,
            pick_list(options, Some(selected), edit)
                .width(Fill)
                .padding([9, 12])
                .font(palette.typography.font())
                .text_size(palette.typography.size(a::CONTROL_SIZE))
                .style(move |_, status| style::picker(palette, status))
                .menu_style(move |_| style::picker_menu(palette)),
        ),
        keys,
        palette,
    )
}
