//! Scrollable recovery guidance and explicit failed-close choices.

use iced::advanced::widget::{
    Id, Operation,
    operation::{Focusable, Outcome, Scrollable},
};
use iced::widget::{button, column, container, row};
use iced::{Element, Fill, Rectangle, Task, Vector};

use crate::application::contracts::storage::StartupOperation;
use crate::application::{Action, Application};

use crate::{Message, widgets::focus_control, window::appearance as a};

pub(crate) const RETRY_SAVE_ID: &str = "storage-retry-save";
pub(crate) const RETRY_STARTUP_ID: &str = "storage-retry-startup";
pub(crate) const RESTORE_ID: &str = "storage-restore-backup";
pub(crate) const CANCEL_CLOSE_ID: &str = "storage-cancel-close";
pub(crate) const DISCARD_ID: &str = "storage-close-without-saving";
pub(crate) const DISMISS_ID: &str = "storage-dismiss";
pub(crate) const CONFIRM_ID: &str = "storage-confirm-recovery";
pub(crate) const CANCEL_CONFIRM_ID: &str = "storage-cancel-confirmation";
pub(crate) const SCROLL_ID: &str = "storage-details-scroll";

pub(crate) fn initial_focus(
    application: &Application,
    confirmation: Option<StartupOperation>,
) -> &'static str {
    if confirmation.is_some() {
        CANCEL_CONFIRM_ID
    } else if application.close_failed() {
        CANCEL_CLOSE_ID
    } else if application.save_failed() {
        RETRY_SAVE_ID
    } else if application.can_retry_startup() {
        RETRY_STARTUP_ID
    } else {
        DISMISS_ID
    }
}

pub(crate) fn focus_initial(
    application: &Application,
    confirmation: Option<StartupOperation>,
) -> Task<Message> {
    iced::widget::operation::focus(initial_focus(application, confirmation))
        .chain(iced::advanced::widget::operate(reveal_focus()))
}

pub(crate) fn move_focus(backward: bool) -> Task<Message> {
    let focus = if backward {
        iced::widget::operation::focus_previous()
    } else {
        iced::widget::operation::focus_next()
    };
    focus.chain(iced::advanced::widget::operate(reveal_focus()))
}

/// Reveal keyboard targets even when recovery details exceed a minimum window.
pub(crate) fn reveal_focus<T: 'static>() -> impl Operation<T> {
    #[derive(Default)]
    struct Reveal {
        focused: Option<Rectangle>,
        viewport: Option<(Rectangle, Rectangle, Vector)>,
    }
    impl<T: 'static> Operation<T> for Reveal {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<T>)) {
            operate(self);
        }

        fn focusable(&mut self, _: Option<&Id>, bounds: Rectangle, state: &mut dyn Focusable) {
            if state.is_focused() {
                self.focused = Some(bounds.expand(3.0));
            }
        }

        fn scrollable(
            &mut self,
            id: Option<&Id>,
            bounds: Rectangle,
            content: Rectangle,
            translation: Vector,
            _: &mut dyn Scrollable,
        ) {
            if id == Some(&Id::new(SCROLL_ID)) {
                self.viewport = Some((bounds, content, translation));
            }
        }

        fn finish(&self) -> Outcome<T> {
            let (Some(focused), Some((bounds, content, translation))) =
                (self.focused, self.viewport)
            else {
                return Outcome::None;
            };
            let top = focused.y - translation.y;
            let bottom = top + focused.height;
            let adjustment = if top < bounds.y {
                top - bounds.y
            } else if bottom > bounds.y + bounds.height {
                bottom - bounds.y - bounds.height
            } else {
                return Outcome::None;
            };
            let offset =
                (translation.y + adjustment).clamp(0.0, (content.height - bounds.height).max(0.0));
            Outcome::Chain(Box::new(
                iced::advanced::widget::operation::scrollable::scroll_to(
                    SCROLL_ID.into(),
                    iced::advanced::widget::operation::scrollable::AbsoluteOffset {
                        x: None,
                        y: Some(offset),
                    },
                ),
            ))
        }
    }
    Reveal::default()
}

fn control(
    id: &'static str,
    label: &'static str,
    action: Message,
    palette: a::Palette,
) -> Element<'static, Message> {
    let ty = palette.typography;
    focus_control::themed(
        id,
        action.clone(),
        button(ty.control(label))
            .padding([7, 12])
            .style(move |_, status| palette.inspector_button(status, false))
            .on_press(action),
        palette,
    )
}

pub(crate) fn view(
    application: &Application,
    confirmation: Option<StartupOperation>,
    logs: &crate::dialogs::diagnostics::Logs,
    palette: a::Palette,
) -> Element<'static, Message> {
    let content = if let Some(operation) = confirmation {
        confirmation_view(application, operation, palette)
    } else {
        details_view(application, palette).push(crate::dialogs::diagnostics::view(logs, palette))
    };
    container(
        container(
            a::scroll_area(content)
                .id(SCROLL_ID)
                .height(Fill)
                .direction(iced::widget::scrollable::Direction::Vertical(a::scrollbar()))
                .style(palette.scrollable()),
        )
        .padding(20)
        .max_width(460)
        .height(Fill)
        .style(move |_| palette.modal_panel()),
    )
    .padding(36)
    .center_x(Fill)
    .center_y(Fill)
    .style(|_| container::Style {
        background: Some(a::MODAL_SCRIM.into()),
        ..container::Style::default()
    })
    .into()
}

fn details_view(
    application: &Application,
    palette: a::Palette,
) -> iced::widget::Column<'static, Message> {
    let ty = palette.typography;
    let management_close_failed = application.workspace_management_close_waiting()
        && application.workspace_management_error().is_some();
    let mut content = column![
        ty.heading(if management_close_failed {
            "Workspace transition did not finish before close"
        } else if application.close_failed() {
            "Workspace could not be saved before close"
        } else {
            "Workspace storage"
        })
        .color(palette.text),
        ty.text(application.storage_detail().unwrap_or_default())
            .color(palette.text),
    ]
    .spacing(12);
    if application.close_failed() {
        content = content.push(
            ty.text("Closing without saving discards changes that have not been saved.")
                .color(palette.text),
        );
    }
    content.push(details_actions(application, palette))
}

fn details_actions(application: &Application, palette: a::Palette) -> Element<'static, Message> {
    let mut actions = row![if application.close_failed() {
        control(
            CANCEL_CLOSE_ID,
            "Cancel close",
            Message::Action(Action::CancelClose),
            palette,
        )
    } else {
        control(
            DISMISS_ID,
            "Dismiss",
            Message::StorageDetails(false),
            palette,
        )
    }]
    .spacing(8)
    .width(Fill);
    if application.close_failed() {
        actions = actions.push(control(
            DISCARD_ID,
            "Close without saving",
            Message::Action(Action::CloseWithoutSaving),
            palette,
        ));
    }
    let management_close_failed = application.workspace_management_close_waiting()
        && application.workspace_management_error().is_some();
    if application.save_failed() || management_close_failed {
        actions = actions.push(control(
            RETRY_SAVE_ID,
            if management_close_failed {
                "Retry close"
            } else {
                "Retry saving"
            },
            Message::Action(Action::RetrySave),
            palette,
        ));
    }
    if application.can_retry_startup() {
        if let Some(offer) = application.backup_offer() {
            actions = actions.push(control(
                RESTORE_ID,
                "Restore backup…",
                Message::PrepareStartup(StartupOperation::RestoreBackup(offer.token)),
                palette,
            ));
        }
        actions = actions.push(control(
            RETRY_STARTUP_ID,
            "Retry startup",
            Message::PrepareStartup(StartupOperation::Retry),
            palette,
        ));
    }
    actions.wrap().align_x(iced::Right).into()
}

fn confirmation_view(
    application: &Application,
    operation: StartupOperation,
    palette: a::Palette,
) -> iced::widget::Column<'static, Message> {
    let ty = palette.typography;
    let mut content =
        column![ty.heading("Confirm workspace recovery").color(palette.text)].spacing(12);
    let available = match operation {
        StartupOperation::Retry => {
            content = content.push(
                ty.text("Reload saved configuration? Unsaved changes to the viewing copy will be discarded.")
                    .color(palette.text),
            );
            true
        }
        StartupOperation::RestoreBackup(token) => {
            if let Some(offer) = application
                .backup_offer()
                .filter(|offer| offer.token == token)
            {
                content = content.push(
                    ty.text(format!("Restore these documents from validated backups?\n\n{}\n\nPrevious files will be preserved beside the configuration. Backups will not be changed. Unsaved viewing changes will be discarded.", offer.documents.join("\n")))
                        .color(palette.text),
                );
                true
            } else {
                content = content.push(
                    ty.text("This backup offer is no longer available. Cancel and retry startup.")
                        .color(palette.text),
                );
                false
            }
        }
    };
    let mut actions = row![control(
        CANCEL_CONFIRM_ID,
        "Cancel",
        Message::CancelStartupConfirmation,
        palette,
    )]
    .spacing(8)
    .width(Fill);
    if available {
        actions = actions.push(control(
            CONFIRM_ID,
            match operation {
                StartupOperation::Retry => "Reload saved state",
                StartupOperation::RestoreBackup(_) => "Restore backup",
            },
            Message::ConfirmStartup,
            palette,
        ));
    }
    content.push(actions.wrap().align_x(iced::Right))
}
