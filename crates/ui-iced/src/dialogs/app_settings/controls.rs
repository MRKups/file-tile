//! Compact appearance inputs; the workspace renders draft colors directly.

use std::fmt;
use std::time::Duration;

use iced::widget::{button, container, pick_list, row, slider, text, tooltip};
use iced::{Center, Element, Fill, keyboard};

use crate::application::preferences::appearance::{
    BoxPercent, Preset, SurfaceTexture, SystemAppearance, ThemeChoice,
};

use super::appearance::{self, AppearanceEdit, Draft};
use super::style;
use crate::{
    Message,
    widgets::focus_control,
    window::appearance as a,
    window::icons::{self, Icon},
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct ThemeOption {
    choice: ThemeChoice,
    label: String,
}

impl fmt::Display for ThemeOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

pub(super) fn theme(
    draft: &Draft,
    slot: SystemAppearance,
    palette: a::Palette,
) -> Element<'static, Message> {
    let ty = palette.typography;
    let mut options: Vec<ThemeOption> = Preset::ALL
        .into_iter()
        .map(|preset| ThemeOption {
            choice: ThemeChoice::Preset(preset),
            label: preset.label().to_owned(),
        })
        .collect();
    options.extend(draft.user_themes.iter().map(|custom| ThemeOption {
        choice: ThemeChoice::Custom(custom.id),
        label: custom.name.clone(),
    }));
    let current = draft.theme_for(slot);
    let index = options
        .iter()
        .position(|item| item.choice == current)
        .unwrap_or(0);
    let selected = options.get(index).cloned();
    let edit = move |option: ThemeOption| {
        Message::AppAppearanceEdited(AppearanceEdit::Theme(slot, option.choice))
    };
    let last = options.len().saturating_sub(1);
    let keys = [
        (keyboard::key::Named::ArrowUp, index.saturating_sub(1)),
        (keyboard::key::Named::ArrowDown, (index + 1).min(last)),
        (keyboard::key::Named::Home, 0),
        (keyboard::key::Named::End, last),
    ]
    .into_iter()
    .map(|(key, idx)| {
        (
            key,
            keyboard::Modifiers::empty(),
            Message::AppAppearanceEdited(AppearanceEdit::Theme(slot, options[idx].choice)),
        )
    })
    .collect();
    let next = Message::AppAppearanceEdited(AppearanceEdit::Theme(
        slot,
        options[(index + 1) % options.len()].choice,
    ));
    let picker = pick_list(options, selected, edit)
        .width(Fill)
        .padding([9, 12])
        .text_size(ty.size(a::CONTROL_SIZE))
        .font(ty.font())
        .style(move |_, status| style::picker(palette, status))
        .menu_style(move |_| style::picker_menu(palette));
    let button_size =
        f32::from(text::LineHeight::default().to_absolute(ty.size(a::CONTROL_SIZE).into())) + 18.0;
    let open_msg = Message::AppAppearanceEdited(AppearanceEdit::OpenCustomThemes(slot));
    let wrench_face = container(icons::view(Icon::Wrench, 17.0, Some(palette.text), 1.0))
        .center_x(button_size)
        .center_y(button_size);
    let wrench_button = focus_control::themed(
        appearance::custom_themes_id(slot),
        open_msg.clone(),
        button(wrench_face)
            .width(button_size)
            .height(button_size)
            .padding(0)
            .on_press(open_msg)
            .style(move |_, status| style::button_style(palette, status, false, false)),
        palette,
    );
    let wrench = tooltip(
        wrench_button,
        ty.small("Custom themes"),
        tooltip::Position::Bottom,
    )
    .delay(Duration::from_millis(350))
    .gap(5)
    .padding(7)
    .style(move |_| palette.tooltip());
    row![
        focus_control::exclusive_keys(
            appearance::theme_id(slot),
            next,
            a::picker(palette, picker),
            keys,
            palette,
        ),
        wrench,
    ]
    .spacing(10)
    .align_y(Center)
    .into()
}

pub(super) fn transparency(value: u8, palette: a::Palette) -> Element<'static, Message> {
    transparency_slider(
        appearance::TRANSPARENCY_ID,
        100,
        value,
        AppearanceEdit::Transparency,
        palette,
    )
}

pub(super) fn box_transparency(value: u8, palette: a::Palette) -> Element<'static, Message> {
    transparency_slider(
        appearance::BOX_TRANSPARENCY_ID,
        BoxPercent::MAX,
        value,
        AppearanceEdit::BoxTransparency,
        palette,
    )
}

pub(super) fn surface_texture(
    current: SurfaceTexture,
    palette: a::Palette,
) -> Element<'static, Message> {
    let ty = palette.typography;
    let options = SurfaceTexture::ALL;
    let index = options
        .iter()
        .position(|item| *item == current)
        .unwrap_or(0);
    let last = options.len().saturating_sub(1);
    let keys = [
        (keyboard::key::Named::ArrowUp, index.saturating_sub(1)),
        (keyboard::key::Named::ArrowDown, (index + 1).min(last)),
        (keyboard::key::Named::Home, 0),
        (keyboard::key::Named::End, last),
    ]
    .into_iter()
    .map(|(key, idx)| {
        (
            key,
            keyboard::Modifiers::empty(),
            Message::AppAppearanceEdited(AppearanceEdit::SurfaceTexture(options[idx])),
        )
    })
    .collect();
    let next = Message::AppAppearanceEdited(AppearanceEdit::SurfaceTexture(
        options[(index + 1) % options.len()],
    ));
    let picker = pick_list(options, Some(current), |choice| {
        Message::AppAppearanceEdited(AppearanceEdit::SurfaceTexture(choice))
    })
    .width(Fill)
    .padding([9, 12])
    .text_size(ty.size(a::CONTROL_SIZE))
    .font(ty.font())
    .style(move |_, status| style::picker(palette, status))
    .menu_style(move |_| style::picker_menu(palette));
    focus_control::exclusive_keys(
        appearance::SURFACE_TEXTURE_ID,
        next,
        a::picker(palette, picker),
        keys,
        palette,
    )
}

fn transparency_slider(
    id: &'static str,
    max: u8,
    value: u8,
    make_edit: fn(u8) -> AppearanceEdit,
    palette: a::Palette,
) -> Element<'static, Message> {
    let value = value.min(max);
    let message = move |value| Message::AppAppearanceEdited(make_edit(value));
    let keys = [
        (keyboard::key::Named::ArrowLeft, value.saturating_sub(1)),
        (keyboard::key::Named::ArrowDown, value.saturating_sub(1)),
        (
            keyboard::key::Named::ArrowRight,
            value.saturating_add(1).min(max),
        ),
        (
            keyboard::key::Named::ArrowUp,
            value.saturating_add(1).min(max),
        ),
        (keyboard::key::Named::Home, 0),
        (keyboard::key::Named::End, max),
    ]
    .into_iter()
    .map(|(key, value)| (key, keyboard::Modifiers::empty(), message(value)))
    .collect();
    let readout = if value == 0 {
        "Off".to_owned()
    } else {
        format!("{value}%")
    };
    row![
        transparency_step(id, "less", "-", value.saturating_sub(1), make_edit, palette),
        focus_control::exclusive_keys(
            id,
            message(value),
            slider(0..=max, value, message)
                .step(1_u8)
                .style(style::slider_style(palette)),
            keys,
            palette,
        ),
        transparency_step(
            id,
            "more",
            "+",
            value.saturating_add(1).min(max),
            make_edit,
            palette,
        ),
        palette
            .typography
            .control(readout)
            .color(palette.text)
            .width(42)
            .align_x(iced::Right),
    ]
    .spacing(8)
    .align_y(Center)
    .padding([7, 0])
    .into()
}

fn transparency_step(
    base_id: &'static str,
    direction: &'static str,
    label: &'static str,
    value: u8,
    make_edit: fn(u8) -> AppearanceEdit,
    palette: a::Palette,
) -> Element<'static, Message> {
    let id = format!("{base_id}-{direction}");
    let message = Message::AppAppearanceEdited(make_edit(value));
    focus_control::themed(
        id,
        message.clone(),
        button(
            palette
                .typography
                .control(label)
                .width(Fill)
                .align_x(Center),
        )
        .width(26)
        .padding([4, 0])
        .style(move |_, status| style::button_style(palette, status, false, false))
        .on_press(message),
        palette,
    )
}
