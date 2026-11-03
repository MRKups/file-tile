use std::time::Duration;

use iced::widget::{button, container, tooltip};
use iced::{Border, Element};

use crate::{
    Message,
    widgets::focus_control,
    window::appearance as a,
    window::icons::{self, Icon},
};

pub const TOOL_TARGET: f32 = 34.0;

pub(crate) fn focus_id(icon: Icon, action: &Message) -> String {
    match action {
        Message::Action(crate::application::Action::Navigate(view, _)) => {
            format!("icon-{icon:?}-view-{}", view.0)
        }
        Message::ToggleSort(view) => format!("icon-{icon:?}-view-{}", view.0),
        _ => format!("icon-{icon:?}"),
    }
}

#[derive(Clone, Copy)]
pub enum Treatment {
    Tool { selected: bool },
    Warning,
    Navigation,
    NavigationSelected,
}

/// A labelled icon control. The label stays in its tooltip, not on the button face.
pub fn themed(
    icon: Icon,
    label: impl Into<String>,
    action: Option<Message>,
    treatment: Treatment,
    palette: a::Palette,
) -> Element<'static, Message> {
    let available = action.is_some();
    let selected = matches!(
        treatment,
        Treatment::Tool { selected: true } | Treatment::NavigationSelected
    );
    let (target, color, opacity) = match treatment {
        Treatment::Tool { .. } => (
            TOOL_TARGET,
            if selected {
                palette.tool_active_icon
            } else {
                palette.text
            },
            if available { 1.0 } else { 0.35 },
        ),
        Treatment::Navigation | Treatment::NavigationSelected => {
            (32.0, palette.navigation, if available { 1.0 } else { 0.25 })
        }
        Treatment::Warning => (
            TOOL_TARGET,
            palette.error,
            if available { 1.0 } else { 0.35 },
        ),
    };
    let face = container(icons::view(icon, 19.0, Some(color), opacity))
        .center_x(target)
        .center_y(target);
    let control = button(face)
        .padding(0)
        .on_press_maybe(action.clone())
        .style(move |_, status| style(status, treatment, palette));
    let control: Element<'static, Message> = match action {
        Some(action) if matches!(treatment, Treatment::Tool { .. } | Treatment::Warning) => {
            focus_control::themed_tool(focus_id(icon, &action), action, control, palette)
        }
        Some(action) => focus_control::themed(focus_id(icon, &action), action, control, palette),
        None => control.into(),
    };
    tooltip(
        control,
        palette.typography.small(label.into()),
        tooltip::Position::Bottom,
    )
    .delay(Duration::from_millis(350))
    .gap(5)
    .padding(7)
    .style(move |_| palette.tooltip())
    .into()
}

fn style(status: button::Status, treatment: Treatment, palette: a::Palette) -> button::Style {
    let selected = matches!(
        treatment,
        Treatment::Tool { selected: true } | Treatment::NavigationSelected
    );
    let background = if selected || matches!(status, button::Status::Pressed) {
        Some(palette.tool_active.into())
    } else if matches!(status, button::Status::Hovered) {
        Some(palette.tool_hover.into())
    } else {
        None
    };
    button::Style {
        background,
        text_color: palette.text,
        border: Border {
            radius: if matches!(
                treatment,
                Treatment::Navigation | Treatment::NavigationSelected
            ) {
                6.0
            } else {
                9.0
            }
            .into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}
