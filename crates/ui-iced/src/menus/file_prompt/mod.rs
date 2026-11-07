//! Small prompts clipped and scrolled within the affected folder box, never the window.
use crate::application::{
    Action,
    files::file_operations::{ConflictChoice, Item, Operation, Phase},
};
use crate::domain::{ViewId, files::CreateKind};
use crate::{
    Message, dialogs::app_settings::style as field_style, menus::file_actions,
    widgets::focus_control, window::appearance as a, window::icons,
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

pub(crate) fn outside_press(
    view: ViewId,
    presentation: Option<&crate::ViewPresentation>,
) -> Option<Message> {
    let state = presentation?;
    if let Some(draft) = &state.name_draft {
        Some(Message::CancelName(view, draft.generation))
    } else {
        state
            .trash_draft
            .as_ref()
            .map(|draft| Message::TrashPrompt(view, draft.generation, false))
    }
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
    let content: Element<'a, Message> =
        if let Some(draft) = presentation.and_then(|s| s.name_draft.as_ref()) {
            name_content(view, draft, palette)
        } else if let Some(trash) = presentation.and_then(|s| s.trash_draft.as_ref()) {
            trash_content(view, trash, palette)
        } else {
            let operation = operation?;
            match &operation.phase {
                Phase::Running(job) => running_content(operation, job, closing, palette),
                Phase::Conflict(job) => conflict_content(operation.id, job, conflict_name, palette),
                Phase::Finished => finished_content(operation, palette),
            }
        };
    let card_width = (available_size.width - 24.0).clamp(220.0, 320.0);
    let max_width = 340.0;
    let max_height = available_size.height.clamp(1.0, 220.0);
    let padding = iced::Padding {
        top: 14.0,
        right: 16.0,
        bottom: 14.0,
        left: 16.0,
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

fn with_icon<'a>(
    icon: icons::Icon,
    body: impl Into<Element<'a, Message>>,
    palette: a::Palette,
) -> Element<'a, Message> {
    row![
        container(icons::view(icon, 20.0, Some(palette.muted), 1.0)).padding(iced::Padding {
            top: 2.0,
            ..iced::Padding::ZERO
        }),
        body.into(),
    ]
    .spacing(12)
    .align_y(iced::Alignment::Start)
    .into()
}

fn item_icon(item: &Item) -> icons::Icon {
    match item {
        Item::Copy(_) => icons::Icon::Copy,
        Item::Move(_) => icons::Icon::Move,
        Item::Create(CreateKind::Folder) => icons::Icon::Folder,
        Item::Create(_) => icons::Icon::Document,
        Item::Rename(_) => icons::Icon::Rename,
        Item::Trash(_) => icons::Icon::Trash,
    }
}

fn running_content<'a>(
    operation: &'a Operation,
    job: &'a crate::application::files::file_operations::Job,
    closing: bool,
    palette: a::Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    let copy = column![
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
        .color(palette.muted)
        .wrapping(iced::widget::text::Wrapping::None),
        ty.caption(format!(
            "{} completed · {} skipped · {} total",
            operation.completed, operation.skipped, operation.total
        ))
        .color(palette.muted),
    ]
    .spacing(4);
    let action: Element<'a, Message> = if closing {
        control(
            "file-cancel-quit",
            "Cancel Quit",
            Message::Action(Action::CancelClose),
            palette,
        )
    } else if operation.stopped {
        ty.small("Stopping after this item…")
            .color(palette.muted)
            .into()
    } else {
        control(
            "file-stop",
            "Stop after this item",
            Message::Action(Action::StopFileOperation(operation.id)),
            palette,
        )
    };
    let body = column![copy, action].spacing(12).width(iced::Fill);
    with_icon(item_icon(&job.item), body, palette)
}

fn finished_content<'a>(operation: &'a Operation, palette: a::Palette) -> Element<'a, Message> {
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
    let mut copy = column![
        ty.text(format!(
            "{} completed · {} skipped\n{failed} failed · {cancelled} cancelled",
            operation.completed, operation.skipped
        ))
        .font(ty.heading_font())
    ]
    .spacing(4);
    if cancelled > 0 {
        copy = copy.push(
            ty.caption("Completed items were not reversed.")
                .color(palette.muted),
        );
    }
    if let Some(error) = &operation.error {
        copy = copy.push(ty.caption(error).color(palette.error));
    }
    let body = column![
        copy,
        primary_control(
            "file-operation-done",
            "Done",
            Message::Action(Action::DismissFileOperation(operation.id)),
            palette,
        )
    ]
    .spacing(12)
    .width(iced::Fill);
    with_icon(icons::Icon::Info, body, palette)
}

fn name_content<'a>(
    view: ViewId,
    draft: &'a file_actions::NameDraft,
    palette: a::Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    let generation = draft.generation;
    let icon = match draft.purpose {
        file_actions::NamePurpose::Rename(_) => icons::Icon::Rename,
        file_actions::NamePurpose::Create(CreateKind::Folder) => icons::Icon::Folder,
        file_actions::NamePurpose::Create(_) => icons::Icon::Document,
    };
    let field = text_input("Filename", &draft.value)
        .id(file_actions::name_id(view))
        .font(ty.font())
        .size(ty.size(a::CONTROL_SIZE))
        .padding([7, 10])
        .on_input(move |value| Message::NameChanged(view, generation, value))
        .on_submit(Message::SubmitName(view, generation))
        .style(move |_, status| field_style::field(palette, status, draft.error.is_some()));
    let mut copy = column![
        ty.text(draft.purpose.title())
            .font(ty.heading_font())
            .color(palette.text),
        field,
    ]
    .spacing(8);
    if let Some(error) = &draft.error {
        copy = copy.push(ty.caption(error).color(palette.error));
    }
    let body = column![
        copy,
        row![
            control(
                "file-create-cancel",
                "Cancel",
                Message::CancelName(view, generation),
                palette
            ),
            primary_control(
                "file-create",
                draft.purpose.action(),
                Message::SubmitName(view, generation),
                palette
            )
        ]
        .spacing(8)
    ]
    .spacing(12)
    .width(iced::Fill);
    with_icon(icon, body, palette)
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
    let mut copy = column![ty.text(heading).font(ty.heading_font()).color(palette.text)].spacing(4);
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
    with_icon(icons::Icon::Trash, body, palette)
}

fn conflict_content<'a>(
    id: u64,
    job: &'a crate::application::files::file_operations::Job,
    name: Option<&'a str>,
    palette: a::Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    let step = job.step;
    let name = name.unwrap_or(&job.name);
    let invalid = crate::domain::files::validate_name(name).is_err();
    let field = text_input("New filename", name)
        .id("file-conflict-name")
        .font(ty.font())
        .size(ty.size(a::CONTROL_SIZE))
        .padding([7, 10])
        .on_input(move |value| Message::ConflictNameChanged(id, step, value))
        .on_submit(Message::KeepBoth(id, step))
        .style(move |_, status| field_style::field(palette, status, invalid));
    let mut copy = column![
        ty.text(format!("“{}” already exists.", job.name))
            .font(ty.heading_font())
            .color(palette.text),
        field,
    ]
    .spacing(8);
    if let Err(error) = crate::domain::files::validate_name(name) {
        copy = copy.push(ty.caption(error).color(palette.error));
    }
    let body = column![
        copy,
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
            primary_control(
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
        .spacing(8),
    ]
    .spacing(12)
    .width(iced::Fill);
    with_icon(item_icon(&job.item), body, palette)
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

fn primary_control<'a>(
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
                    background: hovered.then_some(palette.tool_active.into()),
                    text_color: if hovered {
                        palette.accent
                    } else {
                        palette.text
                    },
                    border: iced::Border {
                        color: if hovered {
                            palette.accent
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
