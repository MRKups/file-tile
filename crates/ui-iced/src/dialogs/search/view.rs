//! Minimalist, high-density `Cmd+F` floating Search card and zero-polling debounce driver.

use std::time::Instant;

use iced::advanced::widget::{
    Id, Operation, Tree,
    operation::{Outcome, Scrollable, scrollable as scroll_op},
};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, renderer};
use iced::border::Border;
use iced::widget::{
    button, column, container, mouse_area, opaque, row, rule, space, stack, text, text_input,
};
use iced::{
    Center, Element, Event, Fill, Length, Padding, Rectangle, Renderer, Size, Theme, Vector, mouse,
    window,
};

use super::{Action, EXPAND_RESULTS_STEP, INPUT_ID, State};
use crate::Message;
use crate::application::Application;
use crate::application::files::search::{
    SearchResult, SystemSearchStatus, meets_system_query_minimum,
};
use crate::window::appearance as a;
use crate::window::icons::{self, Icon};

const LIST_SCROLL_ID: &str = "global-search-scroll";
const SELECTED_ROW_ID: &str = "global-search-selected";

pub(super) fn reset_scroll() -> iced::Task<Message> {
    iced::advanced::widget::operate(scroll_op::scroll_to(
        LIST_SCROLL_ID.into(),
        scroll_op::AbsoluteOffset {
            x: None,
            y: Some(0.0),
        },
    ))
}

pub(super) fn reveal_selected() -> iced::Task<Message> {
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

pub(crate) fn view<'a>(
    application: &'a Application,
    state: &'a State,
    size: Size,
    palette: a::Palette,
) -> Element<'a, Message> {
    let width = (size.width - 32.0).clamp(280.0, 620.0);
    let top_inset = (size.height * 0.12).round().clamp(16.0, 84.0);
    let max_height = (size.height - top_inset - 24.0).clamp(200.0, 520.0);
    let ty = palette.typography;

    let mut card_content = column![input_row(state, palette)].spacing(8);
    if !state.hide_open_error
        && let Some(error) = application.search_open_error()
    {
        card_content = card_content.push(ty.small(error).color(palette.error));
    }
    card_content = card_content
        .push(rule::horizontal(1).style(move |_| palette.rule()))
        .push(results_body(application, state, max_height, palette));

    let card = opaque(
        container(card_content.width(Fill))
            .padding(Padding {
                top: 12.0,
                right: 12.0,
                bottom: 10.0,
                left: 12.0,
            })
            .width(width)
            .max_height(max_height)
            .style(move |_| palette.modal_panel()),
    );

    let backdrop = opaque(
        mouse_area(
            container(space())
                .width(Fill)
                .height(Fill)
                .style(|_| container::Style {
                    background: Some(a::MODAL_SCRIM.into()),
                    ..container::Style::default()
                }),
        )
        .on_press(Message::Search(Action::Close)),
    );

    let positioned = container(card)
        .padding(Padding {
            top: top_inset,
            right: 16.0,
            bottom: 16.0,
            left: 16.0,
        })
        .width(Fill)
        .height(Fill)
        .center_x(Fill);

    stack![
        backdrop,
        positioned,
        deadline_driver(state.debounce_deadline)
    ]
    .into()
}

fn input_row<'a>(state: &'a State, palette: a::Palette) -> Element<'a, Message> {
    let ty = palette.typography;
    let field = text_input("Search open tiles and This Mac...", &state.query)
        .id(INPUT_ID)
        .font(ty.font())
        .size(ty.size(a::CONTROL_SIZE))
        .padding([7, 10])
        .width(Fill)
        .on_input(|query| Message::Search(Action::QueryChanged(query)))
        .on_submit(Message::Search(Action::ActivateSelected))
        .style(move |_, status| crate::dialogs::app_settings::style::field(palette, status, false));

    let close = button(icons::view(
        Icon::Close,
        ty.icon(13.0),
        Some(palette.muted),
        1.0,
    ))
    .padding(6)
    .on_press(Message::Search(Action::Close))
    .style(move |_, status| palette.text_button(status));

    row![
        icons::view(Icon::Search, ty.icon(15.0), Some(palette.muted), 0.9),
        field,
        close,
    ]
    .spacing(8)
    .align_y(Center)
    .into()
}

fn results_body<'a>(
    application: &'a Application,
    state: &'a State,
    max_height: f32,
    palette: a::Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    if !state.has_query() {
        return container(
            ty.small("Type a filename to search open tiles and This Mac.")
                .color(palette.muted),
        )
        .padding([6, 4])
        .width(Fill)
        .into();
    }

    let mut list = column![].spacing(4).width(Fill);
    let visible_open = state.visible_open_tiles();
    let total_open = state.open_tiles.results.len();
    let open_count_label =
        section_count_label(visible_open.len(), total_open, state.open_tiles.truncated);
    list = list.push(section_header("OPEN TILES", open_count_label, palette));
    if visible_open.is_empty() {
        list = list.push(empty_status("No matches in open tiles", palette));
    } else {
        for (index, item) in visible_open.iter().enumerate() {
            list = list.push(result_row(
                item,
                index,
                index == state.selected_index,
                palette,
            ));
        }
        list = push_section_footer(
            list,
            visible_open.len(),
            total_open,
            state.open_tiles.truncated,
            Action::ShowMoreOpenTiles,
            palette,
        );
    }

    list = list.push(
        container(rule::horizontal(1).style(move |_| palette.rule()))
            .padding([4, 0])
            .width(Fill),
    );
    list = push_system_section(list, application, state, visible_open.len(), palette);

    let scroller = a::scroll_area(container(list).padding(Padding {
        top: 0.0,
        right: a::SCROLLBAR_WIDTH,
        bottom: 2.0,
        left: 0.0,
    }))
    .id(LIST_SCROLL_ID)
    .width(Fill)
    .height(Length::Shrink)
    .direction(iced::widget::scrollable::Direction::Vertical(a::scrollbar()))
    .style(palette.scrollable());

    container(scroller)
        .width(Fill)
        .max_height((max_height - 96.0).max(120.0))
        .into()
}

fn push_system_section<'a>(
    mut list: iced::widget::Column<'a, Message>,
    application: &'a Application,
    state: &'a State,
    offset: usize,
    palette: a::Palette,
) -> iced::widget::Column<'a, Message> {
    if !meets_system_query_minimum(&state.query) {
        list = list.push(section_header("THIS MAC", String::new(), palette));
        return list.push(empty_status(
            "Type at least 2 characters to search This Mac",
            palette,
        ));
    }
    let debouncing = state.debounce_deadline.is_some();
    let status = application.system_search_status();
    if debouncing
        || matches!(
            status,
            SystemSearchStatus::Searching | SystemSearchStatus::Idle
        )
    {
        list = list.push(section_header(
            "THIS MAC",
            "Searching...".to_owned(),
            palette,
        ));
        return list.push(empty_status("Searching This Mac...", palette));
    }
    match status {
        SystemSearchStatus::Ready { results, truncated } => {
            let visible = state.visible_system_results(application);
            let total = results.len();
            list = list.push(section_header(
                "THIS MAC",
                section_count_label(visible.len(), total, *truncated),
                palette,
            ));
            if visible.is_empty() {
                list.push(empty_status("No matches on This Mac", palette))
            } else {
                for (index, item) in visible.iter().enumerate() {
                    let flat_index = offset + index;
                    list = list.push(result_row(
                        item,
                        flat_index,
                        flat_index == state.selected_index,
                        palette,
                    ));
                }
                push_section_footer(
                    list,
                    visible.len(),
                    total,
                    *truncated,
                    Action::ShowMoreThisMac,
                    palette,
                )
            }
        }
        SystemSearchStatus::Unavailable(message) => {
            list = list.push(section_header("THIS MAC", String::new(), palette));
            list.push(empty_status(message.as_str(), palette))
        }
        SystemSearchStatus::Idle | SystemSearchStatus::Searching => list,
    }
}

fn section_count_label(visible: usize, total: usize, truncated: bool) -> String {
    if visible < total {
        if truncated {
            format!("{visible} of {total}+")
        } else {
            format!("{visible} of {total}")
        }
    } else if truncated {
        format!("{total}+")
    } else {
        total.to_string()
    }
}

fn push_section_footer<'a>(
    list: iced::widget::Column<'a, Message>,
    visible: usize,
    total: usize,
    truncated: bool,
    action: Action,
    palette: a::Palette,
) -> iced::widget::Column<'a, Message> {
    if visible < total {
        let next = (total - visible).min(EXPAND_RESULTS_STEP);
        list.push(show_more_row(format!("Show {next} more"), action, palette))
    } else if truncated {
        list.push(empty_status(
            format!("Showing first {total} matches. Type more to narrow."),
            palette,
        ))
    } else {
        list
    }
}

fn show_more_row<'a>(label: String, action: Action, palette: a::Palette) -> Element<'a, Message> {
    let ty = palette.typography;
    button(container(ty.small(label)).width(Fill).align_x(Center))
        .padding([5, 8])
        .width(Fill)
        .on_press(Message::Search(action))
        .style(move |_, status| show_more_style(palette, status))
        .into()
}

fn show_more_style(palette: a::Palette, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: hovered.then(|| palette.row_hover.into()),
        text_color: if hovered { palette.text } else { palette.muted },
        border: Border {
            radius: 5.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

fn section_header<'a>(
    title: &'static str,
    detail: String,
    palette: a::Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    row![
        ty.caption(title)
            .font(ty.heading_font())
            .color(palette.muted),
        space::horizontal(),
        ty.caption(detail).color(palette.muted),
    ]
    .padding([2, 6])
    .width(Fill)
    .align_y(Center)
    .into()
}

fn empty_status<'a>(
    label: impl text::IntoFragment<'a>,
    palette: a::Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    container(ty.small(label).color(palette.muted))
        .padding([4, 6])
        .width(Fill)
        .into()
}

fn result_row<'a>(
    item: &'a SearchResult,
    flat_index: usize,
    selected: bool,
    palette: a::Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    let icon = Icon::for_path_kind(&item.path, item.kind);
    let leading: Element<'a, Message> =
        icons::view(icon, ty.icon(15.0), Some(palette.icon(icon)), 1.0).into();
    let mut items = row![
        leading,
        container(
            ty.text(&item.name)
                .color(palette.text)
                .wrapping(text::Wrapping::None),
        )
        .width(Fill)
        .clip(true),
        container(
            ty.small(&item.parent_display)
                .color(palette.muted)
                .wrapping(text::Wrapping::None),
        )
        .max_width(240.0)
        .clip(true),
    ]
    .spacing(8)
    .align_y(Center);

    if item.dataless {
        let cloud: Element<'a, Message> =
            icons::view(Icon::Cloud, ty.icon(13.0), Some(palette.muted), 0.78).into();
        items = items.push(cloud);
    }

    let btn = button(items)
        .padding([5, 8])
        .width(Fill)
        .on_press(Message::Search(Action::ActivateIndex(flat_index)))
        .style(move |_, status| row_style(palette, selected, status));

    if selected {
        container(btn).id(SELECTED_ROW_ID).width(Fill).into()
    } else {
        btn.into()
    }
}

fn row_style(palette: a::Palette, selected: bool, status: button::Status) -> button::Style {
    let background = if selected {
        Some(palette.row_selected.into())
    } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        Some(palette.row_hover.into())
    } else {
        None
    };
    button::Style {
        background,
        text_color: palette.text,
        border: Border {
            radius: 5.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

fn deadline_driver<'a>(deadline: Option<Instant>) -> Element<'a, Message> {
    Element::new(DeadlineDriver { deadline })
}

struct DeadlineDriver {
    deadline: Option<Instant>,
}

impl Widget<Message, Theme, Renderer> for DeadlineDriver {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(Size::ZERO)
    }

    fn update(
        &mut self,
        _tree: &mut Tree,
        event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        if let Event::Window(window::Event::RedrawRequested(now)) = event
            && let Some(deadline) = self.deadline
        {
            if *now >= deadline {
                shell.publish(Message::Search(Action::DebounceTick(*now)));
            } else {
                shell.request_redraw_at(deadline);
            }
        }
    }

    fn draw(
        &self,
        _tree: &Tree,
        _renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
    }
}
