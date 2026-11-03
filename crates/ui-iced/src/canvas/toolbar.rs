use iced::widget::{Row, container, rule};
use iced::{Center, Element};

use crate::application::Action;
use crate::domain::Mode;

use crate::{
    Message,
    widgets::icon_button::{self, Treatment},
    window::appearance as a,
    window::icons::Icon,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tool {
    Browse,
    Configure,
    Layout,
}

const TOOLS: [Tool; 3] = [Tool::Browse, Tool::Configure, Tool::Layout];
const SPACING: f32 = 6.0;
const WORKSPACE_WIDTH: f32 =
    TOOLS.len() as f32 * icon_button::TOOL_TARGET + (TOOLS.len() - 1) as f32 * SPACING;
const GROUP_GAP: f32 = 12.0;
pub const WIDTH: f32 = WORKSPACE_WIDTH + 2.0 * GROUP_GAP + 1.0 + icon_button::TOOL_TARGET;

impl Tool {
    fn icon(self) -> Icon {
        match self {
            Self::Browse => Icon::Eye,
            Self::Configure => Icon::Sliders,
            Self::Layout => Icon::Move,
        }
    }

    fn mode(self) -> Mode {
        match self {
            Self::Browse => Mode::Browse,
            Self::Layout => Mode::Layout,
            Self::Configure => Mode::Configure,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Browse => "Browse",
            Self::Configure => "Configure boxes and shortcuts",
            Self::Layout => "Layout",
        }
    }
}

pub fn view(
    active: Mode,
    workspace_enabled: bool,
    settings_enabled: bool,
    storage_attention: bool,
    recovery_enabled: bool,
    palette: a::Palette,
) -> Element<'static, Message> {
    let workspace_tools = Row::with_children(TOOLS.map(|tool| {
        let mode = tool.mode();
        icon_button::themed(
            tool.icon(),
            tool.label(),
            workspace_enabled.then_some(Message::Action(Action::SelectMode(mode))),
            Treatment::Tool {
                selected: mode == active,
            },
            palette,
        )
    }))
    .spacing(SPACING);
    let settings = icon_button::themed(
        Icon::Cog,
        if recovery_enabled {
            "Workspace recovery · Open details"
        } else if storage_attention {
            "Application settings · Workspace not saved"
        } else if settings_enabled {
            "Application settings"
        } else {
            "Application settings · Saved configuration must be ready"
        },
        if recovery_enabled {
            Some(Message::StorageDetails(true))
        } else {
            settings_enabled.then_some(Message::OpenAppSettings)
        },
        if storage_attention {
            Treatment::Warning
        } else {
            Treatment::Tool { selected: false }
        },
        palette,
    );
    Row::new()
        .push(workspace_tools)
        .push(container(rule::vertical(1).style(move |_| palette.rule())).height(20))
        .push(settings)
        .spacing(GROUP_GAP)
        .align_y(Center)
        .into()
}
