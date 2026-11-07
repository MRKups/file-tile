//! Per-capability toggles in the Settings dialog.

use iced::widget::{column, mouse_area};
use iced::{Element, mouse};

use crate::domain::{CapabilityKind, CapabilityPreferences};

use super::view::setting;
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
    let header = column![
        ty.heading("Capabilities").color(palette.text),
        ty.text(
            "Inspect folders for developer tools, runtimes, and project configuration files, and show active details in the box footer."
        )
        .color(palette.muted),
    ]
    .spacing(6);

    column![header]
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
        CapabilityKind::Python => "app-settings-cap-python",
        CapabilityKind::Node => "app-settings-cap-node",
        CapabilityKind::Rust => "app-settings-cap-rust",
        CapabilityKind::Go => "app-settings-cap-go",
        CapabilityKind::EnvFiles => "app-settings-cap-env-files",
    }
}

fn capability_row(
    kind: CapabilityKind,
    capabilities: CapabilityPreferences,
    width: f32,
    palette: a::Palette,
) -> Element<'static, Message> {
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
    setting(kind.label(), kind.description(), control, width, palette)
}
