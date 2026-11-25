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
const SEPARATOR_SPAN: f32 = 2.0 * GROUP_GAP + 1.0;
pub const WIDTH: f32 = icon_button::TOOL_TARGET
    + SEPARATOR_SPAN
    + WORKSPACE_WIDTH
    + SEPARATOR_SPAN
    + icon_button::TOOL_TARGET;

#[derive(Debug, Clone, Copy)]
pub struct State {
    pub active: Mode,
    pub search_enabled: bool,
    pub search_open: bool,
    pub workspace_enabled: bool,
    pub settings_enabled: bool,
    pub storage_attention: bool,
    pub recovery_enabled: bool,
}

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

pub fn view(state: State, palette: a::Palette) -> Element<'static, Message> {
    let search = icon_button::themed(
        Icon::Search,
        if state.active != Mode::Browse {
            "Search · Available in Browse mode"
        } else {
            "Search · ⌘F"
        },
        state
            .search_enabled
            .then_some(Message::Search(crate::dialogs::search::Action::Open)),
        Treatment::Tool {
            selected: state.search_open,
        },
        palette,
    );
    let workspace_tools = Row::with_children(TOOLS.map(|tool| {
        let mode = tool.mode();
        icon_button::themed(
            tool.icon(),
            tool.label(),
            state
                .workspace_enabled
                .then_some(Message::Action(Action::SelectMode(mode))),
            Treatment::Tool {
                selected: mode == state.active,
            },
            palette,
        )
    }))
    .spacing(SPACING);
    let settings = icon_button::themed(
        Icon::Cog,
        if state.recovery_enabled {
            "Workspace recovery · Open details"
        } else if state.storage_attention {
            "Application settings · Workspace not saved"
        } else if state.settings_enabled {
            "Application settings"
        } else {
            "Application settings · Saved configuration must be ready"
        },
        if state.recovery_enabled {
            Some(Message::StorageDetails(true))
        } else {
            state.settings_enabled.then_some(Message::OpenAppSettings)
        },
        if state.storage_attention {
            Treatment::Warning
        } else {
            Treatment::Tool { selected: false }
        },
        palette,
    );
    Row::new()
        .push(search)
        .push(container(rule::vertical(1).style(move |_| palette.rule())).height(20))
        .push(workspace_tools)
        .push(container(rule::vertical(1).style(move |_| palette.rule())).height(20))
        .push(settings)
        .spacing(GROUP_GAP)
        .align_y(Center)
        .into()
}
