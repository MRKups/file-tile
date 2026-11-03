//! Small prompts clipped and scrolled within the affected folder box, never the window.
use crate::application::{
    Action,
    files::file_operations::{ConflictChoice, Item, Operation, Phase},
};
use crate::domain::ViewId;
use crate::{Message, menus::file_actions, widgets::focus_control, window::appearance as a};
use iced::Element;
use iced::widget::{button, column, container, row, scrollable, text_input};

pub(crate) const PANEL_ID: &str = "box-file-prompt";
pub(crate) const BLOCK_ID: &str = "box-file-input-shield";
mod placement;
pub(crate) use placement::view as anchored;

pub(crate) fn position(
    anchor: Option<iced::Point>,
    size: iced::Size,
    content: iced::Size,
) -> iced::Point {
    let anchor = anchor.unwrap_or(iced::Point::ORIGIN);
    iced::Point::new(
        anchor.x.clamp(0.0, (size.width - content.width).max(0.0)),
        anchor.y.clamp(0.0, (size.height - content.height).max(0.0)),
    )
}

pub(crate) fn view<'a>(
    view: ViewId,
    presentation: Option<&'a crate::ViewPresentation>,
    operation: Option<&'a Operation>,
    conflict_name: Option<&'a str>,
    closing: bool,
    palette: a::Palette,
    available_height: f32,
) -> Option<Element<'a, Message>> {
    let ty = palette.typography;
    let content = if let Some(draft) = presentation.and_then(|s| s.name_draft.as_ref()) {
        name_content(view, draft, palette)
    } else if let Some(trash) = presentation.and_then(|s| s.trash_draft.as_ref()) {
        trash_content(view, trash, palette)
    } else {
        let operation = operation?;
        let id = operation.id;
        match &operation.phase {
            Phase::Running(job) => {
                let content = column![
                    ty.text(if closing {
                        "Finishing before Quit…"
                    } else {
                        match job.item {
                            Item::Copy(_) => "Copying…",
                            Item::Move(_) => "Moving…",
                            Item::Create(_) => "Creating…",
                            Item::Rename(_) => "Renaming…",
                            Item::Trash(_) => "Moving to Trash…",
                        }
                    })
                    .font(ty.heading_font()),
                    ty.text(job.name.clone())
                        .wrapping(iced::widget::text::Wrapping::None),
                    ty.caption(if matches!(job.item, Item::Trash(_)) {
                        format!("From: {}", job.directory.display())
                    } else {
                        format!("To: {}", job.directory.display())
                    })
                    .wrapping(iced::widget::text::Wrapping::None),
                    ty.caption(format!(
                        "{} completed · {} skipped · {} total",
                        operation.completed, operation.skipped, operation.total
                    )),
                    ty.caption("Cannot interrupt the active item."),
                ]
                .spacing(6);
                if closing {
                    content.push(control(
                        "file-cancel-quit",
                        "Cancel Quit",
                        Message::Action(Action::CancelClose),
                        palette,
                    ))
                } else if operation.stopped {
                    content.push(ty.small("Stopping after this item…"))
                } else {
                    content.push(control(
                        "file-stop",
                        "Stop after this item",
                        Message::Action(Action::StopFileOperation(id)),
                        palette,
                    ))
                }
            }
            Phase::Conflict(job) => conflict_content(id, job, conflict_name, palette),
            Phase::Finished => finished_content(operation, palette),
        }
    };
    Some(
        container(
            a::scroll_area(container(content).width(220).padding(8))
                .style(palette.scrollable())
                .direction(scrollable::Direction::Both {
                    vertical: a::scrollbar(),
                    horizontal: a::scrollbar(),
                }),
        )
        .id(PANEL_ID)
        .width(iced::Shrink)
        .max_width(300)
        .max_height(available_height.clamp(1.0, 180.0))
        .style(move |_| container::Style {
            background: Some(palette.contrast_background.into()),
            text_color: Some(palette.text),
            border: iced::Border {
                color: palette.border,
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        })
        .into(),
    )
}

fn finished_content<'a>(
    operation: &'a Operation,
    palette: a::Palette,
) -> iced::widget::Column<'a, Message> {
    use crate::application::files::file_operations::ItemStatus;
    let ty = palette.typography;
    let failed = operation
        .outcomes
        .iter()
        .filter(|item| item.status == ItemStatus::Failed)
        .count();
    let cancelled = operation
        .outcomes
        .iter()
        .filter(|item| item.status == ItemStatus::Cancelled)
        .count();
    let mut content = column![ty.text(format!(
        "{} completed · {} skipped\n{failed} failed · {cancelled} cancelled",
        operation.completed, operation.skipped
    ))]
    .spacing(6);
    if cancelled > 0 {
        content = content.push(
            ty.caption("Completed items were not reversed.")
                .color(palette.muted),
        );
    }
    if let Some(error) = &operation.error {
        content = content.push(ty.caption(error).color(palette.error));
    }
    content.push(control(
        "file-operation-done",
        "Done",
        Message::Action(Action::DismissFileOperation(operation.id)),
        palette,
    ))
}

fn name_content<'a>(
    view: ViewId,
    draft: &'a file_actions::NameDraft,
    palette: a::Palette,
) -> iced::widget::Column<'a, Message> {
    let ty = palette.typography;
    let generation = draft.generation;
    let field = text_input("Filename", &draft.value)
        .id(file_actions::name_id(view))
        .font(ty.font())
        .size(ty.size(a::CONTROL_SIZE))
        .padding(6)
        .on_input(move |value| Message::NameChanged(view, generation, value))
        .on_submit(Message::SubmitName(view, generation));
    let field = field.style(move |_, status| palette.path_field(status, draft.error.is_some()));
    let mut content = column![
        ty.text(draft.purpose.title()).font(ty.heading_font()),
        field,
        row![
            control(
                "file-create-cancel",
                "Cancel",
                Message::CancelName(view, generation),
                palette
            ),
            control(
                "file-create",
                draft.purpose.action(),
                Message::SubmitName(view, generation),
                palette
            )
        ]
        .spacing(4)
    ]
    .spacing(6);
    if let Some(error) = &draft.error {
        content = content.push(ty.caption(error).color(palette.error));
    }
    content
}

fn trash_content<'a>(
    view: ViewId,
    draft: &'a file_actions::TrashDraft,
    palette: a::Palette,
) -> iced::widget::Column<'a, Message> {
    let ty = palette.typography;
    let generation = draft.generation;
    let heading = if let [(_, name)] = draft.items.as_slice() {
        format!("Move “{name}” to Trash?")
    } else {
        format!("Move {} items to Trash?", draft.items.len())
    };
    let mut content = column![
        ty.text(heading).font(ty.heading_font()),
        ty.caption("Items can be restored from the macOS Trash.")
            .color(palette.muted),
        row![
            control(
                "file-trash-cancel",
                "Cancel",
                Message::TrashPrompt(view, generation, false),
                palette
            ),
            control(
                "file-trash-confirm",
                "Move to Trash",
                Message::TrashPrompt(view, generation, true),
                palette
            )
        ]
        .spacing(4)
    ]
    .spacing(6);
    if let Some(error) = &draft.error {
        content = content.push(ty.caption(error).color(palette.error));
    }
    content
}

fn conflict_content<'a>(
    id: u64,
    job: &'a crate::application::files::file_operations::Job,
    name: Option<&'a str>,
    palette: a::Palette,
) -> iced::widget::Column<'a, Message> {
    let ty = palette.typography;
    let step = job.step;
    let name = name.unwrap_or(&job.name);
    let field = text_input("New filename", name)
        .id("file-conflict-name")
        .font(ty.font())
        .size(ty.size(a::CONTROL_SIZE))
        .padding(6)
        .on_input(move |value| Message::ConflictNameChanged(id, step, value))
        .on_submit(Message::KeepBoth(id, step));
    let field = field.style(move |_, status| {
        palette.path_field(status, crate::domain::files::validate_name(name).is_err())
    });
    let mut content = column![
        ty.text(format!("“{}” already exists.", job.name))
            .font(ty.heading_font()),
        field,
        row![
            control(
                "file-cancel",
                "Cancel",
                Message::Action(Action::ResolveFileConflict(
                    id,
                    step,
                    ConflictChoice::Cancel
                )),
                palette
            ),
            control(
                "file-skip",
                "Skip",
                Message::Action(Action::ResolveFileConflict(id, step, ConflictChoice::Skip)),
                palette
            ),
            control(
                "file-keep-both",
                if matches!(
                    job.item,
                    crate::application::files::file_operations::Item::Rename(_)
                ) {
                    "Rename"
                } else {
                    "Keep Both"
                },
                Message::KeepBoth(id, step),
                palette
            )
        ]
        .spacing(4),
        ty.caption("Existing entries will not be replaced.")
    ]
    .spacing(6);
    if let Err(error) = crate::domain::files::validate_name(name) {
        content = content.push(ty.caption(error).color(palette.error));
    }
    content
}

fn control<'a>(
    id: &'static str,
    label: &'static str,
    action: Message,
    palette: a::Palette,
) -> Element<'a, Message> {
    focus_control::themed(
        id,
        action.clone(),
        button(palette.typography.small(label))
            .padding([5, 8])
            .style(move |_, status| palette.text_button(status))
            .on_press(action),
        palette,
    )
}
