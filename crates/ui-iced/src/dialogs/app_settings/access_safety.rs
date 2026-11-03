//! Folder access table, macOS TCC permission actions, and write-safety controls.

use std::collections::BTreeMap;
use std::path::Path;

use iced::widget::{button, column, container, mouse_area, row, rule};
use iced::{Alignment, Center, Element, Fill, FillPortion, mouse};

use crate::application::{Application, LoadState};
use crate::domain::{ProtectedAccess, SafetyPreferences};
#[cfg(target_os = "macos")]
use crate::platform::permissions::PermissionStatus;

use super::{layout, style, view::setting};
use crate::{
    Message,
    widgets::{focus_control, toggle_switch::toggle_switch},
    window::appearance as a,
};

const ACTION_BUTTON_WIDTH: f32 = 104.0;

#[cfg(not(target_os = "macos"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionStatus {
    Granted,
    Denied,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AccessSafetyEdit {
    ToggleAccess(ProtectedAccess, bool),
    RequestAccess(ProtectedAccess),
    OpenSystemSettings(ProtectedAccess),
    SetSafety(SafetyPreferences),
}

#[derive(Debug, Clone, Default)]
pub(super) struct AccessSafetyDraft {
    statuses: BTreeMap<ProtectedAccess, PermissionStatus>,
    notice: Option<String>,
}

impl AccessSafetyDraft {
    pub(super) fn new(application: &Application, probe_os: bool) -> Self {
        let home = application.home();
        let safety = application.safety();
        let mut draft = Self::default();
        if probe_os {
            draft.refresh_full_disk(home);
        }
        for target in [
            ProtectedAccess::Desktop,
            ProtectedAccess::Documents,
            ProtectedAccess::Downloads,
        ] {
            if let Some(folder_name) = target.home_folder_name() {
                let folder = home.join(folder_name);
                if has_loaded_subtree(application, &folder) {
                    draft.statuses.insert(target, PermissionStatus::Granted);
                } else if probe_os && safety.access_enabled(target) {
                    #[cfg(target_os = "macos")]
                    {
                        let status = crate::platform::permissions::check_folder_access(
                            target.home_folder_name(),
                            home,
                        );
                        draft.statuses.insert(target, status);
                    }
                }
            }
        }
        draft
    }

    pub(super) fn status(&self, target: ProtectedAccess) -> Option<PermissionStatus> {
        self.statuses.get(&target).copied()
    }

    pub(super) fn apply(
        &mut self,
        edit: AccessSafetyEdit,
        safety: &mut SafetyPreferences,
        home: &Path,
        probe_os: bool,
    ) {
        self.notice = None;
        match edit {
            AccessSafetyEdit::ToggleAccess(target, enabled) => {
                *safety = safety.with_access(target, enabled);
                if enabled && probe_os {
                    self.probe_target(target, home);
                }
            }
            AccessSafetyEdit::RequestAccess(target) => {
                if !safety.access_enabled(target) {
                    *safety = safety.with_access(target, true);
                }
                if probe_os {
                    self.request_or_open_settings(target, home);
                }
            }
            AccessSafetyEdit::OpenSystemSettings(target) => {
                if probe_os {
                    self.open_settings(target, home);
                }
            }
            AccessSafetyEdit::SetSafety(next) => {
                *safety = next;
            }
        }
    }

    fn refresh_full_disk(&mut self, home: &Path) {
        #[cfg(target_os = "macos")]
        {
            let status = crate::platform::permissions::check_full_disk_access(home);
            self.statuses.insert(ProtectedAccess::FullDisk, status);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = home;
    }

    fn probe_target(&mut self, target: ProtectedAccess, home: &Path) {
        #[cfg(target_os = "macos")]
        {
            let status =
                crate::platform::permissions::check_folder_access(target.home_folder_name(), home);
            self.statuses.insert(target, status);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (target, home);
    }

    fn request_or_open_settings(&mut self, target: ProtectedAccess, home: &Path) {
        #[cfg(target_os = "macos")]
        {
            if target == ProtectedAccess::FullDisk {
                self.open_settings(target, home);
                return;
            }
            let status = crate::platform::permissions::request_folder_access(
                target.home_folder_name(),
                home,
            );
            self.statuses.insert(target, status);
            if status == PermissionStatus::Denied
                && let Err(error) = crate::platform::permissions::open_privacy_settings(
                    target == ProtectedAccess::FullDisk,
                )
            {
                self.notice = Some(error);
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (target, home);
    }

    fn open_settings(&mut self, target: ProtectedAccess, home: &Path) {
        #[cfg(target_os = "macos")]
        {
            if let Err(error) = crate::platform::permissions::open_privacy_settings(
                target == ProtectedAccess::FullDisk,
            ) {
                self.notice = Some(error);
            }
            if target == ProtectedAccess::FullDisk {
                self.refresh_full_disk(home);
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (target, home);
    }
}

fn has_loaded_subtree(application: &Application, folder: &Path) -> bool {
    application.workspace().active_views().any(|view| {
        matches!(
            application.load(view.view),
            Some(LoadState::Ready(listing)) if listing.directory.starts_with(folder)
        )
    })
}

struct AccessRowSpec {
    target: ProtectedAccess,
    label: &'static str,
    toggle_id: &'static str,
    button_id: &'static str,
}

pub(super) fn view<'a>(
    safety: SafetyPreferences,
    access: &'a AccessSafetyDraft,
    width: f32,
    palette: a::Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    let header = column![
        table_header(width, palette),
        rule::horizontal(1).style(move |_| palette.rule()),
    ]
    .spacing(10);

    let mut section = column![ty.heading("Access and Safety").color(palette.text), header]
        .extend(folder_access_rows(safety, access, width, palette))
        .push(rule::horizontal(1).style(move |_| palette.rule()))
        .extend(write_protection_rows(safety, width, palette))
        .spacing(24);
    if let Some(notice) = &access.notice {
        section = section.push(ty.caption(notice.clone()).color(palette.error));
    }
    section.into()
}

fn table_header(width: f32, palette: a::Palette) -> Element<'static, Message> {
    let (left, right) = layout::column_widths(width);
    let ty = palette.typography;
    let right_cols = row![
        container(ty.text("Access").color(palette.muted)).width(FillPortion(1)),
        container(ty.text("macOS").color(palette.muted)).width(FillPortion(1)),
        container(ty.text("Action").color(palette.muted))
            .width(FillPortion(1))
            .align_x(Alignment::End),
    ]
    .spacing(16)
    .align_y(Center);

    row![
        container(ty.text("Location").color(palette.muted)).width(left),
        container(right_cols).width(right),
    ]
    .spacing(36)
    .align_y(Center)
    .wrap()
    .vertical_spacing(8)
    .into()
}

fn folder_access_rows<'a>(
    safety: SafetyPreferences,
    access: &'a AccessSafetyDraft,
    width: f32,
    palette: a::Palette,
) -> [Element<'a, Message>; 4] {
    [
        access_row(
            AccessRowSpec {
                target: ProtectedAccess::Desktop,
                label: "Desktop",
                toggle_id: "app-settings-access-desktop",
                button_id: "app-settings-request-desktop",
            },
            safety,
            access,
            width,
            palette,
        ),
        access_row(
            AccessRowSpec {
                target: ProtectedAccess::Documents,
                label: "Documents",
                toggle_id: "app-settings-access-documents",
                button_id: "app-settings-request-documents",
            },
            safety,
            access,
            width,
            palette,
        ),
        access_row(
            AccessRowSpec {
                target: ProtectedAccess::Downloads,
                label: "Downloads",
                toggle_id: "app-settings-access-downloads",
                button_id: "app-settings-request-downloads",
            },
            safety,
            access,
            width,
            palette,
        ),
        access_row(
            AccessRowSpec {
                target: ProtectedAccess::FullDisk,
                label: "Full Disk Access",
                toggle_id: "app-settings-access-full-disk",
                button_id: "app-settings-request-full-disk",
            },
            safety,
            access,
            width,
            palette,
        ),
    ]
}

fn access_row<'a>(
    spec: AccessRowSpec,
    safety: SafetyPreferences,
    access: &'a AccessSafetyDraft,
    width: f32,
    palette: a::Palette,
) -> Element<'a, Message> {
    let (left, right) = layout::column_widths(width);
    let target = spec.target;
    let enabled = safety.access_enabled(target);
    let toggle_msg =
        Message::AppAccessSafetyEdited(AccessSafetyEdit::ToggleAccess(target, !enabled));
    let toggle = focus_control::themed(
        spec.toggle_id,
        toggle_msg.clone(),
        mouse_area(toggle_switch(enabled, palette).on_toggle(move |next| {
            Message::AppAccessSafetyEdited(AccessSafetyEdit::ToggleAccess(target, next))
        }))
        .on_press(toggle_msg)
        .interaction(mouse::Interaction::Pointer),
        palette,
    );
    let status = access.status(target);
    let granted = status == Some(PermissionStatus::Granted);
    let status_label = if granted { "Granted" } else { "Not granted" };
    let status_color = if granted { palette.text } else { palette.muted };
    let (btn_label, btn_msg) = if granted {
        (
            "Settings",
            Message::AppAccessSafetyEdited(AccessSafetyEdit::OpenSystemSettings(target)),
        )
    } else {
        (
            "Request",
            Message::AppAccessSafetyEdited(AccessSafetyEdit::RequestAccess(target)),
        )
    };
    let action_btn = focus_control::themed(
        spec.button_id,
        btn_msg.clone(),
        button(container(palette.typography.control(btn_label)).center_x(Fill))
            .padding([8, 16])
            .width(ACTION_BUTTON_WIDTH)
            .style(move |_, st| style::button_style(palette, st, false, false))
            .on_press(btn_msg),
        palette,
    );
    let right_cols = row![
        container(toggle).width(FillPortion(1)).align_y(Center),
        container(palette.typography.control(status_label).color(status_color))
            .width(FillPortion(1))
            .align_y(Center),
        container(action_btn)
            .width(FillPortion(1))
            .align_x(Alignment::End)
            .align_y(Center),
    ]
    .spacing(16)
    .align_y(Center);

    row![
        container(palette.typography.control(spec.label).color(palette.text))
            .width(left)
            .align_y(Center),
        container(right_cols).width(right).align_y(Center),
    ]
    .spacing(36)
    .align_y(Center)
    .wrap()
    .vertical_spacing(12)
    .into()
}

fn write_protection_rows(
    safety: SafetyPreferences,
    width: f32,
    palette: a::Palette,
) -> [Element<'static, Message>; 3] {
    [
        write_safety_row(
            (
                "System folders",
                "macOS system paths, Library, and top-level user folders.",
            ),
            ("app-settings-safety-system", safety.system_folders),
            move |system_folders| SafetyPreferences {
                system_folders,
                ..safety
            },
            width,
            palette,
        ),
        write_safety_row(
            ("Applications", "/Applications and ~/Applications."),
            ("app-settings-safety-applications", safety.applications),
            move |applications| SafetyPreferences {
                applications,
                ..safety
            },
            width,
            palette,
        ),
        write_safety_row(
            (
                "Credentials and keys",
                "~/.ssh, ~/.gnupg, ~/.aws, ~/.kube, ~/.docker, ~/.config, and credential files.",
            ),
            ("app-settings-safety-credentials", safety.credentials),
            move |credentials| SafetyPreferences {
                credentials,
                ..safety
            },
            width,
            palette,
        ),
    ]
}

fn write_safety_row(
    copy: (&'static str, &'static str),
    toggle: (&'static str, bool),
    update: impl Fn(bool) -> SafetyPreferences + Copy + 'static,
    width: f32,
    palette: a::Palette,
) -> Element<'static, Message> {
    let (label, description) = copy;
    let (id, value) = toggle;
    let next_message = Message::AppAccessSafetyEdited(AccessSafetyEdit::SetSafety(update(!value)));
    let control = focus_control::themed(
        id,
        next_message.clone(),
        mouse_area(toggle_switch(value, palette).on_toggle(move |next| {
            Message::AppAccessSafetyEdited(AccessSafetyEdit::SetSafety(update(next)))
        }))
        .on_press(next_message)
        .interaction(mouse::Interaction::Pointer),
        palette,
    );
    setting(label, description, control, width, palette)
}
