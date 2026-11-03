//! Shared path pill and integrated Choose control for Settings and Configure.
use crate::{
    Message,
    widgets::focus_control,
    window::appearance as a,
    window::icons::{self, Icon},
};
use iced::widget::{button, container, row, text_input};
use iced::{Border, Center, Element, Fill};

pub(crate) fn field_style(
    palette: a::Palette,
    status: text_input::Status,
    invalid: bool,
) -> text_input::Style {
    let mut field = crate::dialogs::app_settings::style::field(palette, status, invalid);
    field.border.radius = iced::border::left(24.0);
    if !invalid && !matches!(status, text_input::Status::Focused { .. }) {
        field.border.width = 0.0;
    }
    field
}

pub(crate) fn view<'a>(
    field: Element<'a, Message>,
    id: &'static str,
    action: Option<Message>,
    palette: a::Palette,
) -> Element<'a, Message> {
    view_with_label(field, "Choose", id, action, palette)
}

pub(crate) fn view_with_label<'a>(
    field: Element<'a, Message>,
    label: &'a str,
    id: &'static str,
    action: Option<Message>,
    palette: a::Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    let icon_color = if action.is_some() {
        palette.text
    } else {
        palette.muted
    };
    let choose = focus_control::themed_optional(
        id,
        action.clone(),
        button(
            container(
                row![
                    icons::view(Icon::Folder, ty.icon(14.0), Some(icon_color), 1.0),
                    ty.control(label)
                ]
                .spacing(6)
                .align_y(Center),
            )
            .center_x(Fill)
            .center_y(Fill),
        )
        .padding([0, 8])
        .width(112)
        .height(Fill)
        .on_press_maybe(action)
        .style(move |_, status| {
            let mut button =
                crate::dialogs::app_settings::style::button_style(palette, status, false, false);
            button.background = Some(palette.row_hover.into());
            button.border.radius = iced::border::right(24.0);
            button.border.width = 0.0;
            button
        }),
        palette,
    );
    container(row![field, choose].height(ty.row(40.0)).align_y(Center))
        .padding(1)
        .style(move |_| container::Style {
            background: Some(palette.contrast_background.into()),
            border: Border {
                color: palette.border,
                width: 1.0,
                radius: 24.0.into(),
            },
            ..Default::default()
        })
        .into()
}
