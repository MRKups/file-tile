//! View-tagged Range drafts and atomic Start/Stops editing.
use crate::application::{Action, workspace::scopes::ScopeEdit};
use crate::domain::{NavigationRange, ScopeKind, ViewId};
use crate::{Desktop, Message, widgets::focus_control, window::appearance as a};
use iced::widget::{button, column, container, row, scrollable, text_editor, text_input};
use iced::{Element, Fill, Task};
use std::path::{Path, PathBuf};

pub(crate) const SCROLL_ID: &str = "range-page-scroll";
pub(crate) const CANCEL_ID: &str = "range-page-cancel";
pub(crate) const APPLY_ID: &str = "range-page-apply";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Field {
    Start,
    Stops,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Stage {
    #[default]
    Start,
    Stops,
}
pub(crate) struct Draft {
    pub error: Option<String>,
    chosen: Option<NavigationRange>,
    pub start: String,
    pub stops: text_editor::Content,
    pub stage: Stage,
    pub focused: Option<Field>,
    edited: bool,
    submitted: bool,
    baseline: Option<NavigationRange>,
    start_edited: bool,
    stops_edited: bool,
}
impl Draft {
    pub fn new() -> Self {
        Self {
            error: None,
            chosen: None,
            start: String::new(),
            stops: text_editor::Content::new(),
            stage: Stage::Start,
            focused: None,
            edited: false,
            submitted: false,
            baseline: None,
            start_edited: false,
            stops_edited: false,
        }
    }
    pub fn sync(&mut self, range: Option<&NavigationRange>, pending: bool, error: bool) -> bool {
        let applied = self.submitted && !pending && !error;
        if self.submitted && !pending {
            self.submitted = false;
            if !error {
                self.edited = false;
                self.chosen = None;
            }
        }
        if !self.edited && self.baseline.as_ref() != range {
            self.start_edited = false;
            self.stops_edited = false;
            self.start = range.map_or_else(String::new, |range| {
                range.start().to_string_lossy().into_owned()
            });
            let text = range.map_or_else(String::new, |range| {
                range
                    .stops()
                    .iter()
                    .map(|path| path.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("\n")
            });
            self.stops = text_editor::Content::with_text(&text);
            self.baseline = range.cloned();
        }
        applied
    }

    fn paths(&self, current: &Path, home: &Path) -> Result<(PathBuf, Vec<PathBuf>), String> {
        let base = self
            .chosen
            .as_ref()
            .or(self.baseline.as_ref())
            .ok_or("Choose an Origin first.")?;
        let expand = |text: &str| {
            crate::domain::expand_path(text, current, home).map_err(|error| error.to_string())
        };
        let start = if self.start_edited {
            expand(&self.start)?
        } else {
            base.start().into()
        };
        let stops = if self.stops_edited {
            let text = self.stops.text();
            let lines: Vec<_> = text
                .lines()
                .filter(|line| !line.trim().is_empty())
                .collect();
            if lines.is_empty() || lines.len() > NavigationRange::MAX_STOPS {
                return Err("Enter 1 to 64 range ends, one per line.".into());
            }
            lines
                .into_iter()
                .map(expand)
                .collect::<Result<Vec<_>, _>>()?
        } else {
            base.stops().to_vec()
        };
        Ok((start, stops))
    }

    pub fn preview(&self, current: &Path, home: &Path) -> Option<NavigationRange> {
        let (start, stops) = self.paths(current, home).ok()?;
        NavigationRange::new(start, stops).ok()
    }

    pub fn suspend(&mut self) {
        self.submitted = false;
        self.focused = None;
    }

    fn choose(&mut self, range: NavigationRange) {
        self.start = range.start().to_string_lossy().into_owned();
        self.stops = text_editor::Content::with_text(
            &range
                .stops()
                .iter()
                .map(|path| path.to_string_lossy())
                .collect::<Vec<_>>()
                .join("\n"),
        );
        self.chosen = Some(range);
        self.start_edited = false;
        self.stops_edited = false;
        self.edited = true;
        self.submitted = false;
        self.error = None;
        self.focused = None;
    }
}
pub fn start_id(view: ViewId) -> String {
    format!("range-start-{}", view.0)
}
pub fn stops_id(view: ViewId) -> String {
    format!("range-stops-{}", view.0)
}

impl Desktop {
    pub(crate) fn range_message(&mut self, message: Message) -> Task<Message> {
        if matches!(message, Message::TreeChosen(..) | Message::TreeScrolled(..)) {
            return self.tree_message(message);
        }
        if !self.workspace_input_enabled() {
            return Task::none();
        }
        let view = match &message {
            Message::RangeStartChanged(view, _)
            | Message::RangeStopsChanged(view, _)
            | Message::CommitRange(view)
            | Message::CancelRange(view)
            | Message::RangeStage(view, _) => *view,
            _ => return Task::none(),
        };
        let Some(draft) = self
            .box_settings
            .as_mut()
            .filter(|draft| draft.view == view && draft.range_page)
        else {
            return Task::none();
        };
        match message {
            Message::CancelRange(_) => self.cancel_range_page(view),
            Message::RangeStartChanged(_, text) => {
                draft.range.start = text;
                draft.range.error = None;
                draft.range.edited = true;
                draft.range.start_edited = true;
                draft.range.submitted = false;
                draft.range.focused = Some(Field::Start);
                self.application.cancel_scope_edit();
                self.retain_configuration_reads();
                Task::none()
            }
            Message::RangeStopsChanged(_, action) => {
                let editing = action.is_edit();
                draft.range.stops.perform(action);
                draft.range.focused = Some(Field::Stops);
                if editing {
                    draft.range.error = None;
                    draft.range.edited = true;
                    draft.range.stops_edited = true;
                    draft.range.submitted = false;
                    self.application.cancel_scope_edit();
                    self.retain_configuration_reads();
                }
                Task::none()
            }
            Message::RangeStage(_, stage) => {
                draft.range.stage = stage;
                Task::none()
            }
            Message::CommitRange(_) => self.commit_range(),
            _ => Task::none(),
        }
    }
    pub(crate) fn commit_range(&mut self) -> Task<Message> {
        let Some(draft) = &mut self.box_settings else {
            return Task::none();
        };
        if !draft.range_page || draft.range.submitted {
            return Task::none();
        }
        let view = draft.view;
        if !draft.range.edited {
            return self.cancel_range_page(view);
        }
        let Some(item) = self.application.workspace().view(view) else {
            return Task::none();
        };
        let (start, stops) = match draft
            .range
            .paths(item.navigation.current(), self.application.home())
        {
            Ok(paths) => paths,
            Err(error) => {
                draft.range.error = Some(error);
                return Task::none();
            }
        };
        draft.range.submitted = true;
        draft.range.error = None;
        self.dispatch(Action::EditScope(
            view,
            ScopeEdit::NativeBoundaries { start, stops },
        ))
    }
    pub(crate) fn tree_choice(&mut self, view: ViewId, path: std::path::PathBuf) -> Task<Message> {
        if !self.workspace_input_enabled()
            || !self
                .box_settings
                .as_ref()
                .is_some_and(|draft| draft.view == view && draft.range_page)
        {
            return Task::none();
        }
        let range = self
            .application
            .workspace()
            .view(view)
            .is_some_and(|item| item.navigation.scope().kind() == ScopeKind::Range);
        if range {
            let Some(draft) = &mut self.box_settings else {
                return Task::none();
            };
            let Some(item) = self.application.workspace().view(view) else {
                return Task::none();
            };
            let origin_stage = draft.range.stage == Stage::Start;
            let range = match draft.range.stage {
                Stage::Start => {
                    draft.range.stage = Stage::Stops;
                    NavigationRange::new(path.clone(), vec![path.clone()])
                }
                Stage::Stops => draft
                    .range
                    .preview(item.navigation.current(), self.application.home())
                    .ok_or("Correct Origin and range ends before using the tree.")
                    .and_then(|range| range.toggle(path.clone())),
            };
            match range {
                Ok(range) => draft.range.choose(range),
                Err(error) => {
                    draft.range.error = Some(error.into());
                    draft.range.submitted = false;
                }
            }
            self.application.cancel_scope_edit();
            self.retain_configuration_reads();
            if origin_stage
                && let Some(tree_path) = self.application.folder_tree().and_then(|tree| {
                    tree.nodes
                        .iter()
                        .find(|(_, node)| node.target == path && !node.expanded)
                        .map(|(path, _)| path.clone())
                })
            {
                return self.dispatch(Action::ToggleTree(view, tree_path));
            }
            Task::none()
        } else {
            Task::none()
        }
    }
}

pub(crate) fn page<'a>(
    desktop: &'a Desktop,
    draft: &'a crate::dialogs::box_settings::Draft,
) -> Element<'a, Message> {
    let palette = desktop.configure_palette();
    let ty = palette.typography;
    let pending = desktop.application.scope_pending(draft.view);
    let mut contents = column![
        ty.heading("Choose range"),
        ty.text("Set an origin, then choose the ends of the branches this view may browse.")
            .color(palette.muted),
        crate::dialogs::folder_tree::view(desktop, draft)
    ]
    .spacing(16);
    for error in [
        draft.range.error.as_deref(),
        desktop.application.scope_error(draft.view),
    ]
    .into_iter()
    .flatten()
    {
        contents = contents.push(ty.caption(error).color(palette.error));
    }
    let cancel = Message::CancelRange(draft.view);
    let apply = (!pending).then_some(Message::CommitRange(draft.view));
    column![
        a::scroll_area(container(contents).padding(iced::Padding {
            left: 26.0,
            right: 30.0,
            ..iced::Padding::ZERO
        }))
        .id(SCROLL_ID)
        .height(Fill)
        .direction(scrollable::Direction::Vertical(a::scrollbar()))
        .style(palette.scrollable()),
        container(
            row![
                ty.caption(if pending { "Checking range…" } else { "" })
                    .color(palette.muted)
                    .width(Fill),
                focus_control::themed(
                    CANCEL_ID,
                    cancel.clone(),
                    button(ty.control("Cancel"))
                        .padding([8, 16])
                        .style(move |_, status| palette.inspector_button(status, false))
                        .on_press(cancel),
                    palette
                ),
                focus_control::themed_optional(
                    APPLY_ID,
                    apply.clone(),
                    button(ty.control("Apply"))
                        .padding([8, 16])
                        .style(move |_, status| palette.inspector_button(status, true))
                        .on_press_maybe(apply),
                    palette
                )
            ]
            .spacing(10)
            .align_y(iced::Center)
        )
        .padding(iced::Padding {
            top: 12.0,
            left: 26.0,
            right: 26.0,
            ..iced::Padding::ZERO
        })
    ]
    .into()
}

pub(crate) fn view<'a>(
    desktop: &'a Desktop,
    draft: &'a crate::dialogs::box_settings::Draft,
) -> Element<'a, Message> {
    let palette = desktop.configure_palette();
    let ty = palette.typography;
    let view = draft.view;
    let invalid = draft.range.error.is_some() || desktop.application.scope_error(view).is_some();
    column![
        ty.control("● Origin").color(palette.accent),
        text_input("Origin path", &draft.range.start)
            .id(start_id(view))
            .size(ty.size(a::CONTROL_SIZE))
            .padding([9, 12])
            .font(ty.font())
            .on_input(move |text| Message::RangeStartChanged(view, text))
            .style(move |_, status| palette.inspector_field(status, invalid)),
        ty.control("■ Range ends").color(palette.text),
        text_editor(&draft.range.stops)
            .id(stops_id(view))
            .height(96)
            .size(ty.size(a::CONTROL_SIZE))
            .font(ty.font())
            .padding(9)
            .on_action(move |action| Message::RangeStopsChanged(view, action))
            .style(move |_, status| {
                let field = palette.inspector_field(
                    if matches!(status, text_editor::Status::Focused { .. }) {
                        iced::widget::text_input::Status::Focused { is_hovered: false }
                    } else {
                        iced::widget::text_input::Status::Active
                    },
                    invalid,
                );
                text_editor::Style {
                    background: field.background,
                    border: field.border,
                    placeholder: field.placeholder,
                    value: field.value,
                    selection: field.selection,
                }
            }),
        ty.caption("One range end per line.").color(palette.muted),
    ]
    .spacing(7)
    .into()
}
pub(crate) fn stages(view: ViewId, stage: Stage, palette: a::Palette) -> Element<'static, Message> {
    let ty = palette.typography;
    let control = |label: &'static str, target| {
        let action = Message::RangeStage(view, target);
        focus_control::themed(
            format!("range-stage-{target:?}"),
            action.clone(),
            button(ty.control(label).color(if target == Stage::Start {
                palette.accent
            } else {
                palette.text
            }))
            .on_press(action)
            .padding([8, 12])
            .style(move |_, status| {
                let color = if target == Stage::Start {
                    palette.accent
                } else {
                    palette.text
                };
                let active = target == stage;
                button::Style {
                    text_color: color,
                    background: (active
                        || matches!(status, button::Status::Hovered | button::Status::Pressed))
                    .then_some(iced::Color { a: 0.12, ..color }.into()),
                    border: iced::Border {
                        radius: 6.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            }),
            palette,
        )
    };
    container(
        row![
            control("● Origin", Stage::Start),
            control("■ Range ends", Stage::Stops)
        ]
        .spacing(8),
    )
    .width(Fill)
    .into()
}
