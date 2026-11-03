//! Local management drafts. Storage transitions and failures belong to the application.
use crate::application::contracts::configuration::{WorkspaceId, WorkspaceIndex, WorkspaceSummary};
use crate::application::{Action, Application};
use crate::domain::ViewId;
use crate::{Desktop, Message, widgets::focus_control, window::appearance as a};
use iced::widget::{button, column, container, operation, row, space, text_input};
use iced::{Element, Event, Fill, Size, Task, keyboard};

const NAME_FIELD: &str = "workspace-create-name";
const RENAME_FIELD: &str = "workspace-rename-name";

struct RenameDraft {
    id: WorkspaceId,
    name: String,
    invalid: bool,
}

enum Subview {
    List,
    Remove(WorkspaceId),
}

pub(crate) struct Draft {
    name: String,
    invalid: bool,
    renaming: Option<RenameDraft>,
    subview: Subview,
    return_focus: Option<ViewId>,
}

impl Draft {
    pub(crate) fn clear_return_focus(&mut self) {
        self.return_focus = None;
    }
}

impl Desktop {
    pub(crate) fn workspace_management_message(&mut self, message: Message) -> Task<Message> {
        if !matches!(message, Message::OpenWorkspaceManagement)
            && (self.workspace_management.is_none()
                || self.storage_details && self.application.storage_detail().is_some()
                || self.native_menu.error().is_some())
        {
            return Task::none();
        }
        match message {
            Message::OpenWorkspaceManagement => {
                if self.modal_open() {
                    return Task::none();
                }
                let task = self.dispatch(Action::OpenWorkspaceManagement);
                if self.application.workspace_management_open() {
                    self.workspace_management = Some(Draft {
                        name: String::new(),
                        invalid: false,
                        renaming: None,
                        subview: Subview::List,
                        return_focus: self.focused_path.take(),
                    });
                    self.focus_version = self.focus_version.saturating_add(1);
                    self.pointer = None;
                    for presentation in self.views.values_mut() {
                        presentation.hovered = None;
                    }
                    return Task::batch([task, operation::focus(NAME_FIELD)]);
                }
                task
            }
            Message::ManagementNameChanged(name) => {
                if !self.application.workspace_management_pending()
                    && let Some(draft) = &mut self.workspace_management
                {
                    draft.name = crate::domain::validation::sanitize_config_name(&name);
                    draft.invalid = false;
                }
                Task::none()
            }
            Message::CreateWorkspace => {
                let Some(draft) = &mut self.workspace_management else {
                    return Task::none();
                };
                draft.name = crate::domain::validation::finalize_config_name(&draft.name);
                if !WorkspaceIndex::valid_name(&draft.name) {
                    draft.invalid = true;
                    return Task::none();
                }
                draft.renaming = None;
                let name = draft.name.clone();
                self.dispatch(Action::CreateWorkspace(name))
            }
            Message::ChooseWorkspace(id) => {
                if let Some(draft) = &mut self.workspace_management {
                    draft.renaming = None;
                }
                self.dispatch(Action::SelectWorkspace(id))
            }
            Message::PrepareWorkspaceRename(_)
            | Message::WorkspaceRenameChanged(_)
            | Message::ConfirmWorkspaceRename
            | Message::CancelWorkspaceRename => self.workspace_rename_message(message),
            Message::PrepareWorkspaceRemoval(_)
            | Message::ConfirmWorkspaceRemoval
            | Message::CancelWorkspaceRemoval => self.workspace_removal_message(message),
            Message::RetryWorkspaceManagement => self.dispatch(Action::RetryWorkspaceManagement),
            Message::CancelWorkspaceManagement => {
                let focus = self
                    .workspace_management
                    .as_ref()
                    .and_then(|draft| draft.return_focus);
                let task = self.dispatch(Action::CancelWorkspaceManagement);
                if self.application.workspace_management_open() {
                    task
                } else {
                    Task::batch([task, self.restore_workspace_focus(focus)])
                }
            }
            _ => Task::none(),
        }
    }

    fn workspace_removal_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PrepareWorkspaceRemoval(id) => {
                if !self.application.workspace_management_pending()
                    && let Some(draft) = &mut self.workspace_management
                {
                    draft.renaming = None;
                    draft.subview = Subview::Remove(id);
                    return operation::focus("workspace-remove-cancel");
                }
                Task::none()
            }
            Message::ConfirmWorkspaceRemoval => {
                let id = self.workspace_management.as_mut().and_then(|draft| {
                    if let Subview::Remove(id) = draft.subview {
                        draft.subview = Subview::List;
                        Some(id)
                    } else {
                        None
                    }
                });
                id.map_or_else(Task::none, |id| {
                    self.dispatch(Action::RemoveWorkspace {
                        id,
                        confirmed: true,
                    })
                })
            }
            Message::CancelWorkspaceRemoval => {
                if let Some(draft) = &mut self.workspace_management {
                    draft.subview = Subview::List;
                }
                operation::focus(NAME_FIELD)
            }
            _ => Task::none(),
        }
    }

    fn workspace_rename_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PrepareWorkspaceRename(id) => {
                if self.application.workspace_management_pending() {
                    return Task::none();
                }
                let current = self.application.configuration().and_then(|(_, index)| {
                    index
                        .workspaces
                        .iter()
                        .find(|entry| entry.id == id)
                        .map(|entry| entry.name.clone())
                });
                if let (Some(name), Some(draft)) = (current, &mut self.workspace_management) {
                    draft.subview = Subview::List;
                    draft.renaming = Some(RenameDraft {
                        id,
                        name,
                        invalid: false,
                    });
                    return operation::focus(RENAME_FIELD)
                        .chain(operation::select_all(RENAME_FIELD));
                }
                Task::none()
            }
            Message::WorkspaceRenameChanged(value) => {
                if !self.application.workspace_management_pending()
                    && let Some(Draft {
                        renaming: Some(rename),
                        ..
                    }) = &mut self.workspace_management
                {
                    rename.name = crate::domain::validation::sanitize_config_name(&value);
                    rename.invalid = false;
                }
                Task::none()
            }
            Message::ConfirmWorkspaceRename => {
                if self.application.workspace_management_pending() {
                    return Task::none();
                }
                let Some(draft) = &mut self.workspace_management else {
                    return Task::none();
                };
                let Some(rename) = &mut draft.renaming else {
                    return Task::none();
                };
                rename.name = crate::domain::validation::finalize_config_name(&rename.name);
                if !WorkspaceIndex::valid_name(&rename.name) {
                    rename.invalid = true;
                    return Task::none();
                }
                let (id, name) = (rename.id, rename.name.clone());
                draft.renaming = None;
                let task = self.dispatch(Action::RenameWorkspace { id, name });
                Task::batch([task, operation::focus(NAME_FIELD)])
            }
            Message::CancelWorkspaceRename => {
                if let Some(draft) = &mut self.workspace_management {
                    draft.renaming = None;
                }
                operation::focus(NAME_FIELD)
            }
            _ => Task::none(),
        }
    }

    pub(crate) fn workspace_management_input(&mut self, event: Event) -> Task<Message> {
        if let Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modifiers,
            repeat: false,
            ..
        }) = event
        {
            match key {
                keyboard::Key::Named(keyboard::key::Named::Escape) => {
                    let message = match self.workspace_management.as_ref() {
                        Some(Draft {
                            subview: Subview::Remove(_),
                            ..
                        }) => Message::CancelWorkspaceRemoval,
                        Some(Draft {
                            renaming: Some(_), ..
                        }) => Message::CancelWorkspaceRename,
                        Some(Draft {
                            subview: Subview::List,
                            renaming: None,
                            ..
                        })
                        | None => Message::CancelWorkspaceManagement,
                    };
                    return self.workspace_management_message(message);
                }
                keyboard::Key::Named(keyboard::key::Named::Tab) => {
                    return if modifiers.shift() {
                        operation::focus_previous()
                    } else {
                        operation::focus_next()
                    };
                }
                _ => {}
            }
        }
        Task::none()
    }
}

fn control<'a>(
    id: String,
    label: &'a str,
    action: Message,
    enabled: bool,
    palette: a::Palette,
) -> Element<'a, Message> {
    styled_control(id, label, action, enabled, false, palette)
}

fn styled_control<'a>(
    id: String,
    label: &'a str,
    action: Message,
    enabled: bool,
    primary: bool,
    palette: a::Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    let content = button(ty.control(label))
        .padding([7, 10])
        .style(move |_, status| palette.inspector_button(status, primary));
    if enabled {
        focus_control::themed(id, action.clone(), content.on_press(action), palette)
    } else {
        content.into()
    }
}

pub(crate) fn view<'a>(
    application: &'a Application,
    draft: &'a Draft,
    size: Size,
    palette: a::Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    let pending = application.workspace_management_pending();
    let running = application.workspace_management_running();
    let mut content = fields(application, draft, palette);
    if running {
        content = content.push(
            ty.caption(if application.workspace_management_close_waiting() {
                "Finishing workspace transition before close…"
            } else {
                "Saving workspace and applying transition…"
            })
            .color(palette.muted),
        );
    }
    if let Some(error) = application.workspace_management_error() {
        content = content.push(
            column![
                ty.caption(format!(
                    "Workspace transition did not finish. The current workspace is retained.\n{error}"
                ))
                .color(palette.error),
                control(
                    "workspace-management-retry".into(),
                    "Retry",
                    Message::RetryWorkspaceManagement,
                    !running,
                    palette,
                ),
            ]
            .spacing(8),
        );
    }
    let footer = row![
        space().width(Fill),
        control(
            "workspace-management-cancel".into(),
            if pending {
                "Cancel transition"
            } else {
                "Close"
            },
            Message::CancelWorkspaceManagement,
            !running,
            palette,
        ),
    ]
    .spacing(10);
    let panel = column![
        ty.title("Workspaces").color(palette.text),
        a::scroll_area(container(content).padding(iced::Padding {
            right: 14.0,
            ..iced::Padding::ZERO
        }))
        .direction(iced::widget::scrollable::Direction::Vertical(a::scrollbar()))
        .style(palette.scrollable())
        .height(Fill),
        footer
    ]
    .spacing(16);
    container(
        container(panel)
            .padding(20)
            .width((size.width - 32.0).clamp(1.0, 640.0))
            .height((size.height - 32.0).clamp(1.0, 560.0))
            .style(move |_| palette.modal_panel()),
    )
    .padding(16)
    .center_x(Fill)
    .center_y(Fill)
    .style(|_| container::Style {
        background: Some(a::MODAL_SCRIM.into()),
        ..container::Style::default()
    })
    .into()
}

fn fields<'a>(
    application: &'a Application,
    draft: &'a Draft,
    palette: a::Palette,
) -> iced::widget::Column<'a, Message> {
    let ty = palette.typography;
    let pending = application.workspace_management_pending();
    let mut content = column![].spacing(10);
    if let Some((preferences, index)) = application.configuration() {
        if let Subview::Remove(id) = draft.subview {
            return removal_fields(&index.workspaces, id, pending, palette);
        }
        for entry in &index.workspaces {
            if let Some(rename) = draft.renaming.as_ref().filter(|item| item.id == entry.id) {
                content = content.push(inline_rename_row(rename, pending, palette));
            } else {
                let active = preferences.last_workspace_id == Some(entry.id);
                content = content.push(workspace_row(
                    entry,
                    active,
                    pending,
                    index.workspaces.len() > 1,
                    palette,
                ));
            }
        }
        let invalid = draft.invalid;
        let mut input = text_input("New workspace name", &draft.name)
            .font(ty.font())
            .size(ty.size(a::CONTROL_SIZE))
            .id(NAME_FIELD)
            .padding([7, 10])
            .width(Fill)
            .style(move |_, status| {
                crate::dialogs::app_settings::style::field(palette, status, invalid)
            });
        if !pending {
            input = input
                .on_input(Message::ManagementNameChanged)
                .on_submit(Message::CreateWorkspace);
        }
        content = content.push(
            row![
                input,
                control(
                    "workspace-create".into(),
                    "Create and open",
                    Message::CreateWorkspace,
                    !pending && index.workspaces.len() < WorkspaceIndex::MAX_WORKSPACES,
                    palette,
                ),
            ]
            .spacing(8)
            .align_y(iced::Center),
        );
        if draft.invalid {
            content = content.push(
                ty.caption(crate::domain::validation::CONFIG_NAME_ERROR)
                    .color(palette.error),
            );
        }
        if index.workspaces.len() >= WorkspaceIndex::MAX_WORKSPACES {
            content = content.push(
                ty.caption(
                    "Workspace limit reached. Remove an inactive workspace from the list first.",
                )
                .color(palette.muted),
            );
        }
    }
    content
}

fn workspace_row(
    entry: &WorkspaceSummary,
    active: bool,
    pending: bool,
    removable: bool,
    palette: a::Palette,
) -> Element<'_, Message> {
    let ty = palette.typography;
    let label = format!("{}{}", entry.name, if active { " · Active" } else { "" });
    let title = if active {
        ty.control(label).font(ty.heading_font())
    } else {
        ty.control(label)
    };
    row![
        title.color(palette.text).width(Fill),
        control(
            format!("workspace-select-{}", entry.id),
            "Open",
            Message::ChooseWorkspace(entry.id),
            !pending && !active,
            palette,
        ),
        control(
            format!("workspace-rename-{}", entry.id),
            "Rename",
            Message::PrepareWorkspaceRename(entry.id),
            !pending,
            palette,
        ),
        control(
            format!("workspace-remove-{}", entry.id),
            "Remove",
            Message::PrepareWorkspaceRemoval(entry.id),
            !pending && !active && removable,
            palette,
        ),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

fn inline_rename_row(
    rename: &RenameDraft,
    pending: bool,
    palette: a::Palette,
) -> Element<'_, Message> {
    let ty = palette.typography;
    let invalid = rename.invalid;
    let mut input = text_input("Workspace name", &rename.name)
        .font(ty.font())
        .size(ty.size(a::CONTROL_SIZE))
        .id(RENAME_FIELD)
        .padding([7, 10])
        .width(Fill)
        .style(move |_, status| {
            crate::dialogs::app_settings::style::field(palette, status, invalid)
        });
    if !pending {
        input = input
            .on_input(Message::WorkspaceRenameChanged)
            .on_submit(Message::ConfirmWorkspaceRename);
    }
    let line = row![
        input,
        styled_control(
            "workspace-rename-confirm".into(),
            "Save",
            Message::ConfirmWorkspaceRename,
            !pending,
            true,
            palette,
        ),
        control(
            "workspace-rename-cancel".into(),
            "Cancel",
            Message::CancelWorkspaceRename,
            !pending,
            palette,
        ),
    ]
    .spacing(8)
    .align_y(iced::Center);
    if invalid {
        column![
            line,
            ty.caption(crate::domain::validation::CONFIG_NAME_ERROR)
                .color(palette.error),
        ]
        .spacing(6)
        .into()
    } else {
        line.into()
    }
}

fn removal_fields<'a>(
    workspaces: &[WorkspaceSummary],
    id: WorkspaceId,
    pending: bool,
    palette: a::Palette,
) -> iced::widget::Column<'a, Message> {
    let ty = palette.typography;
    let name = workspaces
        .iter()
        .find(|entry| entry.id == id)
        .map_or("Workspace", |entry| entry.name.as_str());
    column![
        ty.control(format!("Remove {name} from the workspace list?"))
            .font(ty.heading_font())
            .color(palette.text),
        ty.text("Its saved file and backup remain on disk. An older index backup may restore an older list.")
            .color(palette.muted),
        row![
            control(
                "workspace-remove-cancel".into(),
                "Cancel",
                Message::CancelWorkspaceRemoval,
                !pending,
                palette,
            ),
            control(
                "workspace-remove-confirm".into(),
                "Remove from list",
                Message::ConfirmWorkspaceRemoval,
                !pending,
                palette,
            )
        ]
        .spacing(10),
    ]
    .spacing(10)
}
