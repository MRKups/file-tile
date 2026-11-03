//! Swapped modal view for creating, listing, and editing custom themes.

use iced::widget::{
    button, column, container, row, rule, scrollable, slider, space, text, text_input,
};
use iced::{Border, Center, Element, Fill, Size};

use crate::application::preferences::appearance::UserTheme;

use super::super::{appearance::AppearanceEdit, focus, layout, style};
use super::{CANCEL_ID, ColorSlot, DONE_ID, Edit, EditorDraft, EntryDraft, Hsv, NAME_ID, NEW_ID};
use crate::{Message, widgets::focus_control, window::appearance as a};

fn send(edit: Edit) -> Message {
    Message::AppAppearanceEdited(AppearanceEdit::Custom(edit))
}

pub(in crate::dialogs::app_settings) fn view<'a>(
    editor: &'a EditorDraft,
    size: Size,
    palette: a::Palette,
) -> Element<'a, Message> {
    let dimensions = layout::panel_size(size);
    let ty = palette.typography;
    let can_create = editor.entries.len() < UserTheme::MAX_THEMES;
    let header = row![
        ty.title("Custom Themes").color(palette.text),
        space().width(Fill),
        action_btn(
            NEW_ID,
            "New theme",
            can_create.then(|| send(Edit::Create)),
            false,
            palette,
        ),
    ]
    .spacing(12)
    .align_y(Center);
    let body = body_split(editor, dimensions.width, palette);
    let footer = row![
        space().width(Fill),
        action_btn(
            CANCEL_ID,
            "Cancel",
            Some(send(Edit::Cancel)),
            false,
            palette
        ),
        action_btn(DONE_ID, "Done", Some(send(Edit::Done)), true, palette),
    ]
    .spacing(10)
    .align_y(Center);
    let panel = column![
        header,
        rule::horizontal(1).style(move |_| palette.rule()),
        body,
        rule::horizontal(1).style(move |_| palette.rule()),
        footer,
    ]
    .spacing(16);
    container(
        container(panel)
            .padding(layout::padding(dimensions))
            .width(dimensions.width)
            .height(dimensions.height)
            .style(move |_| style::panel(palette)),
    )
    .padding(layout::MARGIN)
    .center_x(Fill)
    .center_y(Fill)
    .style(|_| container::Style {
        background: Some(a::MODAL_SCRIM.into()),
        ..container::Style::default()
    })
    .into()
}

fn body_split<'a>(
    editor: &'a EditorDraft,
    panel_width: f32,
    palette: a::Palette,
) -> Element<'a, Message> {
    let list_width = (panel_width * 0.27).clamp(160.0, 230.0);
    let list = theme_list(editor, list_width, palette);
    let detail = if let Some(entry) = editor.selected.and_then(|i| editor.entries.get(i)) {
        editor_pane(entry, editor.active_picker, editor.entries.len(), palette)
    } else {
        empty_pane(palette)
    };
    row![
        list,
        rule::vertical(1).style(move |_| palette.rule()),
        detail,
    ]
    .spacing(18)
    .width(Fill)
    .height(Fill)
    .into()
}

fn theme_list<'a>(
    editor: &'a EditorDraft,
    width: f32,
    palette: a::Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    let mut items = column![].spacing(4).width(Fill);
    if editor.entries.is_empty() {
        items = items.push(ty.text("No custom themes").color(palette.muted));
    } else {
        for (index, entry) in editor.entries.iter().enumerate() {
            items = items.push(theme_list_row(
                index,
                entry,
                editor.selected == Some(index),
                palette,
            ));
        }
    }
    a::scroll_area(container(items).padding(iced::Padding {
        right: 10.0,
        ..iced::Padding::ZERO
    }))
    .style(palette.scrollable())
    .direction(scrollable::Direction::Vertical(a::scrollbar()))
    .width(width)
    .height(Fill)
    .into()
}

fn theme_list_row<'a>(
    index: usize,
    entry: &'a EntryDraft,
    selected: bool,
    palette: a::Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    let spec = entry.spec();
    let chip = container(space().width(14).height(14)).style(move |_| container::Style {
        background: Some(a::color(spec.boxes).into()),
        border: Border {
            color: a::color(spec.accent),
            width: 1.5,
            radius: 3.0.into(),
        },
        ..container::Style::default()
    });
    let trimmed = entry.name.trim();
    let label = if trimmed.is_empty() {
        ty.control("Untitled")
            .color(palette.muted)
            .wrapping(text::Wrapping::None)
    } else {
        ty.control(trimmed)
            .color(palette.text)
            .wrapping(text::Wrapping::None)
    };
    let msg = send(Edit::Select(index));
    let item = button(
        container(row![chip, label].spacing(8).align_y(Center))
            .width(Fill)
            .clip(true),
    )
    .padding([7, 10])
    .width(Fill)
    .on_press(msg.clone())
    .style(move |_, status| button::Style {
        text_color: palette.text,
        background: Some(
            if selected {
                palette.row_selected
            } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                palette.row_hover
            } else {
                iced::Color::TRANSPARENT
            }
            .into(),
        ),
        border: Border {
            radius: 6.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    });
    focus_control::themed(format!("custom-theme-item-{index}"), msg, item, palette)
}

fn empty_pane(palette: a::Palette) -> Element<'static, Message> {
    let ty = palette.typography;
    container(
        column![
            ty.control("No custom themes defined.").color(palette.muted),
            action_btn(
                "custom-themes-empty-new",
                "Create from active theme",
                Some(send(Edit::Create)),
                true,
                palette,
            ),
        ]
        .spacing(12)
        .align_x(Center),
    )
    .center_x(Fill)
    .center_y(Fill)
    .into()
}

fn editor_pane<'a>(
    entry: &'a EntryDraft,
    active_picker: Option<(ColorSlot, Hsv)>,
    total: usize,
    palette: a::Palette,
) -> Element<'a, Message> {
    let mut colors = column![].spacing(10).width(Fill);
    for slot in ColorSlot::ALL {
        colors = colors.push(color_row(entry, slot, active_picker, palette));
    }
    if let Some(err) = entry.color_error {
        colors = colors.push(palette.typography.caption(err).color(palette.error));
    }
    let content = column![
        name_and_actions(entry, total < UserTheme::MAX_THEMES, palette),
        rule::horizontal(1).style(move |_| palette.rule()),
        colors,
    ]
    .spacing(16)
    .width(Fill);
    a::scroll_area(container(content).padding(iced::Padding {
        top: 2.0,
        right: 14.0,
        bottom: 10.0,
        left: 2.0,
    }))
    .id(focus::SCROLL_ID)
    .style(palette.scrollable())
    .direction(scrollable::Direction::Vertical(a::scrollbar()))
    .width(Fill)
    .height(Fill)
    .into()
}

fn name_and_actions<'a>(
    entry: &'a EntryDraft,
    can_duplicate: bool,
    palette: a::Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    let input = text_input("Theme name", &entry.name)
        .id(NAME_ID)
        .padding([8, 12])
        .size(ty.size(a::CONTROL_SIZE))
        .font(ty.font())
        .width(Fill)
        .on_input(|val| send(Edit::Name(val)))
        .style(move |_, status| style::field(palette, status, entry.name_error.is_some()));
    let bar = row![
        ty.control("Name").color(palette.text).width(100),
        input,
        action_btn(
            "custom-theme-duplicate",
            "Duplicate",
            can_duplicate.then(|| send(Edit::Duplicate)),
            false,
            palette,
        ),
        action_btn(
            "custom-theme-delete",
            "Delete",
            Some(send(Edit::Delete)),
            false,
            palette,
        ),
    ]
    .spacing(8)
    .align_y(Center);
    let mut section = column![bar].spacing(6);
    if let Some(error) = entry.name_error {
        section = section.push(ty.caption(error).color(palette.error));
    }
    section.into()
}

fn color_row<'a>(
    entry: &'a EntryDraft,
    slot: ColorSlot,
    active_picker: Option<(ColorSlot, Hsv)>,
    palette: a::Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    let field = &entry.colors[slot.index()];
    let open = active_picker.filter(|(open_slot, _)| *open_slot == slot);
    let swatch_color = a::color(field.color);
    let toggle_msg = send(Edit::TogglePicker(slot));
    let swatch = focus_control::themed(
        slot.swatch_id(),
        toggle_msg.clone(),
        button(space().width(Fill).height(Fill))
            .width(32)
            .height(32)
            .padding(0)
            .on_press(toggle_msg)
            .style(move |_, status| button::Style {
                background: Some(swatch_color.into()),
                border: Border {
                    color: if open.is_some()
                        || matches!(status, button::Status::Hovered | button::Status::Pressed)
                    {
                        palette.selected_border
                    } else {
                        palette.border
                    },
                    width: if open.is_some() { 2.0 } else { 1.0 },
                    radius: 6.0.into(),
                },
                ..button::Style::default()
            }),
        palette,
    );
    let invalid = !field.is_valid();
    let hex_input = text_input("#RRGGBB", &field.hex)
        .id(slot.input_id())
        .padding([7, 10])
        .size(ty.size(a::CONTROL_SIZE))
        .font(ty.font())
        .width(114)
        .on_input(move |val| send(Edit::Hex(slot, val)))
        .style(move |_, status| style::field(palette, status, invalid));
    let line = row![
        ty.control(slot.label()).color(palette.text).width(Fill),
        swatch,
        hex_input,
    ]
    .spacing(10)
    .align_y(Center);
    let mut block = column![line].spacing(8);
    if let Some((_, hsv)) = open {
        block = block.push(hsv_sliders(slot, hsv, palette));
    }
    block.into()
}

fn hsv_sliders(slot: ColorSlot, hsv: Hsv, palette: a::Palette) -> Element<'static, Message> {
    let hue_row = hsv_slider_row(
        "Hue",
        slider(0..=360, hsv.hue, move |hue| {
            send(Edit::Hsv(slot, Hsv { hue, ..hsv }))
        })
        .step(1_u16)
        .style(style::slider_style(palette)),
        format!("{} deg", hsv.hue),
        palette,
    );
    let sat_row = hsv_slider_row(
        "Saturation",
        slider(0..=100, hsv.saturation, move |saturation| {
            send(Edit::Hsv(slot, Hsv { saturation, ..hsv }))
        })
        .step(1_u8)
        .style(style::slider_style(palette)),
        format!("{}%", hsv.saturation),
        palette,
    );
    let val_row = hsv_slider_row(
        "Brightness",
        slider(0..=100, hsv.brightness, move |brightness| {
            send(Edit::Hsv(slot, Hsv { brightness, ..hsv }))
        })
        .step(1_u8)
        .style(style::slider_style(palette)),
        format!("{}%", hsv.brightness),
        palette,
    );
    container(column![hue_row, sat_row, val_row].spacing(6))
        .padding([10, 12])
        .width(Fill)
        .style(move |_| container::Style {
            background: Some(palette.row_hover.into()),
            border: Border {
                color: palette.border,
                width: 1.0,
                radius: 8.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn hsv_slider_row(
    label: &'static str,
    control: impl Into<Element<'static, Message>>,
    suffix: String,
    palette: a::Palette,
) -> Element<'static, Message> {
    let ty = palette.typography;
    row![
        ty.text(label).color(palette.muted).width(78),
        control.into(),
        ty.text(suffix)
            .color(palette.muted)
            .width(52)
            .align_x(iced::Right),
    ]
    .spacing(10)
    .align_y(Center)
    .into()
}

fn action_btn(
    id: &'static str,
    label: &'static str,
    message: Option<Message>,
    primary: bool,
    palette: a::Palette,
) -> Element<'static, Message> {
    focus_control::themed_optional(
        id,
        message.clone(),
        button(palette.typography.control(label))
            .padding([7, 14])
            .style(move |_, status| style::button_style(palette, status, primary, false))
            .on_press_maybe(message),
        palette,
    )
}
