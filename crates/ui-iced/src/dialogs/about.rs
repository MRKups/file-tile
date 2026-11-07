//! In-app About dialog with version, author, acknowledgments, bundled fonts, and GitHub link.

use std::sync::OnceLock;
use std::time::Duration;

use iced::widget::{
    button, column, container, operation, row, rule, scrollable, space, svg, tooltip,
};
use iced::{Element, Event, Fill, Padding, Size, Task, keyboard};

use crate::domain::ViewId;

use crate::{
    Desktop, Message,
    boxes::navigation_header,
    widgets::focus_control,
    window::appearance as a,
    window::icons::{self, Icon},
};

pub(crate) const GITHUB_URL: &str = "https://github.com/MRKups/file-tile";
const APP_ICON_SVG: &[u8] = include_bytes!("../../../../assets/icons/app/icon.svg");
const CLOSE_ID: &str = "about-close";
const GITHUB_ID: &str = "about-github";
const UPDATES_ID: &str = "about-updates";

const ACKNOWLEDGMENTS_NOTE: &str = "This project would not be possible without the efforts of many open source projects and contributors. I'd like to thank the following projects, their contributors, and their dependencies.";

const ACKNOWLEDGMENTS: [(&str, &str); 14] = [
    ("Iced", "MIT"),
    ("winit", "Apache-2.0"),
    ("wgpu", "MIT / Apache-2.0"),
    ("cosmic-text", "MIT / Apache-2.0"),
    ("resvg", "Apache-2.0 / MIT"),
    ("tiny-skia", "BSD-3-Clause"),
    ("objc2", "MIT"),
    ("Sparkle", "MIT"),
    ("notify", "CC0-1.0"),
    ("image", "MIT / Apache-2.0"),
    ("serde", "MIT / Apache-2.0"),
    ("serde_json", "MIT / Apache-2.0"),
    ("uuid", "MIT / Apache-2.0"),
    ("rustix", "Apache-2.0 / MIT"),
];

const BUNDLED_FONTS: [(&str, &str); 2] = [
    ("Cantarell (The Cantarell Project)", "SIL OFL 1.1"),
    ("Monaspace Argon (GitHub)", "SIL OFL 1.1"),
];

#[derive(Debug, Clone, Copy)]
pub(crate) enum Action {
    Open,
    Close,
    OpenGithub,
    GithubOpened(bool),
}

pub(crate) struct State {
    pending: Option<iced::task::Handle>,
    error: Option<&'static str>,
    return_focus: Option<ViewId>,
}

impl Drop for State {
    fn drop(&mut self) {
        if let Some(handle) = self.pending.take() {
            handle.abort();
        }
    }
}

impl Desktop {
    pub(crate) fn can_open_about(&self) -> bool {
        let recovery_blocking = self.storage_details && self.application.storage_detail().is_some();
        self.application.accepts_edits()
            && self.native_menu.error().is_none()
            && !recovery_blocking
            && self.about.is_none()
            && self.workspace_management.is_none()
            && !self.settings_picker.pending()
            && !self.context_menu_open()
    }

    pub(crate) fn about_message(&mut self, action: Action) -> Task<Message> {
        match action {
            Action::Open => {
                if !self.can_open_about() {
                    return Task::none();
                }
                self.clear_context_menus();
                if let Some(draft) = &mut self.app_settings {
                    draft.close_transient_menus();
                }
                self.about = Some(State {
                    pending: None,
                    error: None,
                    return_focus: if self.app_settings.is_some() {
                        None
                    } else {
                        self.focused_path.take()
                    },
                });
                self.focus_version = self.focus_version.saturating_add(1);
                self.pointer = None;
                for presentation in self.views.values_mut() {
                    presentation.hovered = None;
                }
                operation::focus(CLOSE_ID)
            }
            Action::Close => self.close_about(),
            Action::OpenGithub => self.open_about_github(),
            Action::GithubOpened(success) => {
                if let Some(state) = &mut self.about {
                    state.pending = None;
                    state.error = (!success).then_some(
                        "Cannot open GitHub in your default browser. Visit https://github.com/MRKups/file-tile directly.",
                    );
                }
                Task::none()
            }
        }
    }

    fn close_about(&mut self) -> Task<Message> {
        let Some(state) = self.about.take() else {
            return Task::none();
        };
        self.focus_version = self.focus_version.saturating_add(1);
        if let Some(draft) = &self.app_settings {
            return draft.restore_focus_after_overlay();
        }
        state
            .return_focus
            .filter(|view| self.file_view_input_enabled(*view))
            .map_or_else(Task::none, |view| {
                self.focused_path = Some(view);
                operation::focus(navigation_header::path_id(view))
            })
    }

    fn open_about_github(&mut self) -> Task<Message> {
        let Some(state) = &mut self.about else {
            return Task::none();
        };
        if state.pending.is_some() {
            return Task::none();
        }
        let Some(workers) = &self.open_workers else {
            state.error = Some(
                "Cannot open GitHub: the link-opening service is unavailable. Visit https://github.com/MRKups/file-tile directly.",
            );
            return Task::none();
        };
        state.error = None;
        let (task, handle) = Task::perform(workers.open_url(GITHUB_URL), |result| {
            Message::About(Action::GithubOpened(result.is_ok()))
        })
        .abortable();
        state.pending = Some(handle);
        task
    }

    pub(crate) fn about_input(&mut self, event: Event) -> Task<Message> {
        if let Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modifiers,
            repeat: false,
            ..
        }) = event
        {
            match key {
                keyboard::Key::Named(keyboard::key::Named::Escape) => {
                    return self.close_about();
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

pub(crate) fn view<'a>(
    state: &'a State,
    updates: &'a crate::dialogs::updates::Updates,
    size: Size,
    palette: a::Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    let header = row![
        svg(app_icon())
            .width(52.0)
            .height(52.0)
            .content_fit(iced::ContentFit::Contain)
            .style(|_, _| svg::Style { color: None }),
        column![
            ty.title("file-tile").color(palette.text),
            ty.control(format!(
                "Version {} ({})",
                crate::VERSION,
                crate::GIT_COMMIT
            ))
            .color(palette.muted),
        ]
        .spacing(3),
    ]
    .spacing(14)
    .align_y(iced::Center);

    let mut body = column![
        column![
            detail_row("Author", "Michał Kupś", palette),
            detail_row("License", "MIT License", palette),
        ]
        .spacing(6),
        rule::horizontal(1).style(move |_| palette.rule()),
        credits_section(
            "Acknowledgments",
            Some(ACKNOWLEDGMENTS_NOTE),
            &ACKNOWLEDGMENTS,
            palette,
        ),
        rule::horizontal(1).style(move |_| palette.rule()),
        credits_section("Bundled fonts", None, &BUNDLED_FONTS, palette),
    ]
    .spacing(16);
    if let Some(error) = state.error {
        body = body.push(ty.caption(error).color(palette.error));
    }

    let inset = Padding {
        right: 12.0,
        ..Padding::ZERO
    };
    let scroll = a::scroll_area(container(body).padding(Padding {
        top: 2.0,
        right: 28.0,
        bottom: 4.0,
        left: 0.0,
    }))
    .direction(scrollable::Direction::Vertical(a::scrollbar()))
    .style(palette.scrollable())
    .width(Fill)
    .height(Fill);

    let footer = container(
        row![
            github_button(palette),
            updates_button(updates, palette),
            space().width(Fill),
            close_button(palette),
        ]
        .spacing(10)
        .align_y(iced::Center),
    )
    .padding(inset);

    let panel = column![
        container(header).padding(inset),
        container(rule::horizontal(1).style(move |_| palette.rule())).padding(inset),
        scroll,
        container(rule::horizontal(1).style(move |_| palette.rule())).padding(inset),
        footer,
    ]
    .spacing(14);

    container(
        container(panel)
            .padding(Padding {
                top: 20.0,
                right: 8.0,
                bottom: 20.0,
                left: 20.0,
            })
            .width((size.width - 32.0).clamp(1.0, 480.0))
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

fn detail_row<'a>(label: &'a str, value: &'a str, palette: a::Palette) -> Element<'a, Message> {
    let ty = palette.typography;
    row![
        ty.control(label).color(palette.muted).width(84),
        ty.control(value).color(palette.text).width(Fill),
    ]
    .spacing(12)
    .into()
}

fn credits_section(
    title: &'static str,
    note: Option<&'static str>,
    entries: &'static [(&'static str, &'static str)],
    palette: a::Palette,
) -> Element<'static, Message> {
    let ty = palette.typography;
    let heading = ty.heading(title).color(palette.text);
    let mut rows = column![].spacing(7);
    for (name, license) in entries {
        rows = rows.push(
            row![
                ty.text(*name).color(palette.text).width(Fill),
                ty.text(*license).color(palette.muted),
            ]
            .spacing(12)
            .align_y(iced::Center),
        );
    }
    if let Some(note) = note {
        column![
            column![heading, ty.text(note).color(palette.muted)].spacing(6),
            rows,
        ]
        .spacing(16)
        .into()
    } else {
        column![heading, rows].spacing(10).into()
    }
}

fn github_button(palette: a::Palette) -> Element<'static, Message> {
    let ty = palette.typography;
    let action = Message::About(Action::OpenGithub);
    let face = row![
        icons::view(Icon::Github, 15.0, Some(palette.text), 1.0),
        ty.control("GitHub"),
    ]
    .spacing(7)
    .align_y(iced::Center);
    let control = button(face)
        .padding([7, 12])
        .on_press(action.clone())
        .style(move |_, status| palette.inspector_button(status, false));
    let control = focus_control::themed(GITHUB_ID, action, control, palette);
    tooltip(control, ty.small(GITHUB_URL), tooltip::Position::Top)
        .delay(Duration::from_millis(250))
        .gap(5)
        .padding(7)
        .style(move |_| palette.tooltip())
        .into()
}

fn updates_button(
    updates: &crate::dialogs::updates::Updates,
    palette: a::Palette,
) -> Element<'static, Message> {
    let ty = palette.typography;
    let action = updates
        .can_check()
        .then_some(Message::Updates(crate::dialogs::updates::Action::CheckNow));
    let control = button(ty.control("Check for Updates"))
        .padding([7, 12])
        .on_press_maybe(action.clone())
        .style(move |_, status| palette.inspector_button(status, false));
    let control = focus_control::themed_optional(UPDATES_ID, action, control, palette);
    if let Some(note) = updates.status_note() {
        tooltip(control, ty.small(note.to_owned()), tooltip::Position::Top)
            .delay(Duration::from_millis(250))
            .gap(5)
            .padding(7)
            .style(move |_| palette.tooltip())
            .into()
    } else {
        control
    }
}

fn close_button(palette: a::Palette) -> Element<'static, Message> {
    let ty = palette.typography;
    let action = Message::About(Action::Close);
    let control = button(ty.control("Close"))
        .padding([7, 14])
        .on_press(action.clone())
        .style(move |_, status| palette.inspector_button(status, false));
    focus_control::themed(CLOSE_ID, action, control, palette)
}

fn app_icon() -> svg::Handle {
    static HANDLE: OnceLock<svg::Handle> = OnceLock::new();
    HANDLE
        .get_or_init(|| svg::Handle::from_memory(APP_ICON_SVG))
        .clone()
}
