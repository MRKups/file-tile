//! Structured file-details inspector for the right pane of the 2-column Miller view.

use iced::Element;
use iced::Fill;
use iced::widget::{button, column, container, row, scrollable, text};

use crate::application::contracts::directory::DirectoryEntry;
use crate::{
    Message,
    boxes::file_list,
    runtime::{metadata, thumbnails},
    widgets::focus_control,
    window::{appearance as a, icons, typography},
};

pub(super) fn file_details<'a>(
    props: file_list::Props<'a>,
    entry: &'a DirectoryEntry,
) -> Element<'a, Message> {
    let palette = props.palette;
    let ty = palette.typography;
    let value = props.metadata.and_then(|values| values.get(&entry.path));
    let icon = icons::Icon::for_entry(entry);

    let artwork = thumbnails::image(
        props.thumbnails,
        props.view,
        props.revision,
        &entry.path,
        120.0,
    )
    .unwrap_or_else(|| {
        container(icons::view(icon, 36.0, Some(palette.icon(icon)), 1.0))
            .padding([6, 0])
            .into()
    });

    let mut meta = column![
        meta_row(ty, palette, "Type", kind_label(entry)),
        meta_row(ty, palette, "Size", metadata::size(value)),
        meta_row(ty, palette, "Modified", metadata::date(value)),
    ]
    .spacing(5);
    if entry.dataless {
        meta = meta.push(meta_row(ty, palette, "Status", "Online only".into()));
    }

    let mut fields = column![
        artwork,
        ty.text(entry.name.to_string_lossy())
            .font(ty.heading_font())
            .color(palette.text)
            .wrapping(text::Wrapping::WordOrGlyph),
        meta,
    ]
    .spacing(10);

    if let Some(error) = metadata::error(value) {
        fields = fields.push(ty.caption(error).color(palette.error));
    }
    if let Some(error) = thumbnails::error(props, &entry.path) {
        fields = fields.push(ty.caption(error).color(palette.muted));
    }

    let action = Message::EntryOpened(props.view, entry.path.clone());
    let mut open =
        button(ty.small("Open file")).style(move |_, status| palette.text_button(status));
    if props.interactive {
        open = open.on_press(action.clone());
    }
    fields = fields.push(focus_control::themed(
        format!("column-open-{}", props.view.0),
        action,
        open,
        palette,
    ));

    let content: Element<'a, Message> =
        a::scroll_area(container(fields).padding([12, 12]).width(Fill))
            .direction(scrollable::Direction::Vertical(a::scrollbar()))
            .height(Fill)
            .style(palette.scrollable())
            .into();
    if props.interactive {
        let view = props.view;
        let path = entry.path.clone();
        crate::menus::context_menu::press(
            None,
            Some(Box::new(move |point| {
                Message::OpenEntryMenu(view, path.clone(), Some(point))
            })),
            content,
        )
    } else {
        content
    }
}

pub(super) fn kind_short(entry: &DirectoryEntry) -> String {
    if entry.symlink {
        return "link".into();
    }
    entry
        .path
        .extension()
        .and_then(|ext| ext.to_str())
        .filter(|ext| !ext.is_empty() && ext.len() <= 8)
        .map_or_else(|| "file".into(), str::to_ascii_uppercase)
}

fn kind_label(entry: &DirectoryEntry) -> String {
    if entry.symlink {
        return "Symlink".into();
    }
    entry
        .path
        .extension()
        .and_then(|ext| ext.to_str())
        .filter(|ext| !ext.is_empty() && ext.len() <= 10)
        .map_or_else(
            || "File".into(),
            |ext| format!("{} file", ext.to_ascii_uppercase()),
        )
}

fn meta_row<'a>(
    ty: typography::Style,
    palette: a::Palette,
    label: &'static str,
    value: String,
) -> Element<'a, Message> {
    row![
        container(ty.caption(label).color(palette.muted)).width(ty.row(54.0)),
        ty.caption(value)
            .color(palette.text)
            .wrapping(text::Wrapping::WordOrGlyph)
    ]
    .spacing(6)
    .into()
}
