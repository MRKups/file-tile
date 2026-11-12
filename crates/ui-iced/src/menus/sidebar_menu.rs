//! Context menu for top-section `Pinned Folders` rows in a box sidebar.

use std::path::{Path, PathBuf};

use iced::widget::{column, container, space};
use iced::{Element, Point, Rectangle, Size, Task, keyboard};

use crate::application::Action;
use crate::domain::{BoxId, Mode};
use crate::{Desktop, Message, boxes::sidebar, menus::context_menu, window::icons};

pub(crate) const MENU_ID: &str = "sidebar-context-menu";
pub(crate) const UNPIN_ID: &str = "sidebar-context-unpin";
pub(crate) const UNPIN_ROW_ID: &str = "sidebar-context-unpin-row";
pub(crate) const UNPIN_ICON_ID: &str = "sidebar-context-unpin-icon";
const WIDTH: f32 = 200.0;

pub(crate) struct Menu {
    pub generation: u64,
    pub id: BoxId,
    pub path: PathBuf,
    position: Point,
}

impl Desktop {
    fn permits_sidebar_menu(&self, id: BoxId, path: &Path) -> bool {
        let workspace = self.application.workspace();
        let mode_ok = workspace.mode() == Mode::Browse || workspace.configuring(id);
        self.application.accepts_workspace_input()
            && !self.modal_open()
            && mode_ok
            && self.application.is_pinned_folder(path)
            && workspace
                .boxes()
                .iter()
                .any(|item| item.id == id && item.has_sidebar())
    }

    pub(crate) fn open_sidebar_menu(
        &mut self,
        id: BoxId,
        path: PathBuf,
        point: Option<Point>,
    ) -> Task<Message> {
        if self.context_menu_open() || !self.permits_sidebar_menu(id, &path) {
            return Task::none();
        }
        let Some(generation) = self.next_sidebar_menu_generation.checked_add(1) else {
            return Task::none();
        };
        let Some(item) = self
            .application
            .workspace()
            .boxes()
            .iter()
            .find(|item| item.id == id && item.has_sidebar())
        else {
            return Task::none();
        };
        let Some(index) = self
            .application
            .pinned_folders()
            .iter()
            .position(|candidate| candidate == &path)
        else {
            return Task::none();
        };
        let geometry = self.geometry();
        let rect = geometry.rect(item.rect);
        let ty = geometry.typography;
        let stored = self.sidebar_offsets.get(&id).copied().unwrap_or(0.0);
        let scroll = sidebar::offset(
            self.application.pinned_folders().len(),
            self.application.locations().len(),
            rect.height,
            stored,
            ty,
        );
        let position = point.unwrap_or_else(|| {
            Point::new(
                rect.x + geometry.viewport.x - geometry.offset.x
                    + sidebar::rail_width(item, rect.width),
                rect.y + geometry.viewport.y - geometry.offset.y
                    + sidebar::END_INSET
                    + ty.row(sidebar::SECTION_HEADER_HEIGHT)
                    + index as f32 * ty.row(sidebar::ROW_HEIGHT)
                    - scroll,
            )
        });
        if !position.x.is_finite() || !position.y.is_finite() {
            return Task::none();
        }
        self.next_sidebar_menu_generation = generation;
        self.prepare_context_menu();
        self.sidebar_drag = None;
        self.sidebar_menu = Some(Menu {
            generation,
            id,
            path,
            position,
        });
        iced::widget::operation::focus(context_menu::opening_focus(point, UNPIN_ID))
    }

    pub(crate) fn dismiss_sidebar_menu(&mut self, generation: u64) -> Task<Message> {
        let Some(menu) = &self.sidebar_menu else {
            return Task::none();
        };
        if menu.generation != generation {
            return Task::none();
        }
        let id = menu.id;
        let path = menu.path.clone();
        self.sidebar_menu = None;
        self.focus_version = self.focus_version.saturating_add(1);
        self.restore_sidebar_menu_focus(id, Some(&path), 0)
    }

    pub(crate) fn unpin_sidebar_menu(&mut self, generation: u64) -> Task<Message> {
        let Some(menu) = &self.sidebar_menu else {
            return Task::none();
        };
        if menu.generation != generation || !self.permits_sidebar_menu(menu.id, &menu.path) {
            return Task::none();
        }
        let id = menu.id;
        let path = menu.path.clone();
        let prior_index = self
            .application
            .pinned_folders()
            .iter()
            .position(|candidate| candidate == &path)
            .unwrap_or(0);
        self.sidebar_menu = None;
        self.focus_version = self.focus_version.saturating_add(1);
        let task = self.dispatch(Action::UnpinFolder(path));
        Task::batch([task, self.restore_sidebar_menu_focus(id, None, prior_index)])
    }

    fn restore_sidebar_menu_focus(
        &self,
        id: BoxId,
        exact_path: Option<&Path>,
        fallback_index: usize,
    ) -> Task<Message> {
        let Some(item) = self
            .application
            .workspace()
            .boxes()
            .iter()
            .find(|item| item.id == id && item.has_sidebar())
        else {
            return iced::widget::operation::focus("workspace-canvas");
        };
        let pinned = self.application.pinned_folders();
        if let Some(path) = exact_path
            && let Some(index) = pinned.iter().position(|candidate| candidate == path)
        {
            return iced::widget::operation::focus(sidebar::pin_row_id(id, index));
        }
        if !pinned.is_empty() {
            let next = fallback_index.min(pinned.len() - 1);
            return iced::widget::operation::focus(sidebar::pin_row_id(id, next));
        }
        let scope = item.standalone.navigation.scope();
        let safety = self.application.safety_policy(Some(item.standalone.view));
        if let Some(loc_index) = self.application.locations().iter().position(|loc| {
            scope.allows(&loc.path) && safety.check_read_directory(&loc.path).is_ok()
        }) {
            return iced::widget::operation::focus(sidebar::location_row_id(id, loc_index));
        }
        iced::widget::operation::focus("workspace-canvas")
    }

    pub(crate) fn reconcile_sidebar_menu(&mut self) {
        if self
            .sidebar_menu
            .as_ref()
            .is_some_and(|menu| !self.permits_sidebar_menu(menu.id, &menu.path))
        {
            self.sidebar_menu = None;
        }
    }

    pub(crate) fn sidebar_menu_input(&mut self, event: iced::Event) -> Task<Message> {
        let Some(menu) = &self.sidebar_menu else {
            return Task::none();
        };
        match event {
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                repeat: false,
                ..
            }) => self.dismiss_sidebar_menu(menu.generation),
            _ => context_menu::input(event),
        }
    }
}

fn bounds(position: Point, size: Size, ty: crate::window::typography::Style) -> Rectangle {
    let height = 12.0 + ty.row(context_menu::ROW_HEIGHT);
    context_menu::bounds(position, size, ty.row(WIDTH), height)
}

pub(crate) fn view(desktop: &Desktop) -> Element<'_, Message> {
    let palette = desktop.box_palette();
    let ty = palette.typography;
    let Some(menu) = &desktop.sidebar_menu else {
        return space().into();
    };
    let generation = menu.generation;
    let control = context_menu::item(
        UNPIN_ID,
        UNPIN_ICON_ID,
        icons::Icon::Pin,
        "Unpin from sidebar",
        Some(Message::UnpinSidebarMenu(generation)),
        palette,
    );
    let contents = column![container(control).id(UNPIN_ROW_ID)];
    let rect = bounds(menu.position, desktop.size, ty);
    context_menu::surface(
        MENU_ID,
        contents,
        rect,
        Message::DismissSidebarMenu(generation),
        palette,
    )
}
