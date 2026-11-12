//! Folder access table, macOS TCC permission actions, and write-safety controls.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use iced::widget::{button, column, container, mouse_area, row, rule};
use iced::{Alignment, Center, Element, Fill, FillPortion, mouse};

use crate::application::{Application, LoadState};
use crate::domain::{ProtectedAccess, SafetyPreferences};
use crate::platform::permissions::PermissionStatus;
use crate::platform::volumes::{DetectedLocation, LocationKind};

use super::{layout, style, view::setting};
use crate::{
    Message,
    boxes::sidebar::folder_display_name,
    widgets::{focus_control, toggle_switch::toggle_switch},
    window::appearance as a,
};

const ACTION_BUTTON_WIDTH: f32 = 104.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteSafetyTarget {
    SystemFolders,
    Applications,
    Credentials,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AccessSafetyEdit {
    ToggleAccess(ProtectedAccess, bool),
    RequestAccess(ProtectedAccess),
    OpenSystemSettings(ProtectedAccess),
    ToggleCloudLocation(PathBuf, bool),
    RequestCloudLocation(PathBuf),
    OpenCloudSettings,
    ToggleWriteSafety(WriteSafetyTarget, bool),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CloudAccessEntry {
    label: String,
    path: PathBuf,
    status: PermissionStatus,
}

#[derive(Debug, Clone, Default)]
pub(super) struct AccessSafetyDraft {
    statuses: BTreeMap<ProtectedAccess, PermissionStatus>,
    cloud_entries: Vec<CloudAccessEntry>,
    external_drives: Vec<(PathBuf, PermissionStatus)>,
    network_shares: Vec<(PathBuf, PermissionStatus)>,
    notice: Option<String>,
}

impl AccessSafetyDraft {
    pub(super) fn new(application: &Application, probe_os: bool) -> Self {
        let safety = application.safety();
        let mut draft = Self::default();
        draft.reprobe_on_focus(application, &safety, probe_os);
        draft
    }

    pub(super) fn reprobe_on_focus(
        &mut self,
        application: &Application,
        safety: &SafetyPreferences,
        probe_os: bool,
    ) {
        if probe_os {
            self.refresh_full_disk(application.home());
        }
        self.sync_locations(application, safety, probe_os);
    }

    fn refresh_home_targets(&mut self, application: &Application, probe_os: bool) {
        let home = application.home();
        for target in [
            ProtectedAccess::Desktop,
            ProtectedAccess::Documents,
            ProtectedAccess::Downloads,
        ] {
            if let Some(folder_name) = target.home_folder_name() {
                let folder = home.join(folder_name);
                if has_loaded_subtree(application, &folder) {
                    self.statuses.insert(target, PermissionStatus::Granted);
                } else if probe_os {
                    let status = crate::platform::permissions::check_folder_access(
                        target.home_folder_name(),
                        home,
                    );
                    self.statuses.insert(target, status);
                } else if application.ungranted_pins().contains(&folder) {
                    self.statuses.insert(target, PermissionStatus::Denied);
                }
            }
        }
    }

    pub(super) fn sync_locations(
        &mut self,
        application: &Application,
        safety: &SafetyPreferences,
        probe_os: bool,
    ) {
        self.refresh_home_targets(application, probe_os);
        self.cloud_entries.clear();
        self.external_drives.clear();
        self.network_shares.clear();
        let mut seen_paths = BTreeSet::new();
        for loc in application.locations() {
            seen_paths.insert(loc.path.clone());
            let status = resolve_location_status(application, loc, safety, probe_os);
            match loc.kind {
                LocationKind::Cloud(_) => self.cloud_entries.push(CloudAccessEntry {
                    label: loc.label.clone(),
                    path: loc.path.clone(),
                    status,
                }),
                LocationKind::ExternalDrive => {
                    self.external_drives.push((loc.path.clone(), status));
                }
                LocationKind::NetworkDrive { .. } => {
                    self.network_shares.push((loc.path.clone(), status));
                }
            }
        }
        for disabled in &safety.disabled_locations {
            if seen_paths.insert(disabled.clone()) {
                let status = if probe_os {
                    crate::platform::permissions::check_path_access(disabled)
                } else {
                    PermissionStatus::Unavailable
                };
                self.cloud_entries.push(CloudAccessEntry {
                    label: folder_display_name(disabled, application.home()),
                    path: disabled.clone(),
                    status,
                });
            }
        }
    }

    pub(super) fn status(&self, target: ProtectedAccess) -> Option<PermissionStatus> {
        match target {
            ProtectedAccess::ExternalDrives => Some(aggregate_volume_status(&self.external_drives)),
            ProtectedAccess::NetworkShares => Some(aggregate_volume_status(&self.network_shares)),
            _ => self.statuses.get(&target).copied(),
        }
    }

    pub(super) fn apply(
        &mut self,
        edit: AccessSafetyEdit,
        safety: &mut SafetyPreferences,
        home: &Path,
        probe_os: bool,
    ) -> bool {
        self.notice = None;
        match edit {
            AccessSafetyEdit::ToggleAccess(target, enabled) => {
                *safety = safety.clone().with_access(target, enabled);
                if enabled && probe_os {
                    self.probe_target(target, home);
                }
                false
            }
            AccessSafetyEdit::RequestAccess(target) => {
                if !safety.access_enabled(target) {
                    *safety = safety.clone().with_access(target, true);
                }
                if probe_os {
                    self.request_or_open_settings(target, home);
                    true
                } else {
                    false
                }
            }
            AccessSafetyEdit::OpenSystemSettings(target) => {
                if probe_os {
                    self.open_settings(target == ProtectedAccess::FullDisk);
                }
                false
            }
            AccessSafetyEdit::ToggleCloudLocation(path, enabled) => {
                if !enabled
                    && !safety.disabled_locations.contains(&path)
                    && safety.disabled_locations.len() >= crate::domain::MAX_DISABLED_LOCATIONS
                {
                    self.notice = Some(
                        "Cannot turn off more than 128 cloud locations. Re-enable an unused location first."
                            .into(),
                    );
                    return false;
                }
                *safety = safety.clone().with_location_access(path.clone(), enabled);
                if enabled && probe_os {
                    self.probe_cloud_entry(&path);
                }
                false
            }
            AccessSafetyEdit::RequestCloudLocation(path) => {
                if !safety.is_location_enabled(&path) {
                    *safety = safety.clone().with_location_access(path.clone(), true);
                }
                if probe_os {
                    self.request_cloud_entry(&path);
                    true
                } else {
                    false
                }
            }
            AccessSafetyEdit::OpenCloudSettings => {
                if probe_os {
                    self.open_settings(false);
                }
                false
            }
            AccessSafetyEdit::ToggleWriteSafety(target, enabled) => {
                match target {
                    WriteSafetyTarget::SystemFolders => safety.system_folders = enabled,
                    WriteSafetyTarget::Applications => safety.applications = enabled,
                    WriteSafetyTarget::Credentials => safety.credentials = enabled,
                }
                false
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
        match target {
            ProtectedAccess::ExternalDrives => {
                for (path, status) in &mut self.external_drives {
                    *status = crate::platform::permissions::check_path_access(path);
                }
            }
            ProtectedAccess::NetworkShares => {
                for (path, status) in &mut self.network_shares {
                    *status = crate::platform::permissions::check_path_access(path);
                }
            }
            ProtectedAccess::FullDisk => self.refresh_full_disk(home),
            ProtectedAccess::Desktop | ProtectedAccess::Documents | ProtectedAccess::Downloads => {
                let status = crate::platform::permissions::check_folder_access(
                    target.home_folder_name(),
                    home,
                );
                self.statuses.insert(target, status);
            }
        }
    }

    fn probe_cloud_entry(&mut self, target_path: &Path) {
        if let Some(entry) = self
            .cloud_entries
            .iter_mut()
            .find(|entry| entry.path == target_path)
        {
            entry.status = crate::platform::permissions::check_path_access(target_path);
        }
    }

    fn request_or_open_settings(&mut self, target: ProtectedAccess, home: &Path) {
        match target {
            ProtectedAccess::FullDisk => self.open_settings(true),
            ProtectedAccess::ExternalDrives => {
                let denied = request_volume_batch(&mut self.external_drives);
                if denied {
                    self.open_settings(false);
                }
            }
            ProtectedAccess::NetworkShares => {
                let denied = request_volume_batch(&mut self.network_shares);
                if denied {
                    self.open_settings(false);
                }
            }
            ProtectedAccess::Desktop | ProtectedAccess::Documents | ProtectedAccess::Downloads => {
                let status = crate::platform::permissions::request_folder_access(
                    target.home_folder_name(),
                    home,
                );
                self.statuses.insert(target, status);
                if status == PermissionStatus::Denied {
                    self.open_settings(false);
                }
            }
        }
    }

    fn request_cloud_entry(&mut self, target_path: &Path) {
        let status = crate::platform::permissions::request_path_access(target_path);
        if let Some(entry) = self
            .cloud_entries
            .iter_mut()
            .find(|entry| entry.path == target_path)
        {
            entry.status = status;
        }
        if status == PermissionStatus::Denied {
            self.open_settings(false);
        }
    }

    fn open_settings(&mut self, full_disk: bool) {
        if let Err(error) = crate::platform::permissions::open_privacy_settings(full_disk) {
            self.notice = Some(error);
        }
    }
}

fn resolve_location_status(
    application: &Application,
    loc: &DetectedLocation,
    safety: &SafetyPreferences,
    probe_os: bool,
) -> PermissionStatus {
    if has_loaded_subtree(application, &loc.path) {
        return PermissionStatus::Granted;
    }
    let can_probe_now = match loc.kind {
        LocationKind::Cloud(_) | LocationKind::ExternalDrive => probe_os,
        LocationKind::NetworkDrive { .. } => probe_os && safety.network_shares_access,
    };
    if can_probe_now {
        crate::platform::permissions::check_path_access(&loc.path)
    } else if loc.accessible {
        PermissionStatus::Granted
    } else {
        PermissionStatus::Denied
    }
}

fn aggregate_volume_status(entries: &[(PathBuf, PermissionStatus)]) -> PermissionStatus {
    if entries.is_empty() {
        PermissionStatus::Unavailable
    } else if entries
        .iter()
        .all(|(_, status)| *status == PermissionStatus::Granted)
    {
        PermissionStatus::Granted
    } else {
        PermissionStatus::Denied
    }
}

fn request_volume_batch(entries: &mut [(PathBuf, PermissionStatus)]) -> bool {
    if entries.is_empty() {
        return true;
    }
    let mut any_denied = false;
    for (path, status) in entries {
        if *status != PermissionStatus::Granted {
            *status = crate::platform::permissions::request_path_access(path);
        }
        if *status == PermissionStatus::Denied {
            any_denied = true;
        }
    }
    any_denied
}

fn has_loaded_subtree(application: &Application, folder: &Path) -> bool {
    application.workspace().active_views().any(|view| {
        matches!(
            application.load(view.view),
            Some(LoadState::Ready(listing)) if listing.directory.starts_with(folder)
        )
    })
}

struct RowViewSpec {
    label: String,
    toggle_id: String,
    button_id: String,
    enabled: bool,
    on_toggle_true: Message,
    on_toggle_false: Message,
    status_label: &'static str,
    status_granted: bool,
    btn_label: &'static str,
    btn_msg: Message,
}

pub(super) fn view<'a>(
    safety: &SafetyPreferences,
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
    safety: &SafetyPreferences,
    access: &'a AccessSafetyDraft,
    width: f32,
    palette: a::Palette,
) -> Vec<Element<'a, Message>> {
    let mut rows = Vec::with_capacity(6 + access.cloud_entries.len());
    for (target, slug) in [
        (ProtectedAccess::Desktop, "desktop"),
        (ProtectedAccess::Documents, "documents"),
        (ProtectedAccess::Downloads, "downloads"),
    ] {
        rows.push(protected_target_row(
            target, slug, false, safety, access, width, palette,
        ));
    }
    for (index, entry) in access.cloud_entries.iter().enumerate() {
        rows.push(cloud_location_row(index, entry, safety, width, palette));
    }
    rows.push(protected_target_row(
        ProtectedAccess::ExternalDrives,
        "external-drives",
        true,
        safety,
        access,
        width,
        palette,
    ));
    rows.push(protected_target_row(
        ProtectedAccess::NetworkShares,
        "network-shares",
        true,
        safety,
        access,
        width,
        palette,
    ));
    rows.push(protected_target_row(
        ProtectedAccess::FullDisk,
        "full-disk",
        false,
        safety,
        access,
        width,
        palette,
    ));
    rows
}

fn protected_target_row<'a>(
    target: ProtectedAccess,
    slug: &'static str,
    volume_category: bool,
    safety: &SafetyPreferences,
    access: &'a AccessSafetyDraft,
    width: f32,
    palette: a::Palette,
) -> Element<'a, Message> {
    let enabled = safety.access_enabled(target);
    let status = access.status(target);
    let granted = status == Some(PermissionStatus::Granted);
    let unavailable = status == Some(PermissionStatus::Unavailable);
    let status_label = if granted {
        "Granted"
    } else if volume_category && unavailable {
        "Not connected"
    } else if unavailable {
        "Unavailable"
    } else {
        "Not granted"
    };
    let (btn_label, btn_msg) = if granted || unavailable {
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
    render_access_row(
        RowViewSpec {
            label: target.label().to_owned(),
            toggle_id: format!("app-settings-access-{slug}"),
            button_id: format!("app-settings-request-{slug}"),
            enabled,
            on_toggle_true: Message::AppAccessSafetyEdited(AccessSafetyEdit::ToggleAccess(
                target, true,
            )),
            on_toggle_false: Message::AppAccessSafetyEdited(AccessSafetyEdit::ToggleAccess(
                target, false,
            )),
            status_label,
            status_granted: granted,
            btn_label,
            btn_msg,
        },
        width,
        palette,
    )
}

fn cloud_location_row<'a>(
    index: usize,
    entry: &CloudAccessEntry,
    safety: &SafetyPreferences,
    width: f32,
    palette: a::Palette,
) -> Element<'a, Message> {
    let enabled = safety.is_location_enabled(&entry.path);
    let granted = entry.status == PermissionStatus::Granted;
    let unavailable = entry.status == PermissionStatus::Unavailable;
    let status_label = if granted {
        "Granted"
    } else if unavailable {
        "Unavailable"
    } else {
        "Not granted"
    };
    let (btn_label, btn_msg) = if granted || unavailable {
        (
            "Settings",
            Message::AppAccessSafetyEdited(AccessSafetyEdit::OpenCloudSettings),
        )
    } else {
        (
            "Request",
            Message::AppAccessSafetyEdited(AccessSafetyEdit::RequestCloudLocation(
                entry.path.clone(),
            )),
        )
    };
    render_access_row(
        RowViewSpec {
            label: entry.label.clone(),
            toggle_id: format!("app-settings-access-cloud-{index}"),
            button_id: format!("app-settings-request-cloud-{index}"),
            enabled,
            on_toggle_true: Message::AppAccessSafetyEdited(AccessSafetyEdit::ToggleCloudLocation(
                entry.path.clone(),
                true,
            )),
            on_toggle_false: Message::AppAccessSafetyEdited(AccessSafetyEdit::ToggleCloudLocation(
                entry.path.clone(),
                false,
            )),
            status_label,
            status_granted: granted,
            btn_label,
            btn_msg,
        },
        width,
        palette,
    )
}

fn render_access_row<'a>(
    spec: RowViewSpec,
    width: f32,
    palette: a::Palette,
) -> Element<'a, Message> {
    let (left, right) = layout::column_widths(width);
    let on_true = spec.on_toggle_true;
    let on_false = spec.on_toggle_false;
    let toggle_msg = if spec.enabled {
        on_false.clone()
    } else {
        on_true.clone()
    };
    let toggle = focus_control::themed(
        spec.toggle_id,
        toggle_msg.clone(),
        mouse_area(toggle_switch(spec.enabled, palette).on_toggle(move |next| {
            if next {
                on_true.clone()
            } else {
                on_false.clone()
            }
        }))
        .on_press(toggle_msg)
        .interaction(mouse::Interaction::Pointer),
        palette,
    );
    let status_color = if spec.status_granted {
        palette.text
    } else {
        palette.muted
    };
    let action_btn = focus_control::themed(
        spec.button_id,
        spec.btn_msg.clone(),
        button(container(palette.typography.control(spec.btn_label)).center_x(Fill))
            .padding([8, 16])
            .width(ACTION_BUTTON_WIDTH)
            .style(move |_, st| style::button_style(palette, st, false, false))
            .on_press(spec.btn_msg),
        palette,
    );
    let right_cols = row![
        container(toggle).width(FillPortion(1)).align_y(Center),
        container(
            palette
                .typography
                .control(spec.status_label)
                .color(status_color),
        )
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
    safety: &SafetyPreferences,
    width: f32,
    palette: a::Palette,
) -> [Element<'static, Message>; 3] {
    [
        write_safety_row(
            (
                "System folders",
                "macOS system paths, Library, cloud roots, and top-level user folders.",
            ),
            ("app-settings-safety-system", safety.system_folders),
            WriteSafetyTarget::SystemFolders,
            width,
            palette,
        ),
        write_safety_row(
            ("Applications", "/Applications and ~/Applications."),
            ("app-settings-safety-applications", safety.applications),
            WriteSafetyTarget::Applications,
            width,
            palette,
        ),
        write_safety_row(
            (
                "Credentials and keys",
                "~/.ssh, ~/.gnupg, ~/.aws, ~/.kube, ~/.docker, ~/.config, and credential files.",
            ),
            ("app-settings-safety-credentials", safety.credentials),
            WriteSafetyTarget::Credentials,
            width,
            palette,
        ),
    ]
}

fn write_safety_row(
    copy: (&'static str, &'static str),
    toggle: (&'static str, bool),
    target: WriteSafetyTarget,
    width: f32,
    palette: a::Palette,
) -> Element<'static, Message> {
    let (label, description) = copy;
    let (id, value) = toggle;
    let next_message =
        Message::AppAccessSafetyEdited(AccessSafetyEdit::ToggleWriteSafety(target, !value));
    let control = focus_control::themed(
        id,
        next_message.clone(),
        mouse_area(toggle_switch(value, palette).on_toggle(move |next| {
            Message::AppAccessSafetyEdited(AccessSafetyEdit::ToggleWriteSafety(target, next))
        }))
        .on_press(next_message)
        .interaction(mouse::Interaction::Pointer),
        palette,
    );
    setting(label, description, control, width, palette)
}
