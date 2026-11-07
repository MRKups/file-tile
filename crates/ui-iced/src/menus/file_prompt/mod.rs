//! Small prompts clipped and scrolled within the affected folder box, never the window.
use crate::application::{
    Action,
    files::file_operations::{ConflictChoice, Item, Operation, Phase},
};
use crate::domain::ViewId;
use crate::{
    Message, menus::file_actions, widgets::focus_control, window::appearance as a, window::icons,
};
use iced::Element;
use iced::widget::{button, column, container, opaque, row, scrollable, text_input};

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
    available_size: iced::Size,
) -> Option<Element<'a, Message>> {
    let is_trash = presentation.and_then(|s| s.name_draft.as_ref()).is_none()
        && presentation.and_then(|s| s.trash_draft.as_ref()).is_some();
    let content: Element<'a, Message> =
        if let Some(draft) = presentation.and_then(|s| s.name_draft.as_ref()) {
            name_content(view, draft, palette).into()
        } else if let Some(trash) = presentation.and_then(|s| s.trash_draft.as_ref()) {
            trash_content(view, trash, palette)
        } else {
            let operation = operation?;
            match &operation.phase {
                Phase::Running(job) => running_content(operation, job, closing, palette).into(),
                Phase::Conflict(job) => {
                    conflict_content(operation.id, job, conflict_name, palette).into()
                }
                Phase::Finished => finished_content(operation, palette).into(),
            }
        };
    let (card_width, max_width, max_height, padding) = if is_trash {
        let width = (available_size.width - 24.0).clamp(220.0, 320.0);
        (
            width,
            340.0,
            available_size.height.clamp(1.0, 220.0),
            iced::Padding {
                top: 14.0,
                right: 16.0,
                bottom: 14.0,
                left: 16.0,
            },
        )
    } else {
        (
            232.0,
            300.0,
            available_size.height.clamp(1.0, 190.0),
            iced::Padding {
                top: 10.0,
                right: 12.0,
                bottom: 10.0,
                left: 12.0,
            },
        )
    };
    Some(opaque(
        container(
            a::scroll_area(container(content).width(card_width).padding(padding))
                .style(palette.scrollable())
                .direction(scrollable::Direction::Both {
                    vertical: a::scrollbar(),
                    horizontal: a::scrollbar(),
                }),
        )
        .id(PANEL_ID)
        .width(iced::Shrink)
        .max_width(max_width)
        .max_height(max_height)
        .style(move |_| container::Style {
            background: Some(palette.contrast_background.into()),
            text_color: Some(palette.text),
            border: iced::Border {
                color: palette.border,
                width: 1.0,
                radius: 10.0.into(),
            },
            shadow: iced::Shadow {
                color: iced::Color::BLACK.scale_alpha(0.24),
                offset: iced::Vector::new(0.0, 6.0),
                blur_radius: 18.0,
            },
            ..Default::default()
        }),
    ))
}

fn running_content<'a>(
    operation: &'a Operation,
    job: &'a crate::application::files::file_operations::Job,
    closing: bool,
    palette: a::Palette,
) -> iced::widget::Column<'a, Message> {
    let ty = palette.typography;
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
            Message::Action(Action::StopFileOperation(operation.id)),
            palette,
        ))
    }
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
) -> Element<'a, Message> {
    let ty = palette.typography;
    let generation = draft.generation;
    let heading = if let [(_, name)] = draft.items.as_slice() {
        format!("Move “{name}” to Trash?")
    } else {
        format!("Move {} items to Trash?", draft.items.len())
    };
    let mut copy = column![
        ty.text(heading).font(ty.heading_font()).color(palette.text),
        ty.caption("Items can be restored from the macOS Trash.")
            .color(palette.muted),
    ]
    .spacing(4);
    if let Some(error) = &draft.error {
        copy = copy.push(ty.caption(error).color(palette.error));
    }
    let body = column![
        copy,
        row![
            control(
                "file-trash-cancel",
                "Cancel",
                Message::TrashPrompt(view, generation, false),
                palette
            ),
            danger_control(
                "file-trash-confirm",
                "Move to Trash",
                Message::TrashPrompt(view, generation, true),
                palette
            )
        ]
        .spacing(8)
    ]
    .spacing(12)
    .width(iced::Fill);

    row![
        container(icons::view(
            icons::Icon::Trash,
            20.0,
            Some(palette.muted),
            1.0
        ))
        .padding(iced::Padding {
            top: 2.0,
            ..iced::Padding::ZERO
        }),
        body,
    ]
    .spacing(12)
    .align_y(iced::Alignment::Start)
    .into()
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
    focus_control::themed_highlight_keys_radius(
        id,
        action.clone(),
        button(palette.typography.control(label))
            .padding([6, 12])
            .style(move |_, status| palette.inspector_button(status, false))
            .on_press(action),
        Vec::new(),
        8.0.into(),
        palette,
    )
}

fn danger_control<'a>(
    id: &'static str,
    label: &'static str,
    action: Message,
    palette: a::Palette,
) -> Element<'a, Message> {
    focus_control::themed_highlight_keys_radius(
        id,
        action.clone(),
        button(palette.typography.control(label))
            .padding([6, 12])
            .style(move |_, status| {
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: hovered.then_some(palette.remove_fill.into()),
                    text_color: if hovered { palette.error } else { palette.text },
                    border: iced::Border {
                        color: if hovered {
                            palette.error
                        } else {
                            palette.border
                        },
                        width: 1.0,
                        radius: 8.0.into(),
                    },
                    ..button::Style::default()
                }
            })
            .on_press(action),
        Vec::new(),
        8.0.into(),
        palette,
    )
}
