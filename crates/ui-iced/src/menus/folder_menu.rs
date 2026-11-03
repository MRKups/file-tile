//! Current-folder background menus with an in-place Create page.
use crate::{Desktop, Message, menus::context_menu, window::icons::Icon};
use crate::{
    application::LoadState,
    domain::{ViewId, files::CreateKind},
};
use iced::widget::{column, container, space};
use iced::{Element, Point, Task};
use std::sync::Arc;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Command {
    Paste,
    Move,
    Create,
    Back,
    New(CreateKind),
    CopyPath,
}
pub(crate) struct Menu {
    pub generation: u64,
    pub view: ViewId,
    listing: Arc<crate::application::contracts::directory::DirectoryListing>,
    position: Point,
    create: bool,
    in_preview: bool,
}

impl Desktop {
    pub(crate) fn open_folder_menu(&mut self, view: ViewId, point: Point) -> Task<Message> {
        self.open_folder_menu_for(view, false, point)
    }

    pub(crate) fn open_preview_folder_menu(&mut self, view: ViewId, point: Point) -> Task<Message> {
        self.open_folder_menu_for(view, true, point)
    }

    fn open_folder_menu_for(
        &mut self,
        view: ViewId,
        in_preview: bool,
        point: Point,
    ) -> Task<Message> {
        if !self.file_view_input_enabled(view)
            || !self.application.workspace().can_browse(view)
            || !point.x.is_finite()
            || !point.y.is_finite()
        {
            return Task::none();
        }
        let listing = if in_preview {
            self.column_previews.retained_listing(view)
        } else {
            match self.application.load(view) {
                Some(LoadState::Ready(listing)) => Some(Arc::clone(listing)),
                _ => None,
            }
        };
        let Some(listing) = listing else {
            self.views.entry(view).or_default().hint = Some(self.file_write_guidance(view).into());
            return Task::none();
        };
        let Some(generation) = self.next_folder_menu_generation.checked_add(1) else {
            return Task::none();
        };
        self.next_folder_menu_generation = generation;
        self.prepare_context_menu();
        if in_preview {
            self.column_previews.clear_selection(view);
            self.views.entry(view).or_default().hint = None;
        } else {
            let state = self.views.entry(view).or_default();
            state.selected = None;
            state.selection.clear();
            state.hint = None;
        }
        self.active_view = Some(view);
        self.folder_menu = Some(Menu {
            generation,
            view,
            listing,
            position: point,
            create: false,
            in_preview,
        });
        iced::widget::operation::focus("context-menu-no-selection")
    }

    pub(crate) fn folder_menu_valid(&self, menu: &Menu) -> bool {
        self.application.accepts_workspace_input()
            && !self.modal_open()
            && self.application.workspace().can_browse(menu.view)
            && if menu.in_preview {
                self.column_previews
                    .retained_listing(menu.view)
                    .is_some_and(|listing| Arc::ptr_eq(&listing, &menu.listing))
            } else {
                matches!(self.application.load(menu.view), Some(LoadState::Ready(listing)) if Arc::ptr_eq(listing, &menu.listing))
            }
    }

    pub(crate) fn folder_menu_action(
        &mut self,
        generation: u64,
        command: Command,
    ) -> Task<Message> {
        let Some(menu) = &self.folder_menu else {
            return Task::none();
        };
        if menu.generation != generation || !self.folder_menu_valid(menu) {
            return Task::none();
        }
        let page_command = matches!(
            (menu.create, command),
            (false, Command::Create) | (true, Command::Back)
        );
        if page_command {
            let Some(generation) = self.next_folder_menu_generation.checked_add(1) else {
                return Task::none();
            };
            self.next_folder_menu_generation = generation;
            if let Some(menu) = &mut self.folder_menu {
                menu.generation = generation;
                menu.create = matches!(command, Command::Create);
            }
            return iced::widget::operation::focus("context-menu-no-selection");
        }
        if menu.create != matches!(command, Command::New(_)) {
            return Task::none();
        }
        let view = menu.view;
        let directory = menu.listing.directory.clone();
        let anchor = menu_bounds(self, menu).position();
        if !matches!(command, Command::CopyPath)
            && !self.application.can_write_folder(view, &directory)
        {
            return Task::none();
        }
        self.folder_menu = None;
        self.anchor_file_prompt(view, anchor);
        match command {
            Command::Paste => self.paste_into_directory(view, directory),
            Command::Move => self.move_into_directory(view, directory),
            Command::New(kind) => self.start_name_in(view, directory, kind),
            Command::CopyPath => {
                if let Some(value) = directory.to_str() {
                    self.views.entry(view).or_default().hint =
                        crate::platform::clipboard::copy_text(value).err();
                }
                Task::none()
            }
            Command::Create | Command::Back => Task::none(),
        }
    }
}

pub(crate) fn target<'a>(
    view: ViewId,
    interactive: bool,
    content: Element<'a, Message>,
) -> Element<'a, Message> {
    context_menu::background(
        view,
        interactive.then(|| {
            Box::new(move |point| Message::OpenFolderMenu(view, point))
                as Box<dyn Fn(Point) -> Message>
        }),
        content,
    )
}

pub(crate) fn preview_target<'a>(
    view: ViewId,
    interactive: bool,
    content: Element<'a, Message>,
) -> Element<'a, Message> {
    context_menu::background_with_focus(
        view,
        Message::ColumnPreviewFocused(view),
        interactive.then(|| {
            Box::new(move |point| Message::OpenPreviewFolderMenu(view, point))
                as Box<dyn Fn(Point) -> Message>
        }),
        content,
    )
}

pub(crate) fn view(desktop: &Desktop) -> Element<'_, Message> {
    let Some(menu) = &desktop.folder_menu else {
        return space().into();
    };
    let palette = desktop.box_palette();
    let ty = palette.typography;
    let writable = desktop
        .application
        .can_write_folder(menu.view, &menu.listing.directory);
    let item = |id, icon, label, command, enabled: bool| {
        context_menu::item(
            id,
            format!("{id}-icon"),
            icon,
            label,
            enabled.then_some(Message::FolderMenuAction(menu.generation, command)),
            palette,
        )
    };
    let mut contents = if menu.create {
        let mut contents = column![item(
            "folder-menu-back",
            Icon::Back,
            "Back",
            Command::Back,
            true
        )];
        for (kind, id) in CreateKind::ALL.into_iter().zip([
            "create-folder",
            "create-text",
            "create-markdown",
            "create-json",
        ]) {
            contents = contents.push(item(
                id,
                if kind == CreateKind::Folder {
                    Icon::Folder
                } else {
                    Icon::Document
                },
                kind.label(),
                Command::New(kind),
                writable,
            ));
        }
        contents
    } else {
        column![
            item(
                "folder-menu-create",
                Icon::Document,
                "Create",
                Command::Create,
                writable
            ),
            context_menu::divider(palette),
            item(
                "folder-menu-paste",
                Icon::Copy,
                "Paste",
                Command::Paste,
                writable
            ),
            item(
                "folder-menu-move",
                Icon::Move,
                "Move here",
                Command::Move,
                writable
            ),
            item(
                "folder-menu-copy-path",
                Icon::Copy,
                "Copy path",
                Command::CopyPath,
                menu.listing.directory.to_str().is_some()
            )
        ]
    };
    if !writable {
        let guidance = desktop.file_write_guidance_for(menu.view, &menu.listing.directory);
        contents =
            contents.push(container(ty.small(guidance).color(palette.muted)).padding([4, 12]));
    }
    context_menu::surface(
        "folder-context-menu",
        contents,
        menu_bounds(desktop, menu),
        Message::DismissFolderMenu(menu.generation),
        palette,
    )
}

fn menu_bounds(desktop: &Desktop, menu: &Menu) -> iced::Rectangle {
    let ty = crate::window::typography::Style(desktop.application.typography());
    let writable = desktop
        .application
        .can_write_folder(menu.view, &menu.listing.directory);
    let height = 12.0
        + ty.row(context_menu::ROW_HEIGHT) * if menu.create { 5.0 } else { 4.0 }
        + if menu.create { 0.0 } else { 9.0 }
        + if writable { 0.0 } else { ty.row(48.0) };
    context_menu::bounds(menu.position, desktop.size, ty.row(210.0), height)
}
