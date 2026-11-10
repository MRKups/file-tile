use iced::widget::{container, mouse_area, row, text};
use iced::{Border, Element, Fill};

use crate::application::contracts::directory::DirectoryEntry;
use crate::domain::ViewId;

use crate::{
    Message,
    boxes::entry_motion,
    window::appearance as a,
    window::icons::{self, Icon},
};

impl crate::Desktop {
    pub(crate) fn listed_entry(
        &self,
        view: ViewId,
        path: &std::path::Path,
    ) -> Option<&DirectoryEntry> {
        self.application
            .load(view)
            .and_then(crate::application::LoadState::listing)
            .and_then(|listing| listing.entry(path))
            .filter(|entry| {
                !entry.hidden
                    || self
                        .application
                        .workspace()
                        .view(view)
                        .is_some_and(|item| item.settings.show_hidden)
            })
    }
    pub(crate) fn select_entry(
        &mut self,
        view: ViewId,
        path: std::path::PathBuf,
    ) -> iced::Task<Message> {
        if !self.file_view_input_enabled(view)
            || !self.application.workspace().can_browse(view)
            || self.visible_entry(view, &path).is_none()
        {
            return iced::Task::none();
        }
        self.cancel_pending_scroll_restore(view);
        let range = if self.modifiers.shift() {
            let anchor = self
                .views
                .get(&view)
                .and_then(|s| s.selection_anchor.as_ref());
            self.application
                .load(view)
                .and_then(crate::application::LoadState::listing)
                .and_then(|_| {
                    let entries: Vec<_> = self
                        .application
                        .ordered_entries(view)
                        .filter(|entry| self.visible_entry(view, &entry.path).is_some())
                        .collect();
                    let first = entries
                        .iter()
                        .position(|entry| Some(&entry.path) == anchor)?;
                    let last = entries.iter().position(|entry| entry.path == path)?;
                    Some(
                        entries[first.min(last)..=first.max(last)]
                            .iter()
                            .map(|entry| entry.path.clone())
                            .collect::<Vec<_>>(),
                    )
                })
        } else {
            None
        };
        let was_active = self.active_view == Some(view);
        let had_preview_focus = self.column_previews.selected_entry(view).is_some()
            || self.column_previews.is_focused(view);
        self.type_jump.dismiss_session();
        let presentation = self.views.entry(view).or_default();
        presentation.pending_selection.clear();
        presentation.reveal_selected = false;
        presentation.reveal_after_motion = false;
        presentation.earlier_ancestors = false;
        let mut release = crate::menus::file_drag::ClickRelease::Keep;
        if let Some(range) = range {
            if !self.modifiers.command() {
                presentation.selection.clear();
            }
            presentation.selection.extend(range);
            presentation.selected = Some(path.clone());
        } else if self.modifiers.command() {
            if !presentation.selection.remove(&path) {
                presentation.selection.insert(path.clone());
            }
            presentation.selection_anchor = Some(path.clone());
            presentation.selected = if presentation.selection.contains(&path) {
                Some(path.clone())
            } else {
                presentation.selection.iter().next().cloned()
            };
        } else if presentation.selection.contains(&path) && presentation.selection.len() > 1 {
            release = crate::menus::file_drag::ClickRelease::NarrowMultiSelection;
            presentation.selected = Some(path.clone());
        } else if was_active
            && !had_preview_focus
            && presentation.selected.as_ref() == Some(&path)
            && (presentation.selection.is_empty()
                || (presentation.selection.len() == 1 && presentation.selection.contains(&path)))
        {
            release = crate::menus::file_drag::ClickRelease::DeselectPrimary;
            presentation.selected = Some(path.clone());
        } else {
            presentation.selection.clear();
            presentation.selection.insert(path.clone());
            presentation.selection_anchor = Some(path.clone());
            presentation.selected = Some(path.clone());
        }
        presentation.hint = None;
        self.column_previews.clear_selection(view);
        self.activate_view(view);
        self.focused_path = None;
        self.focus_version = self.focus_version.saturating_add(1);
        self.demand_single_thumbnail(view, &path);
        self.begin_file_drag(view, path, release);
        iced::Task::batch([
            self.fill_thumbnails(),
            iced::widget::operation::focus("workspace-canvas"),
        ])
    }
    pub(crate) fn hover_entry(
        &mut self,
        view: ViewId,
        path: std::path::PathBuf,
        hovered: bool,
    ) -> iced::Task<Message> {
        if self.workspace_input_enabled()
            && self.application.workspace().can_browse(view)
            && (!hovered || self.visible_entry(view, &path).is_some())
        {
            let presentation = self.views.entry(view).or_default();
            if hovered {
                presentation.hovered = Some(path);
            } else if presentation.hovered.as_ref() == Some(&path) {
                presentation.hovered = None;
            }
        }
        iced::Task::none()
    }
}

pub fn view<'a>(
    entry: &'a DirectoryEntry,
    props: crate::boxes::file_list::Props<'a>,
    outside_scope: bool,
) -> Element<'a, Message> {
    let palette = props.palette;
    let view = props.view;
    let interactive = props.interactive;
    let (size, date, _) = if props.settings.show_sizes || props.settings.show_dates {
        crate::runtime::metadata::labels(props, entry)
    } else {
        (String::new(), String::new(), None)
    };
    let name = entry.name.to_string_lossy();
    let decorated = row_content(entry, props, outside_scope, &name, &size, &date);
    let mut hit_target = mouse_area(decorated);
    if interactive && !outside_scope {
        hit_target = hit_target
            .on_press(Message::EntrySelected(view, entry.path.clone()))
            .on_double_click(Message::EntryOpened(view, entry.path.clone()))
            .on_enter(Message::EntryHovered(view, entry.path.clone(), true))
            .on_exit(Message::EntryHovered(view, entry.path.clone(), false))
            .interaction(iced::mouse::Interaction::Pointer);
    }
    let target: Element<'_, Message> = if interactive {
        crate::menus::entry_menu::target(view, entry.path.clone(), hit_target.into(), palette)
    } else {
        hit_target.into()
    };
    crate::runtime::metadata::hover_tooltip(
        target,
        props,
        props.load.and_then(crate::application::LoadState::listing),
        entry,
    )
}

fn row_content<'a>(
    entry: &'a DirectoryEntry,
    props: crate::boxes::file_list::Props<'a>,
    outside_scope: bool,
    name: &str,
    size: &str,
    date: &str,
) -> Element<'a, Message> {
    let palette = props.palette;
    let ty = palette.typography;
    let selected = props
        .selection
        .is_some_and(|selection| selection.contains(&entry.path))
        || props.selected == Some(entry.path.as_path());
    let hovered = props.hovered == Some(entry.path.as_path());
    let interactive = props.interactive;
    let settings = props.settings;
    let icon = Icon::for_entry(entry);
    let icon_size = ty.icon(18.0);
    let arrival = props.motion.and_then(|motion| motion.arrival(&entry.path));
    let drag_dim = if props
        .dragging
        .is_some_and(|paths| paths.contains(&entry.path))
    {
        0.45
    } else {
        1.0
    };
    let icon_alpha = (arrival.map_or(1.0, |v| v.icon_alpha) * drag_dim).clamp(0.0, 1.0);
    let label_alpha = (arrival.map_or(1.0, |v| v.label_alpha) * drag_dim).clamp(0.0, 1.0);
    let slide_x = arrival.map_or(0.0, |v| v.row_offset).max(0.0) * 14.0;
    let label_slide_x = arrival.map_or(0.0, |v| v.label_offset).max(0.0) * 14.0;
    let row_height = ty.row(crate::boxes::file_list::ROW_HEIGHT);
    let effective_height = (row_height * arrival.map_or(1.0, |v| v.height_factor)).max(0.5);
    let muted = palette.muted.scale_alpha(label_alpha);
    let primary = if outside_scope {
        muted
    } else {
        palette.text.scale_alpha(label_alpha)
    };
    let mut row = row![
        container(icons::view(
            icon,
            icon_size,
            Some(palette.icon(icon)),
            icon_alpha
        ))
        .padding(iced::Padding {
            bottom: icon_size / 12.0,
            ..iced::Padding::ZERO
        }),
        entry_label(ty, name, primary, label_slide_x)
    ]
    .spacing(10)
    .align_y(iced::Center);
    if entry.symlink {
        row = row.push(ty.caption("link").color(muted));
    }
    if outside_scope {
        row = row.push(ty.caption("outside scope").color(muted));
    }
    if settings.show_sizes {
        row = row.push(ty.caption(size.to_owned()).color(muted));
    }
    if settings.show_dates {
        row = row.push(ty.caption(date.to_owned()).color(muted));
    }
    let content = container(row)
        .width(Fill)
        .padding(iced::Padding {
            top: 0.0,
            right: 8.0,
            bottom: 0.0,
            left: 8.0 + slide_x,
        })
        .height(effective_height)
        .align_y(iced::Center)
        .clip(true)
        .style(move |_| container::Style {
            background: entry_motion::row_background(
                row_base_background(interactive, selected, hovered, drag_dim, palette),
                arrival,
                palette,
            ),
            border: Border {
                radius: 5.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        });
    entry_motion::decorate_row(content.into(), arrival, effective_height, palette)
}

fn row_base_background(
    interactive: bool,
    selected: bool,
    hovered: bool,
    drag_dim: f32,
    palette: a::Palette,
) -> Option<iced::Background> {
    if interactive && selected {
        Some(palette.row_selected.scale_alpha(drag_dim).into())
    } else if interactive && hovered {
        Some(palette.row_hover.scale_alpha(drag_dim).into())
    } else {
        None
    }
}

fn entry_label(
    ty: crate::window::typography::Style,
    name: &str,
    color: iced::Color,
    slide_x: f32,
) -> Element<'static, Message> {
    container(
        ty.text(name.to_owned())
            .color(color)
            .wrapping(text::Wrapping::None),
    )
    .width(Fill)
    .clip(true)
    .padding(iced::Padding {
        left: slide_x,
        ..iced::Padding::ZERO
    })
    .into()
}

pub(crate) fn focus_id(view: ViewId, path: &std::path::Path) -> String {
    format!("file-entry-{}-{path:?}", view.0)
}
