//! Current-listing item menus. Only explicit validated actions navigate or launch.

use crate::application::{
    Action, LoadState,
    contracts::directory::{DirectoryEntry, DirectoryListing, EntryKind},
};
use crate::domain::ViewId;
use crate::{
    Desktop, Message,
    boxes::{file_columns, file_entry},
    menus::context_menu,
    widgets::focus_control,
    window::icons::Icon,
};
use iced::widget::{column, container, space};
use iced::{Element, Point, Task, keyboard};
use std::{path::PathBuf, sync::Arc};

pub(crate) const MENU_ID: &str = "entry-context-menu";
pub(crate) const OPEN_ID: &str = "entry-context-open";
pub(crate) const COPY_ID: &str = "entry-context-copy-path";
pub(crate) const FILE_COPY_ID: &str = "entry-context-copy-files";
pub(crate) const CUT_ID: &str = "entry-context-cut-files";
pub(crate) const RENAME_ID: &str = "entry-context-rename";
pub(crate) const TRASH_ID: &str = "entry-context-trash";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Command {
    Open,
    CopyPath,
    Copy,
    Cut,
    Rename,
    Trash,
}

pub(crate) struct Menu {
    pub generation: u64,
    pub view: ViewId,
    pub path: PathBuf,
    listing: Arc<DirectoryListing>,
    position: Point,
    in_preview: bool,
}

impl Desktop {
    fn menu_listed_entry(&self, menu: &Menu) -> Option<&DirectoryEntry> {
        if menu.in_preview {
            self.column_previews.listed_entry(menu.view, &menu.path)
        } else {
            self.listed_entry(menu.view, &menu.path)
        }
    }

    fn menu_visible_entry(&self, menu: &Menu) -> Option<&DirectoryEntry> {
        if menu.in_preview {
            let scope = self
                .application
                .workspace()
                .view(menu.view)?
                .navigation
                .scope();
            self.column_previews
                .visible_entry(scope, menu.view, &menu.path)
        } else {
            self.visible_entry(menu.view, &menu.path)
        }
    }

    fn menu_retained_listing(&self, menu: &Menu) -> Option<Arc<DirectoryListing>> {
        if menu.in_preview {
            self.column_previews.retained_listing(menu.view)
        } else {
            self.application
                .load(menu.view)
                .and_then(LoadState::retained_listing)
        }
    }

    fn permits_entry_menu(&self, menu: &Menu) -> bool {
        self.application.accepts_workspace_input()
            && !self.modal_open()
            && self.application.workspace().can_browse(menu.view)
            && self.menu_listed_entry(menu).is_some()
            && self
                .menu_retained_listing(menu)
                .is_some_and(|listing| Arc::ptr_eq(&listing, &menu.listing))
    }

    pub(crate) fn open_entry_menu(
        &mut self,
        view: ViewId,
        path: PathBuf,
        point: Option<Point>,
    ) -> Task<Message> {
        if !self.file_view_input_enabled(view) || !self.application.workspace().can_browse(view) {
            return Task::none();
        }
        let resolved = if self.listed_entry(view, &path).is_some() {
            self.application
                .load(view)
                .and_then(LoadState::retained_listing)
                .map(|listing| (listing, false))
        } else if self.column_previews.listed_entry(view, &path).is_some() {
            self.column_previews
                .retained_listing(view)
                .map(|listing| (listing, true))
        } else {
            None
        };
        let Some((listing, in_preview)) = resolved else {
            return Task::none();
        };
        let Some(generation) = self.next_entry_menu_generation.checked_add(1) else {
            return Task::none();
        };
        // Keyboard opening uses the containing box, never a stale pointer from another view.
        let position = point.unwrap_or_else(|| {
            let geometry = self.geometry();
            self.application
                .workspace()
                .boxes()
                .iter()
                .find(|item| item.view(view).is_some())
                .map_or(Point::ORIGIN, |item| {
                    let rect = geometry.rect(item.rect);
                    let x_inset = if in_preview && rect.width > file_columns::TWO_COLUMN_MIN_WIDTH {
                        rect.width * 0.5 + 16.0
                    } else {
                        24.0
                    };
                    Point::new(
                        rect.x + geometry.viewport.x - geometry.offset.x + x_inset,
                        rect.y + geometry.viewport.y - geometry.offset.y + 48.0,
                    )
                })
        });
        if !position.x.is_finite() || !position.y.is_finite() {
            return Task::none();
        }
        self.next_entry_menu_generation = generation;
        self.prepare_context_menu();
        if in_preview {
            self.column_previews.select(view, path.clone());
            self.views.entry(view).or_default().hint = None;
        } else {
            self.column_previews.clear_selection(view);
            let state = self.views.entry(view).or_default();
            if !state.selection.contains(&path) {
                state.selection.clear();
                state.selection.insert(path.clone());
                state.selection_anchor = Some(path.clone());
            }
            state.selected = Some(path.clone());
            state.hint = None;
        }
        self.active_view = Some(view);
        let menu = Menu {
            generation,
            view,
            path,
            listing,
            position,
            in_preview,
        };
        let id = if command_enabled(self, &menu, Command::Open) {
            OPEN_ID
        } else if command_enabled(self, &menu, Command::Copy) {
            FILE_COPY_ID
        } else {
            COPY_ID
        };
        self.entry_menu = Some(menu);
        iced::widget::operation::focus(context_menu::opening_focus(point, id))
    }

    pub(crate) fn dismiss_entry_menu(&mut self, generation: u64) -> Task<Message> {
        if !self
            .entry_menu
            .as_ref()
            .is_some_and(|menu| menu.generation == generation)
        {
            return Task::none();
        }
        let Some(menu) = self.entry_menu.take() else {
            return Task::none();
        };
        self.focus_version = self.focus_version.saturating_add(1);
        iced::widget::operation::focus(file_entry::focus_id(menu.view, &menu.path))
    }

    pub(crate) fn entry_menu_action(&mut self, generation: u64, command: Command) -> Task<Message> {
        let view = self.entry_menu.as_ref().map(|menu| menu.view);
        let mut result = None;
        let task = self.entry_menu_action_with_clipboard(generation, command, |value| {
            result = Some(crate::platform::clipboard::copy_text(&value));
            Task::none()
        });
        if let (Some(view), Some(result)) = (view, result) {
            self.views.entry(view).or_default().hint = result.err();
        }
        task
    }

    // Inject text clipboard transport so checks can observe the exact payload
    // without touching the host clipboard.
    pub(crate) fn entry_menu_action_with_clipboard(
        &mut self,
        generation: u64,
        command: Command,
        write: impl FnOnce(String) -> Task<Message>,
    ) -> Task<Message> {
        let Some(menu) = &self.entry_menu else {
            return Task::none();
        };
        if menu.generation != generation
            || !self.permits_entry_menu(menu)
            || !command_enabled(self, menu, command)
        {
            return Task::none();
        }
        let (view, path, in_preview, directory) = (
            menu.view,
            menu.path.clone(),
            menu.in_preview,
            menu.listing.directory.clone(),
        );
        let anchor = menu_bounds(self, menu).position();
        let focus = self.dismiss_entry_menu(generation);
        if command == Command::Rename {
            self.anchor_file_prompt(view, anchor);
            let task = self.start_rename_in(view, directory, path);
            return if self
                .views
                .get(&view)
                .is_some_and(|state| state.name_draft.is_some())
            {
                task
            } else {
                Task::batch([task, focus])
            };
        }
        if command == Command::Trash {
            self.anchor_file_prompt(view, anchor);
            let task = if in_preview {
                self.start_trash_paths(view, directory, vec![path])
            } else {
                self.start_trash(view)
            };
            return if self
                .views
                .get(&view)
                .is_some_and(|state| state.trash_draft.is_some())
            {
                task
            } else {
                Task::batch([task, focus])
            };
        }
        let task = match command {
            Command::Copy => {
                if in_preview {
                    self.transfer_paths(view, vec![path], false)
                } else {
                    self.copy_selected_files(view)
                }
            }
            Command::Cut => {
                if in_preview {
                    self.transfer_paths(view, vec![path], true)
                } else {
                    self.cut_selected_files(view)
                }
            }
            Command::Rename | Command::Trash => Task::none(),
            Command::CopyPath => path
                .to_str()
                .map_or_else(Task::none, |path| write(path.to_owned())),
            Command::Open => {
                if in_preview {
                    self.open_column_preview_entry(view, path)
                } else if self
                    .visible_entry(view, &path)
                    .is_some_and(|entry| entry.kind == EntryKind::Directory)
                {
                    self.open_entry(view, path)
                } else {
                    self.dispatch(Action::OpenFile(view, path))
                }
            }
        };
        Task::batch([task, focus])
    }

    pub(crate) fn reconcile_entry_menu(&mut self) {
        if self
            .entry_menu
            .as_ref()
            .is_some_and(|menu| !self.permits_entry_menu(menu))
        {
            self.entry_menu = None;
        }
    }

    pub(crate) fn entry_menu_input(&mut self, event: iced::Event) -> Task<Message> {
        let Some(menu) = &self.entry_menu else {
            return Task::none();
        };
        if matches!(
            event,
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                repeat: false,
                ..
            })
        ) {
            return self.dismiss_entry_menu(menu.generation);
        }
        context_menu::input(event)
    }
}

fn command_enabled(desktop: &Desktop, menu: &Menu, command: Command) -> bool {
    let Some(entry) = desktop.menu_listed_entry(menu) else {
        return false;
    };
    match command {
        Command::Rename => {
            (entry.kind != EntryKind::Other || entry.symlink)
                && (menu.in_preview
                    || desktop
                        .views
                        .get(&menu.view)
                        .is_none_or(|state| state.selection.len() <= 1))
                && desktop
                    .application
                    .can_write_folder(menu.view, &menu.listing.directory)
        }
        Command::Copy => entry.kind != EntryKind::Other || entry.symlink,
        Command::Cut => {
            (entry.kind != EntryKind::Other || entry.symlink)
                && desktop.application.workspace().mode() == crate::domain::Mode::Browse
                && desktop.application.move_guidance(&menu.path).is_ok()
        }
        Command::Trash => {
            (entry.kind != EntryKind::Other || entry.symlink)
                && desktop.application.workspace().mode() == crate::domain::Mode::Browse
                && if menu.in_preview {
                    desktop
                        .application
                        .can_write_folder(menu.view, &menu.listing.directory)
                        && desktop
                            .application
                            .trash_guidance(menu.view, &menu.path)
                            .is_ok()
                } else {
                    desktop
                        .views
                        .get(&menu.view)
                        .filter(|state| !state.selection.is_empty())
                        .map_or_else(
                            || {
                                desktop
                                    .application
                                    .trash_guidance(menu.view, &menu.path)
                                    .is_ok()
                            },
                            |state| {
                                state.selection.len()
                                    <= crate::application::files::file_operations::ITEM_LIMIT
                                    && state.selection.iter().all(|path| {
                                        desktop.application.trash_guidance(menu.view, path).is_ok()
                                    })
                            },
                        )
                }
        }
        Command::CopyPath => menu.path.to_str().is_some(),
        Command::Open => match entry.kind {
            EntryKind::Directory => desktop.menu_visible_entry(menu).is_some(),
            EntryKind::File => true,
            EntryKind::Other => false,
        },
    }
}

pub(crate) fn target<'a>(
    view: ViewId,
    path: PathBuf,
    content: Element<'a, Message>,
    palette: crate::window::appearance::Palette,
) -> Element<'a, Message> {
    let open = Message::EntryOpened(view, path.clone());
    target_with_open(view, path, open, content, palette)
}

pub(crate) fn preview_target<'a>(
    view: ViewId,
    path: PathBuf,
    content: Element<'a, Message>,
    palette: crate::window::appearance::Palette,
) -> Element<'a, Message> {
    let open = Message::ColumnPreviewOpened(view, path.clone());
    target_with_open(view, path, open, content, palette)
}

fn target_with_open<'a>(
    view: ViewId,
    path: PathBuf,
    open: Message,
    content: Element<'a, Message>,
    palette: crate::window::appearance::Palette,
) -> Element<'a, Message> {
    let pointer_path = path.clone();
    let content = context_menu::press(
        None,
        Some(Box::new(move |point| {
            Message::OpenEntryMenu(view, pointer_path.clone(), Some(point))
        })),
        content,
    );
    focus_control::themed_keys(
        file_entry::focus_id(view, &path),
        open,
        content,
        vec![(
            keyboard::key::Named::F10,
            keyboard::Modifiers::SHIFT,
            Message::OpenEntryMenu(view, path, None),
        )],
        palette,
    )
}

pub(crate) fn view(desktop: &Desktop) -> Element<'_, Message> {
    let palette = desktop.box_palette();
    let ty = palette.typography;
    let Some((menu, entry)) = desktop
        .entry_menu
        .as_ref()
        .and_then(|menu| Some((menu, desktop.menu_listed_entry(menu)?)))
    else {
        return space().into();
    };
    let action = |command| {
        command_enabled(desktop, menu, command)
            .then_some(Message::EntryMenuAction(menu.generation, command))
    };
    let item = |id, icon_id, icon, label, cmd| {
        context_menu::item(id, icon_id, icon, label, action(cmd), palette)
    };
    let mut contents = column![];
    if entry.kind != EntryKind::Other {
        let (icon, label) = if entry.kind == EntryKind::Directory {
            (Icon::Folder, "Open folder")
        } else {
            (Icon::External, "Open")
        };
        contents = contents.push(item(
            OPEN_ID,
            "entry-context-open-icon",
            icon,
            label,
            Command::Open,
        ));
    }
    contents = contents
        .push(item(
            RENAME_ID,
            "entry-context-rename-icon",
            Icon::Rename,
            "Rename",
            Command::Rename,
        ))
        .push(context_menu::divider(palette))
        .push(item(
            FILE_COPY_ID,
            "entry-context-copy-files-icon",
            Icon::Copy,
            "Copy",
            Command::Copy,
        ))
        .push(item(
            CUT_ID,
            "entry-context-cut-files-icon",
            Icon::Move,
            "Cut",
            Command::Cut,
        ))
        .push(item(
            COPY_ID,
            "entry-context-copy-icon",
            Icon::Copy,
            "Copy path",
            Command::CopyPath,
        ))
        .push(context_menu::divider(palette));
    let trash_action = action(Command::Trash);
    contents = contents.push(context_menu::item(
        TRASH_ID,
        "entry-context-trash-icon",
        Icon::Trash,
        if trash_action.is_some() {
            "Move to Trash"
        } else {
            "Deletion Prohibited"
        },
        trash_action,
        palette,
    ));
    let guidance = if !command_enabled(desktop, menu, Command::CopyPath) {
        Some("Path cannot be copied as text.")
    } else if entry.kind == EntryKind::Directory && !command_enabled(desktop, menu, Command::Open) {
        Some("Outside this view's scope.")
    } else {
        None
    };
    if let Some(guidance) = guidance {
        contents =
            contents.push(container(ty.small(guidance).color(palette.muted)).padding([4, 12]));
    }
    let rect = menu_bounds(desktop, menu);
    context_menu::surface(
        MENU_ID,
        contents,
        rect,
        Message::DismissEntryMenu(menu.generation),
        palette,
    )
}

fn menu_bounds(desktop: &Desktop, menu: &Menu) -> iced::Rectangle {
    let ty = crate::window::typography::Style(desktop.application.typography());
    let entry = desktop.menu_listed_entry(menu);
    let rows = if entry.is_some_and(|entry| entry.kind != EntryKind::Other) {
        6.0
    } else {
        5.0
    };
    let guidance = !command_enabled(desktop, menu, Command::CopyPath)
        || entry.is_some_and(|entry| {
            entry.kind == EntryKind::Directory && !command_enabled(desktop, menu, Command::Open)
        });
    context_menu::bounds(
        menu.position,
        desktop.size,
        ty.row(238.0),
        30.0 + rows * ty.row(context_menu::ROW_HEIGHT) + if guidance { ty.row(28.0) } else { 0.0 },
    )
}
