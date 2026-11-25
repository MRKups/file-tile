use std::path::Path;

use iced::widget::{Row, container, row, text, text_input};
use iced::{Element, Fill, Task};

use crate::application::{Action, LoadState, Navigate};
use crate::domain::{BoxSettings, Navigation, ViewId};

use crate::{
    Desktop, Message,
    widgets::icon_button::{self, Treatment},
    window::appearance as a,
    window::icons::Icon,
};

pub const HEIGHT: f32 = 48.0;
pub const NARROW_HEIGHT: f32 = 84.0;

pub struct Props<'a> {
    pub palette: a::Palette,
    pub view: ViewId,
    pub navigation: &'a Navigation,
    pub home: &'a Path,
    pub draft: Option<&'a str>,
    pub invalid: bool,
    pub interactive: bool,
    pub sort_open: bool,
    pub settings: BoxSettings,
}

fn button_width(settings: BoxSettings) -> f32 {
    let count = 1
        + usize::from(settings.show_start)
        + usize::from(settings.show_forward)
        + usize::from(settings.show_up);
    count as f32 * 32.0 + (count - 1) as f32 * 3.0
}

pub fn minimum_width(settings: BoxSettings) -> f32 {
    132.0_f32.max(button_width(settings) + 20.0)
}

fn path_width(width: f32, settings: BoxSettings) -> f32 {
    (width
        - 20.0
        - if narrow(width, settings) {
            40.0
        } else {
            button_width(settings) + 48.0
        })
    .max(1.0)
}

fn narrow(width: f32, settings: BoxSettings) -> bool {
    width <= 220.0 || width < button_width(settings) + 20.0 + 8.0 + 140.0
}

pub fn height(width: f32, settings: BoxSettings, ty: crate::window::typography::Style) -> f32 {
    if narrow(width, settings) {
        (NARROW_HEIGHT + (ty.size(a::SMALL_SIZE) - a::SMALL_SIZE) * 1.3).ceil()
    } else {
        HEIGHT
    }
}

pub fn path_id(view: ViewId) -> String {
    format!("path-{}", view.0)
}

pub fn display_path(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(relative) if relative.as_os_str().is_empty() => "~".into(),
        Ok(relative) => format!("~/{}", relative.to_string_lossy()),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

impl Desktop {
    /// Keep local path drafts independent of modal input and accepted navigation.
    pub(crate) fn path_message(&mut self, message: Message) -> Task<Message> {
        let view = match &message {
            Message::PathChanged(view, _) | Message::PathSubmitted(view) => *view,
            _ => return Task::none(),
        };
        if !self.workspace_input_enabled() || !self.application.workspace().can_browse(view) {
            return Task::none();
        }
        match message {
            Message::PathChanged(view, draft) => {
                let presentation = self.views.entry(view).or_default();
                presentation.draft = Some(draft);
                presentation.draft_revision = presentation.draft_revision.saturating_add(1);
                presentation.invalid = false;
                self.focus_version = self.focus_version.saturating_add(1);
                self.focused_path = Some(view);
                self.activate_view(view);
                Task::none()
            }
            Message::PathSubmitted(view) => {
                self.activate_view(view);
                let command =
                    self.views
                        .get(&view)
                        .and_then(|presentation| presentation.draft.clone())
                        .map(Navigate::Typed)
                        .or_else(|| {
                            self.application.workspace().view(view).map(|item| {
                                Navigate::Folder(item.navigation.current().to_path_buf())
                            })
                        });
                if let Some(command) = command {
                    let task = self.dispatch(Action::Navigate(view, command));
                    if matches!(self.application.load(view), Some(LoadState::Ready(_))) {
                        let presentation = self.views.entry(view).or_default();
                        presentation.draft = None;
                        presentation.invalid = false;
                    }
                    task
                } else {
                    Task::none()
                }
            }
            _ => Task::none(),
        }
    }
}

pub fn view(props: Props<'_>, width: f32) -> Element<'_, Message> {
    let palette = props.palette;
    let ty = palette.typography;
    let path = props
        .draft
        .map(str::to_owned)
        .unwrap_or_else(|| display_path(props.navigation.current(), props.home));
    let id = props.view;
    let invalid = props.invalid;
    let path_control: Element<'_, Message> = if props.interactive {
        text_input("Folder path", &path)
            .id(path_id(id))
            .size(ty.size(a::SMALL_SIZE))
            .font(ty.font())
            .padding(5)
            .on_input(move |draft| Message::PathChanged(id, draft))
            .on_submit(Message::PathSubmitted(id))
            .style(move |_, status| palette.path_field(status, invalid))
            .into()
    } else {
        // No editor while arranging: disabled callbacks leave focus/clipboard behavior active.
        container(
            ty.small(path)
                .color(palette.path_text)
                .wrapping(text::Wrapping::None),
        )
        .padding(5)
        .width(Fill)
        .clip(true)
        .into()
    };
    let buttons = navigation_buttons(&props);
    let path_width = path_width(width, props.settings);
    // Reflow the same child widgets, rather than replacing a Row with a Column
    // and discarding the path editor's caret/focus at this breakpoint.
    container(
        row![
            buttons,
            row![
                container(path_control).width(path_width),
                crate::menus::sort_menu::control(
                    id,
                    props.settings.sort,
                    props.sort_open,
                    props.interactive,
                    palette,
                )
            ]
            .spacing(8)
            .align_y(iced::Center)
        ]
        .spacing(8)
        .align_y(iced::Center)
        .wrap()
        .vertical_spacing(3),
    )
    .padding([0, 10])
    .height(height(width, props.settings, ty))
    .width(Fill)
    .align_y(iced::Center)
    .into()
}

fn navigation_buttons(props: &Props<'_>) -> Row<'static, Message> {
    let id = props.view;
    let palette = props.palette;
    let back = icon_button::themed(
        Icon::Back,
        "Back",
        (props.interactive && props.navigation.back_target().is_some())
            .then_some(Message::Action(Action::Navigate(id, Navigate::Back))),
        Treatment::Navigation,
        palette,
    );
    let start = icon_button::themed(
        Icon::Home,
        "Return to start",
        (props.interactive && props.navigation.current() != props.navigation.return_target())
            .then_some(Message::Action(Action::Navigate(
                id,
                Navigate::ReturnToStart,
            ))),
        Treatment::Navigation,
        palette,
    );
    let mut buttons = Row::new();
    if props.settings.show_start {
        buttons = buttons.push(start);
    }
    buttons = buttons.push(back);
    if props.settings.show_forward {
        buttons = buttons.push(icon_button::themed(
            Icon::Forward,
            "Forward",
            (props.interactive && props.navigation.forward_target().is_some())
                .then_some(Message::Action(Action::Navigate(id, Navigate::Forward))),
            Treatment::Navigation,
            palette,
        ));
    }
    if props.settings.show_up {
        buttons = buttons.push(icon_button::themed(
            Icon::Up,
            "Up",
            (props.interactive && props.navigation.up_target().is_some())
                .then_some(Message::Action(Action::Navigate(id, Navigate::Up))),
            Treatment::Navigation,
            palette,
        ));
    }
    buttons.spacing(3)
}
