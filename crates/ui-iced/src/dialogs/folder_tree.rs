//! Virtualized folder-only Configure tree. Expand and select have distinct actions.
use crate::application::{
    Action,
    files::folder_tree::{TreeLoad, TreeNode},
};
use crate::domain::{ScopeKind, ViewId};
use crate::{
    Desktop, Message, dialogs::range_editor, widgets::focus_control, window::appearance as a,
};
use iced::widget::{button, column, container, row, scrollable, space, text, tooltip};
use iced::{Element, Fill};

pub(crate) const ROW_HEIGHT: f32 = 30.0;

impl Desktop {
    pub(crate) fn tree_message(&mut self, message: Message) -> iced::Task<Message> {
        if !self.workspace_input_enabled() {
            return iced::Task::none();
        }
        match message {
            Message::TreeChosen(view, path) => self.tree_choice(view, path),
            Message::TreeScrolled(view, offset) => {
                if let Some(draft) = &mut self.box_settings
                    && draft.view == view
                {
                    draft.tree_offset = offset;
                }
                iced::Task::none()
            }
            _ => iced::Task::none(),
        }
    }
}
pub(crate) fn view<'a>(
    desktop: &'a Desktop,
    draft: &'a crate::dialogs::box_settings::Draft,
) -> Element<'a, Message> {
    let palette = desktop.configure_palette();
    let ty = palette.typography;
    let view = draft.view;
    let Some(tree) = desktop
        .application
        .folder_tree()
        .filter(|tree| tree.view == view)
    else {
        return ty.text("Folder selection is unavailable.").into();
    };
    let scope = desktop
        .application
        .workspace()
        .view(view)
        .map(|item| item.navigation.scope());
    let range = draft.range_page && scope.is_some_and(|scope| scope.kind() == ScopeKind::Range);
    let mut header = column![];
    if range {
        header = header
            .push(range_editor::stages(view, draft.range.stage, palette))
            .push(
                ty.caption(if draft.range.stage == range_editor::Stage::Start {
                    "Choose an origin anywhere in the tree."
                } else {
                    "Choose range ends below Origin. Keep at least one."
                })
                .color(palette.muted),
            );
    }
    let rows = tree.rows();
    let row_height = ty.row(ROW_HEIGHT);
    let tree_height = (draft.bounds(desktop).height - 620.0).clamp(180.0, 460.0);
    let max_offset = (rows.len() as f32 * row_height - tree_height).max(0.0);
    let offset = draft.tree_offset.clamp(0.0, max_offset);
    let first = (offset / row_height).floor().max(0.0) as usize;
    let start = first.saturating_sub(3).min(rows.len());
    let end = first.saturating_add(17).min(rows.len());
    let mut contents = column![space().height(start as f32 * row_height)].width(Fill);
    for (depth, path) in &rows[start..end] {
        if let Some(node) = tree.nodes.get(*path) {
            contents = contents.push(tree_row(desktop, view, *depth, path, node, draft));
        }
    }
    contents = contents.push(space().height((rows.len() - end) as f32 * row_height));
    let tree_list = a::scroll_area(container(contents).padding(iced::Padding {
        right: 12.0,
        ..iced::Padding::ZERO
    }))
    .id("folder-selection-tree")
    .height(tree_height)
    .direction(scrollable::Direction::Vertical(a::scrollbar()))
    .style(palette.scrollable())
    .on_scroll(move |viewport| Message::TreeScrolled(view, viewport.absolute_offset().y));
    let mut panel = column![header.spacing(8), tree_list].spacing(12);
    if range {
        if let Some(range) = desktop.application.workspace().view(view).and_then(|item| {
            draft
                .range
                .preview(item.navigation.current(), desktop.application.home())
        }) {
            let mut chips = row![].spacing(4);
            for (index, path) in range.stops().iter().enumerate() {
                let action = Message::TreeChosen(view, path.clone());
                let name = path
                    .file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy();
                chips = chips.push(focus_control::themed(
                    format!("range-stop-chip-{index}"),
                    action.clone(),
                    button(ty.caption(format!("■ {name} ×")).color(palette.text))
                        .style(move |_, status| palette.text_button(status))
                        .on_press(action),
                    palette,
                ));
            }
            if draft.range.stage == range_editor::Stage::Stops {
                panel = panel.push(chips.wrap());
            }
        }
        panel = panel.push(range_editor::view(desktop, draft));
    } else if let Some(item) = desktop.application.workspace().view(view) {
        panel = panel.push(
            ty.caption(
                item.navigation
                    .current()
                    .to_string_lossy()
                    .replace('/', " › "),
            )
            .color(palette.muted),
        );
    }
    panel.into()
}
fn tree_row<'a>(
    desktop: &'a Desktop,
    view: ViewId,
    depth: usize,
    path: &'a std::path::Path,
    node: &'a TreeNode,
    draft: &'a crate::dialogs::box_settings::Draft,
) -> Element<'a, Message> {
    let palette = desktop.configure_palette();
    let ty = palette.typography;
    let expand = disclosure(view, path, node.expanded, palette);
    let (label, icon) = row_label_and_icon(desktop, view, path);
    let scope = desktop
        .application
        .workspace()
        .view(view)
        .map(|item| item.navigation.scope());
    let preview = desktop
        .application
        .workspace()
        .view(view)
        .filter(|_| draft.range_page)
        .and_then(|item| {
            draft
                .range
                .preview(item.navigation.current(), desktop.application.home())
        });
    let boundaries = preview.as_ref();
    let allowed = boundaries.is_none_or(|range| {
        draft.range.stage == range_editor::Stage::Start || node.target.starts_with(range.start())
    }) && (draft.range.stage == range_editor::Stage::Start || boundaries.is_some());
    let start = boundaries.is_some_and(|range| range.start() == node.target)
        || scope.is_some_and(|scope| {
            scope.kind() == ScopeKind::Branch && scope.root() == Some(node.target.as_path())
        });
    let stop = boundaries.is_some_and(|range| range.stops().contains(&node.target));
    let permitted = boundaries.is_some_and(|range| range.allows(&node.target));
    let current = desktop
        .application
        .workspace()
        .view(view)
        .is_some_and(|item| item.navigation.current() == node.target);
    let action = Message::TreeChosen(view, node.target.clone());
    let label = format!(
        "{label}{}",
        if current && boundaries.is_none() {
            " Selected"
        } else {
            ""
        }
    );
    let color = if boundaries.is_some() && !permitted {
        palette.muted
    } else if permitted {
        palette.accent
    } else {
        palette.text
    };
    let select: Element<'a, Message> = if allowed {
        focus_control::themed(
            format!("tree-select-{path:?}"),
            action.clone(),
            button(folder_label(label, icon, color, path, start, stop, palette))
                .padding(3)
                .width(Fill)
                .style(move |_, status| palette.text_button(status))
                .on_press(action),
            palette,
        )
    } else {
        folder_label(label, icon, palette.muted, path, start, stop, palette)
    };
    let mut row = row![expand, select]
        .height(ty.row(ROW_HEIGHT))
        .align_y(iced::Center);
    let note = node_note(&node.load);
    if let Some(note) = note {
        row = row.push(ty.caption(note).color(palette.error));
    }
    let row = container(row)
        .padding(iced::Padding {
            left: depth.min(12) as f32 * 12.0,
            ..iced::Padding::ZERO
        })
        .style(move |_| container::Style {
            background: (current || permitted).then_some(palette.row_selected.into()),
            ..container::Style::default()
        });
    let hint = node_hint(path, &node.load);
    tooltip(row, ty.small(hint), tooltip::Position::Bottom)
        .style(move |_| palette.tooltip())
        .into()
}

fn row_label_and_icon(
    desktop: &Desktop,
    view: ViewId,
    path: &std::path::Path,
) -> (String, crate::window::icons::Icon) {
    let root_meta = desktop
        .application
        .folder_tree()
        .filter(|tree| tree.view == view)
        .and_then(|tree| tree.root_meta(path));
    if path == desktop.application.home() {
        ("Home".into(), crate::window::icons::Icon::Home)
    } else if let Some(meta) = root_meta {
        (
            meta.label.clone(),
            crate::boxes::sidebar::location_icon(&meta.kind),
        )
    } else if path == std::path::Path::new("/") {
        ("Computer".into(), crate::window::icons::Icon::Drive)
    } else if path == std::path::Path::new("/Volumes") {
        ("Volumes".into(), crate::window::icons::Icon::Drive)
    } else {
        (
            path.file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .into_owned(),
            crate::window::icons::Icon::Folder,
        )
    }
}

fn folder_label(
    label: String,
    icon: crate::window::icons::Icon,
    color: iced::Color,
    path: &std::path::Path,
    start: bool,
    stop: bool,
    palette: a::Palette,
) -> Element<'static, Message> {
    let ty = palette.typography;
    let mut contents = row![
        crate::window::icons::view(icon, 13.0, Some(color), 1.0),
        ty.small(label)
            .color(color)
            .wrapping(text::Wrapping::None)
            .width(Fill)
    ];
    if start {
        contents = contents.push(
            container(ty.caption("● Origin").color(palette.accent))
                .id(format!("tree-start-{path:?}")),
        );
    }
    if stop {
        contents = contents.push(
            container(ty.caption("■ End").color(palette.text)).id(format!("tree-stop-{path:?}")),
        );
    }
    contents.spacing(6).width(Fill).align_y(iced::Center).into()
}

fn node_hint(path: &std::path::Path, load: &TreeLoad) -> String {
    match load {
        TreeLoad::Failed(error) => error.clone(),
        TreeLoad::Ready {
            issues, limited, ..
        } if *limited || !issues.is_empty() => {
            let detail = issues
                .first()
                .map_or_else(String::new, |issue| issue.error.describe("expand folder"));
            format!(
                "Partial tree. {detail} Collapse unused branches, then collapse and expand this folder to retry. The path field remains available."
            )
        }
        _ => path.to_string_lossy().into_owned(),
    }
}

fn node_note(load: &TreeLoad) -> Option<&'static str> {
    match load {
        TreeLoad::Queued | TreeLoad::Loading(_) => Some("Loading…"),
        TreeLoad::Failed(_) => Some("Failed, collapse and retry"),
        TreeLoad::Ready { limited: true, .. } => Some("Partial: tree limit"),
        TreeLoad::Ready { partial: true, .. } => Some("Partial"),
        _ => None,
    }
}

fn disclosure(
    view: ViewId,
    path: &std::path::Path,
    expanded: bool,
    palette: a::Palette,
) -> Element<'static, Message> {
    let ty = palette.typography;
    let action = Message::Action(Action::ToggleTree(view, path.into()));
    focus_control::themed(
        format!("tree-expand-{path:?}"),
        action.clone(),
        button(ty.control(if expanded { "⌄" } else { "›" }))
            .width(22)
            .padding(2)
            .style(move |_, status| palette.text_button(status))
            .on_press(action),
        palette,
    )
}
