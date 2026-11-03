//! Swapped modal view for searching, comparing, and tuning system and standard fonts.
use super::{font_picker::display_label, layout, style, typography};
use crate::application::preferences::typography::{
    TextSize, TextWeight, Typeface, TypographyPreferences,
};
use crate::{
    Message,
    widgets::focus_control,
    window::{appearance as a, system_font::SystemFont, typography::Style},
};
use iced::advanced::widget::{
    Id, Operation,
    operation::{Outcome, Scrollable, scrollable as scroll_op},
};
use iced::widget::{
    button, column, container, row, rule, scrollable, space, text::Wrapping, text_input,
};
use iced::{Border, Center, Element, Fill, Rectangle, Size, Task, Vector};

pub(crate) const SEARCH_ID: &str = "font-explorer-search";
pub(crate) const SIZE_ID: &str = "font-explorer-size";
pub(crate) const WEIGHT_ID: &str = "font-explorer-weight";
pub(crate) const CANCEL_ID: &str = "font-explorer-cancel";
pub(crate) const DONE_ID: &str = "font-explorer-done";
const LIST_SCROLL_ID: &str = "font-explorer-list";
const PREVIEW_SCROLL_ID: &str = "font-explorer-preview";
const SELECTED_ROW_ID: &str = "font-explorer-selected";

const SAMPLE_ROWS: [(&str, &str); 5] = [
    ("application", "--"),
    ("Cargo.toml", "2.4 KB"),
    ("main.rs", "4.1 KB"),
    ("README.md", "3.8 KB"),
    ("file-tile_0.1.0_aarch64.dmg", "14.2 MB"),
];

pub(crate) struct ExplorerDraft {
    pub(super) query: String,
    pub(super) typography: TypographyPreferences,
}

impl ExplorerDraft {
    pub(super) fn new(typography: TypographyPreferences) -> Self {
        Self {
            query: String::new(),
            typography,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum FontExplorerEdit {
    QueryChanged(String),
    Step(isize),
    SubmitSearch,
    SelectTypeface(Typeface),
    SizeChanged(TextSize),
    WeightChanged(TextWeight),
    Cancel,
    Done,
}

fn send(edit: FontExplorerEdit) -> Message {
    Message::AppFontExplorer(edit)
}

pub(super) fn first_match(query: &str, system_font: &SystemFont) -> Option<Typeface> {
    let (standard, system) = filtered_choices(query, system_font);
    standard
        .first()
        .map(|(face, _)| *face)
        .or_else(|| system.first().copied())
}

pub(super) fn contains_match(current: Typeface, query: &str, system_font: &SystemFont) -> bool {
    let (standard, system) = filtered_choices(query, system_font);
    standard.iter().any(|(face, _)| *face == current) || system.contains(&current)
}

pub(super) fn step_match(
    current: Typeface,
    query: &str,
    delta: isize,
    system_font: &SystemFont,
) -> Option<Typeface> {
    let (standard, system) = filtered_choices(query, system_font);
    let all: Vec<Typeface> = standard
        .into_iter()
        .map(|(face, _)| face)
        .chain(system)
        .collect();
    if all.is_empty() {
        return None;
    }
    let next = if let Some(index) = all.iter().position(|face| *face == current) {
        if delta < 0 {
            index.saturating_sub(1)
        } else {
            (index + 1).min(all.len() - 1)
        }
    } else if delta < 0 {
        all.len() - 1
    } else {
        0
    };
    Some(all[next])
}

pub(super) fn reset_scroll() -> Task<Message> {
    iced::advanced::widget::operate(scroll_op::scroll_to(
        LIST_SCROLL_ID.into(),
        scroll_op::AbsoluteOffset {
            x: None,
            y: Some(0.0),
        },
    ))
}

pub(super) fn reveal_selected() -> Task<Message> {
    #[derive(Default)]
    struct Reveal {
        selected: Option<Rectangle>,
        viewport: Option<(Rectangle, Rectangle, Vector)>,
    }

    impl<T: 'static> Operation<T> for Reveal {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<T>)) {
            operate(self);
        }

        fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
            if id == Some(&Id::new(SELECTED_ROW_ID)) {
                self.selected = Some(bounds.expand(4.0));
            }
        }

        fn scrollable(
            &mut self,
            id: Option<&Id>,
            bounds: Rectangle,
            content: Rectangle,
            translation: Vector,
            _: &mut dyn Scrollable,
        ) {
            if id == Some(&Id::new(LIST_SCROLL_ID)) {
                self.viewport = Some((bounds, content, translation));
            }
        }

        fn finish(&self) -> Outcome<T> {
            let (Some(selected), Some((bounds, content, translation))) =
                (self.selected, self.viewport)
            else {
                return Outcome::None;
            };
            let top = selected.y - translation.y;
            let bottom = top + selected.height;
            let adjustment = if top < bounds.y {
                top - bounds.y
            } else if bottom > bounds.y + bounds.height {
                bottom - bounds.y - bounds.height
            } else {
                return Outcome::None;
            };
            let offset =
                (translation.y + adjustment).clamp(0.0, (content.height - bounds.height).max(0.0));
            Outcome::Chain(Box::new(scroll_op::scroll_to(
                LIST_SCROLL_ID.into(),
                scroll_op::AbsoluteOffset {
                    x: None,
                    y: Some(offset),
                },
            )))
        }
    }

    iced::advanced::widget::operate(Reveal::default())
}

pub(super) fn view<'a>(
    explorer: &'a ExplorerDraft,
    size: Size,
    palette: a::Palette,
    system_font: &'a SystemFont,
) -> Element<'a, Message> {
    let dimensions = layout::panel_size(size);
    let pad = layout::padding(dimensions);
    let list_width = (dimensions.width * 0.27).clamp(196.0, 256.0);
    let right_width = (dimensions.width - 2.0 * f32::from(pad) - list_width - 37.0).max(200.0);
    let ty = palette.typography;
    let body = row![
        font_list_pane(explorer, list_width, palette, system_font),
        rule::vertical(1).style(move |_| palette.rule()),
        detail_pane(explorer, right_width, palette, system_font),
    ]
    .spacing(18)
    .width(Fill)
    .height(Fill);
    let footer = row![
        space().width(Fill),
        action_btn(
            CANCEL_ID,
            "Cancel",
            Some(send(FontExplorerEdit::Cancel)),
            false,
            palette,
        ),
        action_btn(
            DONE_ID,
            "Done",
            Some(send(FontExplorerEdit::Done)),
            true,
            palette,
        ),
    ]
    .spacing(10)
    .align_y(Center);
    let panel = column![
        ty.title("Font Explorer").color(palette.text),
        rule::horizontal(1).style(move |_| palette.rule()),
        body,
        rule::horizontal(1).style(move |_| palette.rule()),
        footer,
    ]
    .spacing(16);
    container(
        container(panel)
            .padding(pad)
            .width(dimensions.width)
            .height(dimensions.height)
            .style(move |_| style::panel(palette)),
    )
    .padding(layout::MARGIN)
    .center_x(Fill)
    .center_y(Fill)
    .style(|_| container::Style {
        background: Some(a::MODAL_SCRIM.into()),
        ..container::Style::default()
    })
    .into()
}

fn filtered_choices(
    query: &str,
    system_font: &SystemFont,
) -> (Vec<(Typeface, String)>, Vec<Typeface>) {
    let needle = query.trim().to_lowercase();
    let standard = Typeface::STANDARD
        .into_iter()
        .filter_map(|face| {
            let label = display_label(face, system_font);
            let matches = needle.is_empty()
                || label.to_lowercase().contains(&needle)
                || face.name().to_lowercase().contains(&needle);
            matches.then_some((face, label))
        })
        .collect();
    let system = system_font
        .installed
        .iter()
        .filter_map(|entry| {
            let matches = needle.is_empty() || entry.font.name().to_lowercase().contains(&needle);
            matches.then_some(Typeface::Custom(entry.font))
        })
        .collect();
    (standard, system)
}

fn font_list_pane<'a>(
    explorer: &'a ExplorerDraft,
    width: f32,
    palette: a::Palette,
    system_font: &'a SystemFont,
) -> Element<'a, Message> {
    let ty = palette.typography;
    let search = text_input("Search fonts...", &explorer.query)
        .id(SEARCH_ID)
        .on_input(|next| send(FontExplorerEdit::QueryChanged(next)))
        .on_submit(send(FontExplorerEdit::SubmitSearch))
        .padding([8, 12])
        .font(ty.font())
        .size(ty.size(a::CONTROL_SIZE))
        .style(move |_, status| style::field(palette, status, false));
    let current = explorer.typography.typeface;
    let (standard, system) = filtered_choices(&explorer.query, system_font);
    let has_standard = !standard.is_empty();
    let has_system = !system.is_empty();
    let mut items = column![].spacing(2).width(Fill);
    if has_standard {
        items = items.push(section_header("Standard", palette));
        for (face, label) in standard {
            items = items.push(font_row(face, label, face == current, palette));
        }
    }
    if has_standard && has_system {
        items = items
            .push(container(rule::horizontal(1).style(move |_| palette.rule())).padding([4, 6]));
    }
    if has_system {
        items = items.push(section_header("System", palette));
        for face in system {
            items = items.push(font_row(face, face.to_string(), face == current, palette));
        }
    }
    if !has_standard && !has_system {
        items = items
            .push(container(ty.small("No matching fonts").color(palette.muted)).padding([8, 10]));
    }
    let scroll = a::scroll_area(container(items).padding(iced::Padding {
        right: 10.0,
        ..iced::Padding::ZERO
    }))
    .id(LIST_SCROLL_ID)
    .style(palette.scrollable())
    .direction(scrollable::Direction::Vertical(a::scrollbar()))
    .width(Fill)
    .height(Fill);
    column![search, scroll]
        .spacing(10)
        .width(width)
        .height(Fill)
        .into()
}

fn section_header(title: &'static str, palette: a::Palette) -> Element<'static, Message> {
    let ty = palette.typography;
    container(
        ty.caption(title)
            .font(ty.heading_font())
            .color(palette.muted),
    )
    .padding([6, 10])
    .into()
}

fn font_row(
    face: Typeface,
    label: String,
    selected: bool,
    palette: a::Palette,
) -> Element<'static, Message> {
    let ty = palette.typography;
    let row_button = button(
        ty.control(label)
            .color(palette.text)
            .wrapping(Wrapping::None)
            .width(Fill),
    )
    .padding([7, 10])
    .width(Fill)
    .on_press(send(FontExplorerEdit::SelectTypeface(face)))
    .style(move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: if selected {
                Some(palette.row_selected.into())
            } else if hovered {
                Some(palette.row_hover.into())
            } else {
                None
            },
            text_color: palette.text,
            border: Border {
                radius: 6.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    });
    if selected {
        container(row_button).id(SELECTED_ROW_ID).width(Fill).into()
    } else {
        row_button.into()
    }
}

fn detail_pane<'a>(
    explorer: &'a ExplorerDraft,
    right_width: f32,
    palette: a::Palette,
    system_font: &'a SystemFont,
) -> Element<'a, Message> {
    let ty = palette.typography;
    let prefs = explorer.typography;
    let size_field = row![
        ty.control("Text size").color(palette.muted),
        container(typography::size_control(
            SIZE_ID,
            prefs.size,
            |size| send(FontExplorerEdit::SizeChanged(size)),
            palette,
        ))
        .width(196),
    ]
    .spacing(12)
    .align_y(Center);
    let weight_field = row![
        ty.control("Weight").color(palette.muted),
        container(typography::weight_control(
            WEIGHT_ID,
            system_font.weights_for(prefs.typeface),
            prefs.weight,
            |weight| send(FontExplorerEdit::WeightChanged(weight)),
            palette,
        ))
        .width(156),
    ]
    .spacing(12)
    .align_y(Center);
    let controls = row![size_field, weight_field]
        .spacing(28)
        .align_y(Center)
        .wrap()
        .vertical_spacing(10);
    let mut header_block = column![controls].spacing(8);
    if let Some(notice) = typography::fallback_notice(prefs.typeface, system_font) {
        header_block = header_block.push(ty.caption(notice).color(palette.error));
    }
    let current_col = preview_column("Current", ty, palette, system_font);
    let selected_col = preview_column("Selected", Style(prefs), palette, system_font);
    let comparison: Element<'static, Message> = if right_width >= 440.0 {
        row![
            current_col,
            rule::vertical(1).style(move |_| palette.rule()),
            selected_col,
        ]
        .spacing(20)
        .width(Fill)
        .into()
    } else {
        column![
            current_col,
            rule::horizontal(1).style(move |_| palette.rule()),
            selected_col,
        ]
        .spacing(20)
        .width(Fill)
        .into()
    };
    let preview_scroll = a::scroll_area(container(comparison).padding(iced::Padding {
        top: 2.0,
        right: 12.0,
        bottom: 8.0,
        left: 2.0,
    }))
    .id(PREVIEW_SCROLL_ID)
    .style(palette.scrollable())
    .direction(scrollable::Direction::Vertical(a::scrollbar()))
    .width(Fill)
    .height(Fill);
    column![
        header_block,
        rule::horizontal(1).style(move |_| palette.rule()),
        preview_scroll,
    ]
    .spacing(14)
    .width(Fill)
    .height(Fill)
    .into()
}

fn preview_column(
    label: &'static str,
    sample: Style,
    palette: a::Palette,
    system_font: &SystemFont,
) -> Element<'static, Message> {
    let chrome = palette.typography;
    let summary = format!(
        "{} · {} pt · {}",
        display_label(sample.0.typeface, system_font),
        sample.0.size.get(),
        sample.0.weight,
    );
    let header = column![
        chrome
            .caption(label)
            .font(chrome.heading_font())
            .color(palette.muted),
        chrome.small(summary).color(palette.text),
    ]
    .spacing(2);
    let headings = column![
        sample
            .title("Workspace")
            .color(palette.text)
            .wrapping(Wrapping::None),
        sample
            .heading("Projects")
            .color(palette.text)
            .wrapping(Wrapping::None),
        sample
            .small("~/Projects/file-tile/src")
            .color(palette.path_text)
            .wrapping(Wrapping::None),
    ]
    .spacing(4);
    let mut files = column![].spacing(6).width(Fill);
    for (name, meta) in SAMPLE_ROWS {
        files = files.push(
            row![
                sample
                    .text(name)
                    .color(palette.text)
                    .wrapping(Wrapping::None)
                    .width(Fill),
                sample
                    .small(meta)
                    .color(palette.muted)
                    .wrapping(Wrapping::None),
            ]
            .spacing(12)
            .align_y(Center),
        );
    }
    let controls_and_chars = column![
        sample
            .control("Starting folder for new boxes")
            .color(palette.text)
            .wrapping(Wrapping::None),
        sample
            .caption("Modified Today, 18:42 · 5 items")
            .color(palette.muted)
            .wrapping(Wrapping::None),
        sample
            .small("ABCDEFGHIJKLM  abcdefghijklm")
            .color(palette.muted)
            .wrapping(Wrapping::None),
        sample
            .small("0123456789  Il1 O0  / : . - _ ~")
            .color(palette.muted)
            .wrapping(Wrapping::None),
    ]
    .spacing(4);
    container(
        column![
            header,
            rule::horizontal(1).style(move |_| palette.rule()),
            headings,
            rule::horizontal(1).style(move |_| palette.rule()),
            files,
            rule::horizontal(1).style(move |_| palette.rule()),
            controls_and_chars,
        ]
        .spacing(12),
    )
    .width(Fill)
    .clip(true)
    .into()
}

fn action_btn(
    id: &'static str,
    label: &'static str,
    message: Option<Message>,
    primary: bool,
    palette: a::Palette,
) -> Element<'static, Message> {
    focus_control::themed_optional(
        id,
        message.clone(),
        button(palette.typography.control(label))
            .padding([7, 14])
            .style(move |_, status| style::button_style(palette, status, primary, false))
            .on_press_maybe(message),
        palette,
    )
}
