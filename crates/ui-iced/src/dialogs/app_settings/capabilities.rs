//! Per-capability toggles in the Settings dialog.

use std::time::Duration;

use iced::widget::{column, container, mouse_area, row, tooltip};
use iced::{Element, mouse};

use crate::domain::{CapabilityKind, CapabilityPreferences};

use super::layout;
use crate::{
    Message,
    widgets::{focus_control, toggle_switch::toggle_switch},
    window::appearance as a,
};

pub(super) fn view(
    capabilities: CapabilityPreferences,
    width: f32,
    palette: a::Palette,
) -> Element<'static, Message> {
    let ty = palette.typography;
    let heading = tooltip(
        ty.heading("Capabilities").color(palette.text),
        ty.small("Turns on additional features and visual indicators inside the app."),
        tooltip::Position::Top,
    )
    .delay(Duration::from_millis(350))
    .gap(6)
    .padding(7)
    .style(move |_| palette.tooltip());

    column![heading]
        .extend(
            CapabilityKind::ALL
                .into_iter()
                .map(|kind| capability_row(kind, capabilities, width, palette)),
        )
        .spacing(24)
        .into()
}

const fn toggle_id(kind: CapabilityKind) -> &'static str {
    match kind {
        CapabilityKind::Git => "app-settings-cap-git",
        CapabilityKind::Languages => "app-settings-cap-languages",
        CapabilityKind::EnvFiles => "app-settings-cap-env-files",
    }
}

fn capability_row(
    kind: CapabilityKind,
    capabilities: CapabilityPreferences,
    width: f32,
    palette: a::Palette,
) -> Element<'static, Message> {
    let ty = palette.typography;
    let (left, right) = layout::column_widths(width);
    let value = capabilities.enabled(kind);
    let next_message = Message::AppCapabilitiesChanged(capabilities.with_enabled(kind, !value));
    let control = focus_control::themed(
        toggle_id(kind),
        next_message.clone(),
        mouse_area(toggle_switch(value, palette).on_toggle(move |next| {
            Message::AppCapabilitiesChanged(capabilities.with_enabled(kind, next))
        }))
        .on_press(next_message)
        .interaction(mouse::Interaction::Pointer),
        palette,
    );

    let explanation = column![
        ty.control(kind.label()).color(palette.text),
        ty.text(kind.description()).color(palette.muted),
    ]
    .spacing(4);

    let left_col: Element<'static, Message> = if let Some(tip) = kind.tooltip() {
        tooltip(
            container(explanation).width(left),
            container(ty.small(tip)).max_width(420),
            tooltip::Position::Top,
        )
        .delay(Duration::from_millis(350))
        .gap(6)
        .padding(7)
        .style(move |_| palette.tooltip())
        .into()
    } else {
        container(explanation).width(left).into()
    };

    row![left_col, container(control).width(right)]
        .spacing(36)
        .wrap()
        .vertical_spacing(12)
        .into()
}
