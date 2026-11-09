//! Two-column Miller hierarchy view: current folder on the left, live subfolder preview or file inspector on the right.

mod inspector;
mod preview;

use std::path::{Path, PathBuf};

use iced::widget::{
    Stack, button, column, container, opaque, pin, row, rule, scrollable, text, tooltip,
};
use iced::{Border, Element, Fill, Length, Task};

use crate::application::contracts::directory::{DirectoryEntry, EntryKind};
use crate::domain::{FileView, Navigation, ViewId};
use crate::{
    Action, Desktop, Message, Navigate,
    boxes::file_list,
    widgets::focus_control,
    window::{
        appearance as a,
        icons::{self, Icon},
        typography,
    },
};

pub(crate) use preview::{ColumnFocus, Runtime, ViewPreview};

pub(crate) const EARLIER_MENU_ID: &str = "column-earlier-menu";
pub(crate) const TWO_COLUMN_MIN_WIDTH: f32 = 300.0;

pub(crate) fn header_height(ty: typography::Style) -> f32 {
    ty.row(28.0)
}

impl Desktop {
    pub(crate) fn column_message(&mut self, message: Message) -> Task<Message> {
        if let Message::ColumnPreviewCompleted(completion) = message {
            return self.complete_column_preview(completion);
        }
        if !self.workspace_input_enabled() {
            return Task::none();
        }
        match message {
            Message::ColumnNavigate(view, path) => self.navigate_column_ancestor(view, path),
            Message::EarlierAncestors(view) => self.toggle_earlier_ancestors(view),
            Message::ColumnAdvance(view) => self.advance_into_preview(view),
            Message::ColumnPreviewFocused(view) => self.focus_column_preview(view),
            Message::ColumnPreviewSelected(view, path) => {
                self.select_column_preview_entry(view, path)
            }
            Message::ColumnPreviewOpened(view, path) => self.open_column_preview_entry(view, path),
            Message::ColumnPreviewHovered(view, path, hovered) => {
                self.hover_column_preview_entry(view, path, hovered)
            }
            Message::ColumnPreviewScrolled(view, generation, offset_y) => {
                self.scroll_column_preview(view, generation, offset_y)
            }
            _ => Task::none(),
        }
    }

    fn navigate_column_ancestor(&mut self, view: ViewId, path: PathBuf) -> Task<Message> {
        if !self.is_browsable_columns_view(view) {
            return Task::none();
        }
        let Some(current) = self
            .application
            .workspace()
            .view(view)
            .filter(|item| {
                item.navigation
                    .allowed_ancestors()
                    .any(|ancestor| ancestor == path)
            })
            .map(|item| item.navigation.current().to_path_buf())
        else {
            return Task::none();
        };
        let branch_child = current
            .strip_prefix(&path)
            .ok()
            .and_then(|rel| rel.components().next())
            .map(|comp| path.join(comp));
        self.type_jump.dismiss_session();
        self.activate_view(view);
        let state = self.views.entry(view).or_default();
        state.earlier_ancestors = false;
        state.column_focus = branch_child.map(|target| ColumnFocus {
            directory: path.clone(),
            target,
        });
        self.dispatch(Action::Navigate(view, Navigate::Folder(path)))
    }

    fn toggle_earlier_ancestors(&mut self, view: ViewId) -> Task<Message> {
        if !self.is_browsable_columns_view(view) || !self.can_show_earlier_ancestors(view) {
            return Task::none();
        }
        self.type_jump.dismiss_session();
        self.activate_view(view);
        for (other, state) in &mut self.views {
            if *other != view {
                state.earlier_ancestors = false;
            }
        }
        let state = self.views.entry(view).or_default();
        state.earlier_ancestors = !state.earlier_ancestors;
        Task::none()
    }

    pub(crate) fn can_show_earlier_ancestors(&self, view: ViewId) -> bool {
        if !self.application.workspace().is_active(view) {
            return false;
        }
        let Some(item) = self.application.workspace().box_for_view(view) else {
            return false;
        };
        if item.settings.file_view != FileView::Columns
            || crate::boxes::folder_box::scroller_geometry(self.geometry(), item).compact
        {
            return false;
        }
        self.application
            .workspace()
            .view(view)
            .is_some_and(|v| v.navigation.allowed_ancestors().nth(1).is_some())
    }

    fn is_browsable_columns_view(&self, view: ViewId) -> bool {
        self.application.workspace().can_browse(view)
            && self
                .application
                .workspace()
                .view(view)
                .is_some_and(|item| item.settings.file_view == FileView::Columns)
    }
}

pub(crate) fn view<'a>(
    mut props: file_list::Props<'a>,
    navigation: &'a Navigation,
    earlier: bool,
    column_preview: Option<&'a ViewPreview>,
) -> Element<'a, Message> {
    let palette = props.palette;
    let ty = palette.typography;
    let view = props.view;
    let interactive = props.interactive;
    let total_width = props.width;
    let two_columns = total_width > TWO_COLUMN_MIN_WIDTH;
    let ancestors: Vec<_> = navigation.allowed_ancestors().collect();

    let left_width = if two_columns {
        ((total_width - 1.0) * 0.5).max(1.0)
    } else {
        total_width.max(1.0)
    };
    let clipped_right = props.clipped_left + props.clipped_width;
    props.clipped_width = (clipped_right.min(left_width) - props.clipped_left.max(0.0)).max(0.0);
    props.width = left_width;

    let left_col = column![
        left_header(view, navigation, &ancestors, interactive, palette),
        file_list::view(props)
    ]
    .width(Length::FillPortion(1))
    .height(Fill);

    let layout: Element<'a, Message> = if two_columns {
        let right_col = right_pane(props, column_preview);
        row![
            container(left_col)
                .id(format!("column-current-{}", view.0))
                .width(Length::FillPortion(1))
                .height(Fill),
            rule::vertical(1).style(move |_| palette.rule()),
            right_col
        ]
        .width(Fill)
        .height(Fill)
        .into()
    } else {
        container(left_col)
            .id(format!("column-current-{}", view.0))
            .width(Fill)
            .height(Fill)
            .into()
    };

    let mut layers = Stack::new().push(layout);
    if earlier && ancestors.len() > 1 {
        let menu = container(
            a::scroll_area(earlier_menu(view, &ancestors, interactive, palette))
                .direction(scrollable::Direction::Vertical(a::scrollbar()))
                .style(palette.scrollable())
                .height(ty.row(124.0)),
        )
        .id(EARLIER_MENU_ID)
        .padding(5)
        .width(ty.row(190.0))
        .style(move |_| palette.inspector_panel());
        layers = layers.push(pin(opaque(menu)).x(6.0).y(header_height(ty) + 2.0));
    }

    crate::widgets::input_boundary::view(!interactive, layers.width(Fill).height(Fill))
}

fn left_header<'a>(
    view: ViewId,
    navigation: &'a Navigation,
    ancestors: &[&'a Path],
    interactive: bool,
    palette: a::Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    let back_control: Element<'a, Message> = if let Some(&parent) = ancestors.first() {
        let action = Message::ColumnNavigate(view, parent.to_path_buf());
        let parent_label = folder_name(parent);
        let mut btn = button(
            container(icons::view(
                Icon::Back,
                ty.icon(12.0),
                Some(palette.navigation),
                1.0,
            ))
            .center_x(ty.row(22.0))
            .center_y(ty.row(20.0)),
        )
        .padding(0)
        .width(ty.row(22.0))
        .height(ty.row(20.0))
        .style(move |_, status| header_icon_style(status, palette));
        if interactive {
            btn = btn.on_press(action.clone());
        }
        tooltip(
            focus_control::themed(
                format!("column-parent-{view:?}-{parent:?}"),
                action,
                btn,
                palette,
            ),
            ty.small(format!("Back to {parent_label}")),
            tooltip::Position::Bottom,
        )
        .style(move |_| palette.tooltip())
        .into()
    } else {
        container(icons::view(
            Icon::Back,
            ty.icon(12.0),
            Some(palette.muted),
            0.35,
        ))
        .width(ty.row(22.0))
        .height(ty.row(20.0))
        .center_x(ty.row(22.0))
        .center_y(ty.row(20.0))
        .into()
    };

    let mut bar = row![back_control].spacing(4).align_y(iced::Center);
    if ancestors.len() > 1 {
        let action = Message::EarlierAncestors(view);
        let mut btn = button(
            container(ty.text("…").color(palette.navigation))
                .center_x(ty.row(20.0))
                .center_y(ty.row(20.0)),
        )
        .padding(0)
        .width(ty.row(20.0))
        .height(ty.row(20.0))
        .style(move |_, status| header_icon_style(status, palette));
        if interactive {
            btn = btn.on_press(action.clone());
        }
        bar = bar.push(
            tooltip(
                focus_control::themed(format!("column-earlier-{}", view.0), action, btn, palette),
                ty.small("Parent folders"),
                tooltip::Position::Bottom,
            )
            .style(move |_| palette.tooltip()),
        );
    }
    bar = bar.push(
        ty.small(folder_name(navigation.current()))
            .font(ty.heading_font())
            .color(palette.text)
            .wrapping(text::Wrapping::None)
            .width(Fill),
    );

    column_header_frame(bar.into(), palette)
}

fn right_pane<'a>(
    props: file_list::Props<'a>,
    column_preview: Option<&'a ViewPreview>,
) -> Element<'a, Message> {
    let palette = props.palette;
    let ty = palette.typography;
    let listing = props.load.and_then(crate::application::LoadState::listing);
    let selected = props
        .selected
        .and_then(|sel| listing.and_then(|list| list.entry(sel)));

    let (header, body): (Element<'a, Message>, Element<'a, Message>) = match (listing, selected) {
        (Some(list), Some(entry)) if entry.kind == EntryKind::Directory => {
            let allowed = file_list::entry_allowed(props, list, entry);
            (
                right_folder_header(
                    props.view,
                    entry,
                    allowed,
                    column_preview,
                    props.interactive,
                    palette,
                ),
                preview::folder_preview(props, allowed, column_preview),
            )
        }
        (_, Some(entry)) => (
            right_file_header(entry, palette),
            inspector::file_details(props, entry),
        ),
        _ => (
            column_header_frame(ty.small("Preview").color(palette.muted).into(), palette),
            container(
                ty.small("Select a folder to preview or a file for details.")
                    .color(palette.muted),
            )
            .padding([12, 12])
            .width(Fill)
            .into(),
        ),
    };

    container(column![header, body].width(Fill).height(Fill))
        .id(format!("column-details-{}", props.view.0))
        .width(Length::FillPortion(1))
        .height(Fill)
        .clip(true)
        .into()
}

fn right_folder_header<'a>(
    view: ViewId,
    entry: &'a DirectoryEntry,
    allowed: bool,
    column_preview: Option<&'a ViewPreview>,
    interactive: bool,
    palette: a::Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    let icon = Icon::for_entry(entry);
    let mut bar = row![
        icons::view(icon, ty.icon(13.0), Some(palette.icon(icon)), 1.0),
        ty.small(entry.name.to_string_lossy())
            .font(ty.heading_font())
            .color(palette.text)
            .wrapping(text::Wrapping::None)
            .width(Fill)
    ]
    .spacing(6)
    .align_y(iced::Center);

    if let Some(count) = column_preview.and_then(ViewPreview::item_count) {
        bar = bar.push(ty.caption(count.to_string()).color(palette.muted));
    }
    if allowed {
        let action = Message::ColumnAdvance(view);
        let mut btn = button(
            container(icons::view(
                Icon::Forward,
                ty.icon(12.0),
                Some(palette.navigation),
                1.0,
            ))
            .center_x(ty.row(22.0))
            .center_y(ty.row(20.0)),
        )
        .padding(0)
        .width(ty.row(22.0))
        .height(ty.row(20.0))
        .style(move |_, status| header_icon_style(status, palette));
        if interactive {
            btn = btn.on_press(action.clone());
        }
        bar = bar.push(
            tooltip(
                focus_control::themed(format!("column-advance-{}", view.0), action, btn, palette),
                ty.small("Enter folder"),
                tooltip::Position::Bottom,
            )
            .style(move |_| palette.tooltip()),
        );
    }
    column_header_frame(bar.into(), palette)
}

fn right_file_header(entry: &DirectoryEntry, palette: a::Palette) -> Element<'static, Message> {
    let ty = palette.typography;
    let bar = row![
        ty.small("Details").color(palette.muted).width(Fill),
        ty.caption(inspector::kind_short(entry))
            .color(palette.muted)
    ]
    .spacing(6)
    .align_y(iced::Center);
    column_header_frame(bar.into(), palette)
}

fn column_header_frame<'a>(
    content: Element<'a, Message>,
    palette: a::Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    container(column![
        container(content)
            .padding([0, 8])
            .width(Fill)
            .height(Fill)
            .align_y(iced::Center)
            .clip(true),
        rule::horizontal(1).style(move |_| palette.rule())
    ])
    .width(Fill)
    .height(header_height(ty))
    .into()
}

fn earlier_menu<'a>(
    view: ViewId,
    ancestors: &[&'a Path],
    interactive: bool,
    palette: a::Palette,
) -> iced::widget::Column<'a, Message> {
    let ty = palette.typography;
    column(ancestors.iter().map(|path| {
        let action = Message::ColumnNavigate(view, path.to_path_buf());
        let mut control = button(ty.small(path.to_string_lossy()))
            .width(Fill)
            .style(move |_, status| palette.text_button(status));
        if interactive {
            control = control.on_press(action.clone());
        }
        focus_control::themed(
            format!("column-earlier-item-{view:?}-{path:?}"),
            action,
            control,
            palette,
        )
    }))
}

fn header_icon_style(status: button::Status, palette: a::Palette) -> button::Style {
    button::Style {
        background: (status == button::Status::Hovered).then(|| palette.row_hover.into()),
        text_color: palette.navigation,
        border: Border {
            radius: 4.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

fn folder_name(path: &Path) -> String {
    if path == Path::new("/") {
        "Computer".into()
    } else {
        path.file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned()
    }
}
