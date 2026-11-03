use iced::widget::{Stack, column, container, mouse_area, row, space, text, tooltip};
use iced::{Center, Element, Fill, Point, Rectangle};

use crate::application::{Action, Application};
use crate::domain::Mode;

use crate::{
    Message,
    canvas::toolbar,
    widgets::icon_button::{self, Treatment},
    window::appearance as a,
    window::icons::Icon,
};

const CORNER_INSET: f32 = 8.0;
pub const CONTROL_HEIGHT: f32 = CORNER_INSET + icon_button::TOOL_TARGET;
const MODE_HEIGHT: f32 = 30.0;
const TRAFFIC_LIGHTS: f32 = 100.0;
const SPACING: f32 = 6.0;
const NAME_MAX_WIDTH: f32 = 128.0;
const ACTIONS_WIDTH: f32 = 2.0 * (icon_button::TOOL_TARGET + SPACING);

pub fn height(width: f32, mode: Mode) -> f32 {
    if mode != Mode::Browse && width <= 1000.0 {
        CONTROL_HEIGHT + MODE_HEIGHT
    } else {
        CONTROL_HEIGHT
    }
}

fn name_width(width: f32) -> f32 {
    (width - CORNER_INSET - toolbar::WIDTH - TRAFFIC_LIGHTS - 12.0 - ACTIONS_WIDTH)
        .clamp(1.0, NAME_MAX_WIDTH)
}

fn reserved_undo_bounds(width: f32) -> Rectangle {
    Rectangle {
        x: TRAFFIC_LIGHTS + name_width(width) + SPACING * 2.0 + icon_button::TOOL_TARGET,
        y: CORNER_INSET,
        width: icon_button::TOOL_TARGET,
        height: icon_button::TOOL_TARGET,
    }
}

/// Reserve OS-owned traffic lights and both control groups, even when disabled.
pub fn can_drag(point: Point, width: f32, height: f32, mode: Mode) -> bool {
    if point.y < 0.0 || point.y >= height || point.x < TRAFFIC_LIGHTS {
        return false;
    }
    let toolbar_start = width - CORNER_INSET - toolbar::WIDTH;
    if point.y < CONTROL_HEIGHT && point.x >= toolbar_start {
        return false;
    }
    if mode != Mode::Browse && point.y < CONTROL_HEIGHT {
        point.x >= reserved_undo_bounds(width).x + icon_button::TOOL_TARGET
    } else {
        true
    }
}

pub fn view(
    application: &Application,
    width: f32,
    height: f32,
    workspace_enabled: bool,
    settings_enabled: bool,
    palette: a::Palette,
) -> Element<'_, Message> {
    let mode = application.workspace().mode();
    let tools: Element<'_, Message> = if mode == Mode::Browse {
        space().into()
    } else {
        workspace_tools(
            application,
            width,
            workspace_enabled,
            settings_enabled,
            palette,
        )
    };
    let row_height = icon_button::TOOL_TARGET;
    let recovery = application.can_retry_startup() && workspace_enabled;
    let controls = container(
        row![
            space().width(TRAFFIC_LIGHTS),
            tools,
            space().width(Fill),
            toolbar::view(
                mode,
                workspace_enabled,
                settings_enabled,
                application.save_failed() || application.can_retry_startup(),
                recovery,
                palette
            ),
        ]
        .height(row_height)
        .align_y(Center),
    )
    .padding(iced::Padding {
        top: CORNER_INSET,
        right: CORNER_INSET,
        ..iced::Padding::ZERO
    })
    .width(Fill)
    .height(CONTROL_HEIGHT);
    let narrow = height > CONTROL_HEIGHT;
    let first_row = Stack::new()
        .push(controls)
        .push(
            container(
                container(if narrow {
                    space().into()
                } else {
                    instruction(application, palette)
                })
                .center_x(Fill)
                .center_y(row_height),
            )
            .padding(iced::Padding {
                top: CORNER_INSET,
                ..iced::Padding::ZERO
            })
            .width(Fill)
            .height(CONTROL_HEIGHT),
        )
        .width(Fill)
        .height(CONTROL_HEIGHT);
    let header: Element<'_, Message> = column![
        first_row,
        container(if narrow {
            instruction(application, palette)
        } else {
            space().into()
        })
        .center_x(Fill)
        .center_y(if narrow { MODE_HEIGHT } else { 0.0 }),
    ]
    .width(Fill)
    .height(height)
    .into();
    mouse_area(header).on_press(Message::DragWindow).into()
}

fn workspace_tools(
    application: &Application,
    width: f32,
    workspace_enabled: bool,
    settings_enabled: bool,
    palette: a::Palette,
) -> Element<'_, Message> {
    let name = application.workspace_name().unwrap_or("Workspace");
    let manage_enabled = settings_enabled && application.can_manage_workspaces();
    row![
        tooltip(
            name_control(name, name_width(width), manage_enabled, palette),
            palette.typography.small("Manage workspaces"),
            tooltip::Position::Bottom,
        )
        .style(move |_| palette.tooltip()),
        icon_button::themed(
            Icon::Rename,
            "Manage workspaces",
            manage_enabled.then_some(Message::OpenWorkspaceManagement),
            Treatment::Tool { selected: false },
            palette,
        ),
        icon_button::themed(
            Icon::Undo,
            "Undo workspace change",
            (workspace_enabled && application.workspace().can_undo())
                .then_some(Message::Action(Action::Undo)),
            Treatment::Tool { selected: false },
            palette,
        ),
    ]
    .spacing(SPACING)
    .align_y(Center)
    .into()
}

fn name_control(
    name: &str,
    width: f32,
    enabled: bool,
    palette: a::Palette,
) -> Element<'_, Message> {
    let ty = palette.typography;
    let label = container(
        ty.control(name)
            .font(ty.heading_font())
            .color(palette.text)
            .wrapping(text::Wrapping::None),
    )
    .max_width(width)
    .height(ty.row(20.0))
    .align_y(Center)
    .clip(true);
    if enabled {
        crate::widgets::focus_control::themed(
            "workspace-management",
            Message::OpenWorkspaceManagement,
            mouse_area(label).on_press(Message::OpenWorkspaceManagement),
            palette,
        )
    } else {
        label.into()
    }
}

fn instruction(application: &Application, palette: a::Palette) -> Element<'_, Message> {
    let ty = palette.typography;
    if application.workspace().mode() == Mode::Browse {
        return space().into();
    }
    if application.workspace().mode() == Mode::Configure {
        return ty
            .small(
                if application
                    .workspace()
                    .selected()
                    .is_some_and(|id| application.workspace().launcher(id).is_some())
                {
                    "Configure · Edit shortcut"
                } else if application.workspace().selected().is_some() {
                    "Configure · Edit this box"
                } else {
                    "Configure · Choose a box or shortcut"
                },
            )
            .color(palette.mode_text)
            .into();
    }
    if let Some(error) = application.status() {
        return tooltip(
            ty.small("Layout · Edit not applied").color(palette.error),
            ty.small(error).width(300),
            tooltip::Position::Bottom,
        )
        .style(move |_| palette.tooltip())
        .into();
    }
    let (feedback, invalid) = if let Some(preview) = application.workspace().layout_preview() {
        (
            if preview.valid {
                format!(
                    "{} × {} · {}",
                    preview.rect.width,
                    preview.rect.height,
                    if matches!(preview.kind, crate::domain::LayoutKind::Resize(_)) {
                        "Release to resize"
                    } else if preview.swap.is_some() {
                        "Release to swap"
                    } else {
                        "Release to place"
                    }
                )
            } else {
                "No room here".into()
            },
            !preview.valid,
        )
    } else if let Some(preview) = application.workspace().draw_preview() {
        match preview.outcome {
            Ok(crate::domain::ItemKind::Launcher) => (
                format!(
                    "{} × {} · Release to create shortcut",
                    preview.rect.width, preview.rect.height
                ),
                false,
            ),
            Ok(crate::domain::ItemKind::FolderBox) => (
                format!(
                    "{} × {} · Release to create box",
                    preview.rect.width, preview.rect.height
                ),
                false,
            ),
            Err(crate::domain::DrawRejection::Overlap) => ("No room here".into(), true),
            Err(crate::domain::DrawRejection::ThinStrip) => (
                format!(
                    "{} × {} · 1-cell strips stop at 2",
                    preview.rect.width, preview.rect.height
                ),
                true,
            ),
        }
    } else {
        ("Drag empty cells, boxes or edges".into(), false)
    };
    ty.small(format!("Layout · {feedback}"))
        .color(if invalid {
            palette.error
        } else {
            palette.mode_text
        })
        .into()
}
