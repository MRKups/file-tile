//! A compact support-folder pill, not an embedded log viewer or Settings draft.
use crate::diagnostics::Status;
use crate::{Desktop, Message, widgets::folder_field, window::appearance::Palette};
use iced::{
    Center, Element, Fill, Task,
    widget::{column, container, text::Wrapping, tooltip},
};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub(crate) enum Action {
    Open,
    Opened(bool),
}

#[derive(Default)]
pub(crate) struct Logs {
    path: Option<PathBuf>,
    status: Option<Status>,
    pending: Option<iced::task::Handle>,
    error: Option<&'static str>,
}
impl Drop for Logs {
    fn drop(&mut self) {
        if let Some(handle) = self.pending.take() {
            handle.abort();
        }
    }
}
impl Desktop {
    pub(crate) fn diagnostic_logs_action(&mut self, action: Action) -> Task<Message> {
        match action {
            Action::Open => self.open_diagnostic_logs(),
            Action::Opened(success) => self.diagnostic_logs_opened(success),
        }
    }
    pub(crate) fn sync_diagnostics(&mut self) {
        self.logs.path = self.storage_root.as_ref().map(|root| root.join("logs"));
        self.logs.status = Some(crate::diagnostics::session().status());
    }

    pub(crate) fn open_diagnostic_logs(&mut self) -> Task<Message> {
        if self.logs.pending.is_some()
            || !(self.app_settings.is_some() || self.storage_details)
            || self.startup_confirmation.is_some()
        {
            return Task::none();
        }
        let (Some(path), Some(workers)) = (self.logs.path.clone(), &self.open_workers) else {
            self.logs.error = Some(
                "Cannot open logs folder: the opening worker is unavailable. Open the displayed path in Finder.",
            );
            return Task::none();
        };
        self.logs.error = None;
        let (task, handle) = Task::perform(workers.open_logs_folder(path), |result| {
            Message::DiagnosticLogs(Action::Opened(result.is_ok()))
        })
        .abortable();
        self.logs.pending = Some(handle);
        task
    }
    pub(crate) fn diagnostic_logs_opened(&mut self, success: bool) -> Task<Message> {
        self.logs.pending = None;
        self.logs.error = (!success).then_some(
            "Cannot open logs folder: macOS could not open the folder. Check the displayed path in Finder, then retry.",
        );
        Task::none()
    }
}

/// The settings row supplies the label; recovery uses the same controls below it.
pub(crate) fn controls(logs: &Logs, palette: Palette) -> Element<'static, Message> {
    let ty = palette.typography;
    let available = logs.path.is_some();
    let path = logs.path.as_ref().map_or_else(
        || "Log folder unavailable".into(),
        |path| path.to_string_lossy().into_owned(),
    );
    let action =
        (available && logs.pending.is_none()).then_some(Message::DiagnosticLogs(Action::Open));
    let label = if logs.pending.is_some() {
        "Opening…"
    } else {
        "Open"
    };
    let path_cell: Element<'static, Message> = container(
        ty.control(path.clone())
            .color(if available {
                palette.text
            } else {
                palette.muted
            })
            .wrapping(Wrapping::None),
    )
    .padding([0, 14])
    .width(Fill)
    .height(Fill)
    .align_y(Center)
    .clip(true)
    .into();
    let path_field: Element<'static, Message> = if available {
        tooltip(path_cell, ty.small(path), tooltip::Position::Top)
            .delay(Duration::from_millis(350))
            .gap(5)
            .padding(7)
            .style(move |_| palette.tooltip())
            .into()
    } else {
        path_cell
    };
    let pill =
        folder_field::view_with_label(path_field, label, "diagnostic-logs-open", action, palette);
    let mut content = column![pill].spacing(8).width(Fill);
    if matches!(logs.status, Some(Status::Failed)) {
        content = content.push(
            ty.caption("Cannot write diagnostic logs. Check this location and available disk space, then relaunch. Existing logs are preserved.")
                .color(palette.error),
        );
    }
    if let Some(error) = logs.error {
        content = content.push(ty.caption(error).color(palette.error));
    }
    content.into()
}
pub(crate) fn view(logs: &Logs, palette: Palette) -> Element<'static, Message> {
    column![
        palette.typography.control("Diagnostic Logs"),
        controls(logs, palette)
    ]
    .spacing(8)
    .into()
}
