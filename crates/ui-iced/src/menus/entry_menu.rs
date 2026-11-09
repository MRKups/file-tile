//! Current-listing item menus. Only explicit validated actions navigate or launch.

use crate::application::{
    Action, LoadState,
    contracts::directory::{DirectoryEntry, DirectoryListing, EntryKind},
};
use crate::domain::{ViewId, files::CreateKind};
use crate::{
    Desktop, Message,
    boxes::{file_columns, file_entry},
    menus::context_menu,
    runtime::{metadata, thumbnails},
    widgets::focus_control,
    window::icons::{self, Icon},
};
use iced::widget::{column, container, mouse_area, opaque, pin, row, space, stack};
use iced::{Alignment, Border, Element, Fill, Point, Rectangle, Shadow, Task, Vector, keyboard};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

pub(crate) const MENU_ID: &str = "entry-context-menu";
pub(crate) const OPEN_ID: &str = "entry-context-open";
pub(crate) const COPY_ID: &str = "entry-context-copy-path";
pub(crate) const FILE_COPY_ID: &str = "entry-context-copy-files";
pub(crate) const CUT_ID: &str = "entry-context-cut-files";
pub(crate) const RENAME_ID: &str = "entry-context-rename";
pub(crate) const CREATE_ID: &str = "entry-context-create";
pub(crate) const DETAILS_ID: &str = "entry-context-details";
pub(crate) const DETAILS_CARD_ID: &str = "entry-context-details-card";
pub(crate) const TRASH_ID: &str = "entry-context-trash";
const DETAILS_HOVER_DELAY: Duration = Duration::from_millis(700);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Command {
    Open,
    CopyPath,
    Copy,
    Cut,
    Rename,
    Create,
    Back,
    New(CreateKind),
    Details,
    DetailsHover(bool),
    Trash,
}

pub(crate) struct Menu {
    pub generation: u64,
    pub view: ViewId,
    pub path: PathBuf,
    listing: Arc<DirectoryListing>,
    position: Point,
    in_preview: bool,
    pub create: bool,
    pub details_open: bool,
    details_hover_since: Option<Instant>,
}

impl Menu {
    pub(crate) fn is_hover_waiting(&self) -> bool {
        !self.details_open && self.details_hover_since.is_some()
    }

    pub(crate) fn tick(&mut self, now: Instant) {
        if !self.details_open
            && self
                .details_hover_since
                .is_some_and(|since| now.saturating_duration_since(since) >= DETAILS_HOVER_DELAY)
        {
            self.details_open = true;
            self.details_hover_since = None;
        }
    }
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
        if in_preview {
            self.activate_column_preview(view);
        } else {
            self.activate_view(view);
        }
        let menu = Menu {
            generation,
            view,
            path,
            listing,
            position,
            in_preview,
            create: false,
            details_open: false,
            details_hover_since: None,
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
        if matches!(
            (menu.create, command),
            (false, Command::Create) | (true, Command::Back)
        ) {
            return self.turn_entry_menu_page(matches!(command, Command::Create));
        }
        if matches!(command, Command::New(_) | Command::Rename | Command::Trash) {
            return self.open_entry_prompt(generation, command);
        }
        let (view, path, in_preview, _) = (
            menu.view,
            menu.path.clone(),
            menu.in_preview,
            menu.listing.directory.clone(),
        );
        if matches!(command, Command::Details | Command::DetailsHover(_)) {
            return self.handle_details_command(view, path, command);
        }
        let focus = self.dismiss_entry_menu(generation);
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
            Command::Rename
            | Command::Create
            | Command::Back
            | Command::New(_)
            | Command::Details
            | Command::DetailsHover(_)
            | Command::Trash => Task::none(),
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

    fn turn_entry_menu_page(&mut self, create: bool) -> Task<Message> {
        let Some(next_gen) = self.next_entry_menu_generation.checked_add(1) else {
            return Task::none();
        };
        self.next_entry_menu_generation = next_gen;
        if let Some(menu) = &mut self.entry_menu {
            menu.generation = next_gen;
            menu.create = create;
            menu.details_open = false;
            menu.details_hover_since = None;
        }
        iced::widget::operation::focus("context-menu-no-selection")
    }

    fn open_entry_prompt(&mut self, generation: u64, command: Command) -> Task<Message> {
        let Some(menu) = &self.entry_menu else {
            return Task::none();
        };
        let (view, path, in_preview, directory) = (
            menu.view,
            menu.path.clone(),
            menu.in_preview,
            menu.listing.directory.clone(),
        );
        let anchor = menu_bounds(self, menu).position();
        let focus = self.dismiss_entry_menu(generation);
        self.anchor_file_prompt(view, anchor);
        let task = match command {
            Command::New(kind) => self.start_name_in(view, directory, kind),
            Command::Rename => self.start_rename_in(view, directory, path),
            Command::Trash if in_preview => self.start_trash_paths(view, directory, vec![path]),
            Command::Trash => self.start_trash(view),
            _ => Task::none(),
        };
        let opened = self
            .views
            .get(&view)
            .is_some_and(|state| state.name_draft.is_some() || state.trash_draft.is_some());
        if opened {
            task
        } else {
            Task::batch([task, focus])
        }
    }

    fn handle_details_command(
        &mut self,
        view: ViewId,
        path: PathBuf,
        command: Command,
    ) -> Task<Message> {
        if let Some(menu) = &mut self.entry_menu {
            match command {
                Command::Details => {
                    menu.details_open = true;
                    menu.details_hover_since = None;
                }
                Command::DetailsHover(true) => {
                    if !menu.details_open && menu.details_hover_since.is_none() {
                        menu.details_hover_since = Some(Instant::now());
                    }
                }
                Command::DetailsHover(false) => {
                    menu.details_hover_since = None;
                }
                _ => {}
            }
        }
        if !matches!(command, Command::Details | Command::DetailsHover(true)) {
            return Task::none();
        }
        self.demand_single_thumbnail(view, &path);
        let thumbnails = self.fill_thumbnails();
        let metadata = if self.application.metadata(view, &path).is_none()
            && !self
                .application
                .metadata_demand_paths(view)
                .is_some_and(|paths| paths.contains(&path))
        {
            let mut paths = self
                .application
                .metadata_demand_paths(view)
                .map_or_else(Vec::new, <[PathBuf]>::to_vec);
            paths.push(path);
            self.dispatch(Action::MetadataNeeded(view, paths))
        } else {
            Task::none()
        };
        Task::batch([thumbnails, metadata])
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
        Command::Back => menu.create,
        Command::Create => {
            !menu.create
                && desktop.application.workspace().mode() == crate::domain::Mode::Browse
                && desktop
                    .application
                    .can_write_folder(menu.view, &menu.listing.directory)
        }
        Command::New(_) => {
            menu.create
                && desktop.application.workspace().mode() == crate::domain::Mode::Browse
                && desktop
                    .application
                    .can_write_folder(menu.view, &menu.listing.directory)
        }
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
                && desktop
                    .application
                    .rename_guidance(menu.view, &menu.path)
                    .is_ok()
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
                && desktop
                    .application
                    .can_write_folder(menu.view, &menu.listing.directory)
                && if menu.in_preview {
                    desktop
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
        Command::Details | Command::DetailsHover(_) => true,
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
    let Some((menu, entry)) = desktop
        .entry_menu
        .as_ref()
        .and_then(|menu| Some((menu, desktop.menu_listed_entry(menu)?)))
    else {
        return space().into();
    };
    let contents = menu_contents(desktop, menu, entry, palette);
    let rect = menu_bounds(desktop, menu);
    let surface = context_menu::surface(
        MENU_ID,
        contents,
        rect,
        Message::DismissEntryMenu(menu.generation),
        palette,
    );
    if !menu.details_open {
        return surface;
    }
    let details_rect = details_bounds(desktop, entry, rect);
    let details_card = details_view(desktop, menu, entry, details_rect, palette);
    stack![
        surface,
        pin(opaque(details_card))
            .x(details_rect.x)
            .y(details_rect.y)
    ]
    .width(Fill)
    .height(Fill)
    .into()
}

fn menu_contents<'a>(
    desktop: &'a Desktop,
    menu: &'a Menu,
    entry: &'a DirectoryEntry,
    palette: crate::window::appearance::Palette,
) -> Element<'a, Message> {
    if menu.create {
        return create_menu_contents(desktop, menu, palette);
    }
    let ty = palette.typography;
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
    let prohibit_delete = desktop
        .application
        .workspace()
        .view(menu.view)
        .is_some_and(|view| view.settings.prohibit_delete);
    let trash_label = if prohibit_delete {
        "Deletion Prohibited"
    } else {
        "Move to Trash"
    };
    contents = contents
        .push(item(
            RENAME_ID,
            "entry-context-rename-icon",
            Icon::Rename,
            "Rename",
            Command::Rename,
        ))
        .push(item(
            CREATE_ID,
            "entry-context-create-icon",
            Icon::Document,
            "Create",
            Command::Create,
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
            Icon::Scissors,
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
        .push(details_menu_item(
            menu.generation,
            item(
                DETAILS_ID,
                "entry-context-details-icon",
                Icon::Info,
                "Details",
                Command::Details,
            ),
        ))
        .push(context_menu::divider(palette))
        .push(item(
            TRASH_ID,
            "entry-context-trash-icon",
            Icon::Trash,
            trash_label,
            Command::Trash,
        ));
    if let Some(guidance) = menu_guidance(desktop, menu, entry) {
        contents =
            contents.push(container(ty.small(guidance).color(palette.muted)).padding([4, 12]));
    }
    contents.into()
}

fn details_menu_item<'a>(generation: u64, row: Element<'a, Message>) -> Element<'a, Message> {
    mouse_area(row)
        .on_enter(Message::EntryMenuAction(
            generation,
            Command::DetailsHover(true),
        ))
        .on_exit(Message::EntryMenuAction(
            generation,
            Command::DetailsHover(false),
        ))
        .into()
}

fn create_menu_contents<'a>(
    desktop: &'a Desktop,
    menu: &'a Menu,
    palette: crate::window::appearance::Palette,
) -> Element<'a, Message> {
    let writable = desktop
        .application
        .can_write_folder(menu.view, &menu.listing.directory);
    let mut contents = column![context_menu::item(
        "entry-menu-back",
        "entry-menu-back-icon",
        Icon::Back,
        "Back",
        Some(Message::EntryMenuAction(menu.generation, Command::Back)),
        palette,
    )];
    for (kind, id) in CreateKind::ALL.into_iter().zip([
        "entry-create-folder",
        "entry-create-text",
        "entry-create-markdown",
        "entry-create-json",
    ]) {
        contents = contents.push(context_menu::item_with_suffix(
            id,
            format!("{id}-icon"),
            if kind == CreateKind::Folder {
                Icon::Folder
            } else {
                Icon::Document
            },
            kind.label(),
            kind.dot_extension(),
            writable.then_some(Message::EntryMenuAction(
                menu.generation,
                Command::New(kind),
            )),
            palette,
        ));
    }
    contents.into()
}

fn details_view<'a>(
    desktop: &'a Desktop,
    menu: &'a Menu,
    entry: &'a DirectoryEntry,
    rect: Rectangle,
    palette: crate::window::appearance::Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    let icon = Icon::for_entry(entry);
    let revision = desktop
        .views
        .get(&menu.view)
        .map_or(0, |state| state.listing_revision);
    let thumb_size = ty.icon(34.0);
    let box_size = ty.row(42.0);
    let preview: Element<'a, Message> = thumbnails::image(
        Some(&desktop.thumbnails.values),
        menu.view,
        revision,
        &entry.path,
        thumb_size,
    )
    .unwrap_or_else(|| icons::view(icon, ty.icon(24.0), Some(palette.icon(icon)), 1.0).into());
    let icon_badge = container(preview)
        .width(box_size)
        .height(box_size)
        .center_x(box_size)
        .center_y(box_size)
        .style(move |_| container::Style {
            background: Some(palette.row_hover.into()),
            border: Border {
                radius: 6.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        });
    let name = entry.name.to_string_lossy().into_owned();
    let header = container(
        row![
            icon_badge,
            ty.text(name)
                .font(ty.heading_font())
                .color(palette.text)
                .width(Fill)
                .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding([6, 12]);
    let meta = desktop.application.metadata(menu.view, &entry.path);
    let size_value = if entry.kind == EntryKind::Directory {
        folder_size_label(desktop, menu, entry)
    } else {
        metadata::size(meta)
    };
    let mut list = column![
        detail_row("Type", details_kind_label(entry), palette),
        detail_row("Size", size_value, palette),
    ]
    .spacing(4);
    if entry.kind == EntryKind::Directory {
        list = list.push(detail_row("Items", metadata::item_count(meta), palette));
    } else if crate::application::contracts::preview::image_candidate(&entry.path) {
        list = list.push(detail_row(
            "Dimensions",
            metadata::dimensions(meta),
            palette,
        ));
    }
    list = list
        .push(detail_row("Modified", metadata::date(meta), palette))
        .push(detail_row("Created", metadata::created_date(meta), palette));
    let rows = container(list).padding([4, 12]);
    let style = container::Style {
        background: Some(palette.contrast_background.into()),
        ..palette.panel()
    };
    container(column![header, context_menu::divider(palette), rows])
        .id(DETAILS_CARD_ID)
        .padding([6, 0])
        .width(rect.width)
        .height(rect.height)
        .style(move |_| container::Style {
            border: Border {
                radius: 8.0.into(),
                ..style.border
            },
            shadow: Shadow {
                color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.12),
                offset: Vector::new(0.0, 4.0),
                blur_radius: 14.0,
            },
            ..style
        })
        .into()
}

fn detail_row<'a>(
    label: &'static str,
    value: String,
    palette: crate::window::appearance::Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    row![
        ty.small(label).color(palette.muted).width(ty.row(76.0)),
        ty.small(value)
            .color(palette.text)
            .width(Fill)
            .wrapping(iced::widget::text::Wrapping::None)
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn details_kind_label(entry: &DirectoryEntry) -> String {
    if entry.symlink {
        return "Symbolic link".into();
    }
    match entry.kind {
        EntryKind::Directory => "Folder".into(),
        EntryKind::File => entry
            .path
            .extension()
            .and_then(|ext| ext.to_str())
            .filter(|ext| !ext.is_empty())
            .map_or_else(
                || "Document".into(),
                |ext| format!("{} file", ext.to_ascii_uppercase()),
            ),
        EntryKind::Other => "Special item".into(),
    }
}

fn folder_size_label(desktop: &Desktop, menu: &Menu, entry: &DirectoryEntry) -> String {
    let book = desktop.application.folder_usages();
    let cached = book.get(&entry.path).or_else(|| {
        menu.listing
            .entry_target(entry)
            .and_then(|target| book.get(target))
    });
    match cached {
        Some(Ok(estimate)) => format!(
            "{}{}",
            if estimate.partial { "≥" } else { "~" },
            metadata::decimal_size(estimate.bytes)
        ),
        Some(Err(_)) => "Unavailable".into(),
        None if book.is_pending(&menu.listing.directory) && !entry.symlink => "Calculating…".into(),
        None => "–".into(),
    }
}

fn menu_guidance(desktop: &Desktop, menu: &Menu, entry: &DirectoryEntry) -> Option<&'static str> {
    if !command_enabled(desktop, menu, Command::CopyPath) {
        Some("Path cannot be copied as text.")
    } else if entry.kind == EntryKind::Directory && !command_enabled(desktop, menu, Command::Open) {
        Some("Outside this view's scope.")
    } else if !command_enabled(desktop, menu, Command::Trash)
        && !desktop
            .application
            .workspace()
            .view(menu.view)
            .is_some_and(|view| view.settings.prohibit_delete)
    {
        if entry.kind == EntryKind::Directory
            && !entry.symlink
            && desktop.application.trash_guidance(menu.view, &menu.path)
                == Err(
                    "This folder is open in a box or used by a navigation scope. Navigate away or update the scope before moving it to Trash.",
                )
        {
            Some("Open in a box or scope.")
        } else if desktop
            .application
            .safety_policy(Some(menu.view))
            .check_trash(&menu.path)
            .is_err()
        {
            Some("Protected in Access and Safety.")
        } else {
            None
        }
    } else {
        None
    }
}

fn menu_bounds(desktop: &Desktop, menu: &Menu) -> iced::Rectangle {
    let ty = crate::window::typography::Style(desktop.application.typography());
    if menu.create {
        return context_menu::bounds(
            menu.position,
            desktop.size,
            ty.row(190.0),
            12.0 + 5.0 * ty.row(context_menu::ROW_HEIGHT),
        );
    }
    let entry = desktop.menu_listed_entry(menu);
    let rows = if entry.is_some_and(|entry| entry.kind != EntryKind::Other) {
        8.0
    } else {
        7.0
    };
    let guidance = entry.is_some_and(|entry| menu_guidance(desktop, menu, entry).is_some());
    let prohibit_delete = desktop
        .application
        .workspace()
        .view(menu.view)
        .is_some_and(|view| view.settings.prohibit_delete);
    let width = if guidance || prohibit_delete {
        ty.row(176.0)
    } else {
        ty.row(154.0)
    };
    context_menu::bounds(
        menu.position,
        desktop.size,
        width,
        30.0 + rows * ty.row(context_menu::ROW_HEIGHT) + if guidance { ty.row(28.0) } else { 0.0 },
    )
}

fn details_bounds(desktop: &Desktop, entry: &DirectoryEntry, menu_rect: Rectangle) -> Rectangle {
    let ty = crate::window::typography::Style(desktop.application.typography());
    let has_extra_row = entry.kind == EntryKind::Directory
        || crate::application::contracts::preview::image_candidate(&entry.path);
    let width = ty.row(228.0).min((desktop.size.width - 16.0).max(1.0));
    let raw_height = if has_extra_row { 180.0 } else { 158.0 };
    let height = ty
        .row(raw_height)
        .min((desktop.size.height - 16.0).max(1.0));
    let gap = 6.0;
    let right_x = menu_rect.x + menu_rect.width + gap;
    let x = if right_x + width <= desktop.size.width - 8.0 {
        right_x
    } else {
        (menu_rect.x - width - gap).max(8.0)
    };
    let y = (menu_rect.y + menu_rect.height - height)
        .clamp(8.0, (desktop.size.height - height - 8.0).max(8.0));
    Rectangle {
        x,
        y,
        width,
        height,
    }
}
