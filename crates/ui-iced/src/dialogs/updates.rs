//! Sparkle 2 updater controls for Settings, About, and the macOS application menu.
//!
//! Automatic update checks live in Sparkle's bundle `NSUserDefaults` rather than the
//! workspace/preferences JSON draft, so toggling them takes effect immediately.

use std::time::Duration;

use iced::widget::{button, column, container, mouse_area, row, text::Wrapping, tooltip};
use iced::{Border, Center, Element, Fill, Task, mouse};

#[cfg(target_os = "macos")]
use crate::platform::updater::UpdaterStatus;

use crate::{
    Desktop, Message,
    widgets::{focus_control, toggle_switch::toggle_switch},
    window::appearance::Palette,
};

#[derive(Debug, Clone, Copy)]
pub(crate) enum Action {
    CheckNow,
    SetAutomatic(bool),
}

pub(crate) struct Updates {
    #[cfg(all(target_os = "macos", not(test)))]
    updater: Option<crate::platform::updater::ApplicationUpdater>,
    #[cfg(target_os = "macos")]
    status: UpdaterStatus,
    can_check: bool,
    automatic_checks: bool,
    error: Option<&'static str>,
}

impl Default for Updates {
    fn default() -> Self {
        Self {
            #[cfg(all(target_os = "macos", not(test)))]
            updater: None,
            #[cfg(target_os = "macos")]
            status: UpdaterStatus::Unbundled,
            can_check: false,
            automatic_checks: false,
            error: None,
        }
    }
}

impl Updates {
    pub(crate) fn can_check(&self) -> bool {
        self.can_check
    }

    pub(crate) fn is_ready(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            matches!(self.status, UpdaterStatus::Ready)
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    pub(crate) fn status_note(&self) -> Option<&str> {
        #[cfg(target_os = "macos")]
        {
            match &self.status {
                UpdaterStatus::Ready => None,
                UpdaterStatus::AwaitingPublicKey => {
                    Some("Set public_ed_key in packaging/sparkle.toml to activate updates.")
                }
                UpdaterStatus::Unbundled => Some("Available in packaged .app builds."),
                UpdaterStatus::Unavailable(reason) => Some(reason.as_str()),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            Some("Available in packaged macOS .app builds.")
        }
    }
}

impl Desktop {
    #[cfg(all(target_os = "macos", not(test)))]
    pub(crate) fn install_updater(&mut self) {
        if self.updates.updater.is_none() {
            self.updates.updater = Some(crate::platform::updater::ApplicationUpdater::install());
        }
        self.sync_updates();
    }

    pub(crate) fn sync_updates(&mut self) {
        #[cfg(all(target_os = "macos", not(test)))]
        if let Some(updater) = &self.updates.updater {
            let snapshot = updater.snapshot();
            self.updates.status = snapshot.status;
            self.updates.can_check = snapshot.can_check;
            self.updates.automatic_checks = snapshot.automatic_checks;
        }
    }

    pub(crate) fn updates_action(&mut self, action: Action) -> Task<Message> {
        match action {
            Action::CheckNow =>
            {
                #[cfg(all(target_os = "macos", not(test)))]
                if let Some(updater) = &self.updates.updater {
                    self.updates.error = updater.check_for_updates().err();
                    self.sync_updates();
                }
            }
            Action::SetAutomatic(enabled) => {
                #[cfg(all(target_os = "macos", not(test)))]
                if let Some(updater) = &self.updates.updater {
                    match updater.set_automatic_checks(enabled) {
                        Ok(current) => {
                            self.updates.automatic_checks = current;
                            self.updates.error = None;
                        }
                        Err(error) => {
                            self.updates.error = Some(error);
                        }
                    }
                    self.sync_updates();
                }
                #[cfg(not(all(target_os = "macos", not(test))))]
                let _ = enabled;
            }
        }
        Task::none()
    }
}

pub(crate) fn controls(updates: &Updates, palette: Palette) -> Element<'static, Message> {
    let ty = palette.typography;
    let note = updates.status_note();
    let pill = container(
        row![
            auto_half(updates, note, palette),
            check_half(updates, note, palette),
        ]
        .height(ty.row(40.0))
        .align_y(Center),
    )
    .padding(1)
    .style(move |_| container::Style {
        background: Some(palette.contrast_background.into()),
        border: Border {
            color: palette.border,
            width: 1.0,
            radius: 24.0.into(),
        },
        ..Default::default()
    });

    let mut content = column![pill].spacing(8).width(Fill);
    if let Some(error) = updates.error {
        content = content.push(ty.caption(error).color(palette.error));
    }
    content.into()
}

fn auto_half(updates: &Updates, note: Option<&str>, palette: Palette) -> Element<'static, Message> {
    let ty = palette.typography;
    let ready = updates.is_ready();
    let checked = updates.automatic_checks;
    let toggle_action = ready.then_some(Message::Updates(Action::SetAutomatic(!checked)));
    let toggle_cell = container(
        row![
            toggle_switch(checked, palette).on_toggle_maybe(
                ready.then_some(|next| Message::Updates(Action::SetAutomatic(next))),
            ),
            ty.control("Check automatically")
                .color(if ready { palette.text } else { palette.muted })
                .wrapping(Wrapping::None),
        ]
        .spacing(10)
        .align_y(Center),
    )
    .padding([0, 14])
    .width(Fill)
    .height(Fill)
    .align_y(Center)
    .clip(true);
    let body: Element<'static, Message> = if let Some(action) = toggle_action.clone() {
        mouse_area(toggle_cell)
            .on_press(action)
            .interaction(mouse::Interaction::Pointer)
            .into()
    } else {
        toggle_cell.into()
    };
    let control = focus_control::themed_optional(
        "app-settings-updates-automatic",
        toggle_action,
        body,
        palette,
    );
    container(unavailable_tooltip(
        control,
        (!ready).then_some(note).flatten(),
        palette,
    ))
    .width(Fill)
    .height(Fill)
    .into()
}

fn check_half(
    updates: &Updates,
    note: Option<&str>,
    palette: Palette,
) -> Element<'static, Message> {
    let ty = palette.typography;
    let check_action = updates
        .can_check
        .then_some(Message::Updates(Action::CheckNow));
    let face = button(
        container(ty.control("Check for updates").wrapping(Wrapping::None))
            .center_x(Fill)
            .center_y(Fill)
            .clip(true),
    )
    .padding([0, 12])
    .width(Fill)
    .height(Fill)
    .on_press_maybe(check_action.clone())
    .style(move |_, status| {
        let mut style =
            crate::dialogs::app_settings::style::button_style(palette, status, false, false);
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        style.background = Some(
            if hovered && status != button::Status::Disabled {
                palette.row_selected
            } else {
                palette.row_hover
            }
            .into(),
        );
        style.border.radius = iced::border::right(24.0);
        style.border.width = 0.0;
        style
    });
    let control =
        focus_control::themed_optional("app-settings-updates-check", check_action, face, palette);
    container(unavailable_tooltip(
        control,
        (!updates.can_check).then_some(note).flatten(),
        palette,
    ))
    .width(Fill)
    .height(Fill)
    .into()
}

fn unavailable_tooltip(
    control: Element<'static, Message>,
    note: Option<&str>,
    palette: Palette,
) -> Element<'static, Message> {
    let Some(note) = note else {
        return control;
    };
    let ty = palette.typography;
    tooltip(control, ty.small(note.to_owned()), tooltip::Position::Top)
        .delay(Duration::from_millis(250))
        .gap(5)
        .padding(7)
        .style(move |_| palette.tooltip())
        .into()
}
