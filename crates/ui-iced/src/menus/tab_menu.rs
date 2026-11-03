//! One transient tab context menu. Closing uses the existing typed tab proposal.

use crate::application::Action;
use crate::domain::{BoxId, TabCommand, ViewId};
use crate::{Desktop, Message, boxes::tab_rail, menus::context_menu, window::icons};
use iced::widget::{column, container, space};
use iced::{Element, Point, Rectangle, Size, Task, keyboard};

pub(crate) const MENU_ID: &str = "tab-context-menu";
pub(crate) const CLOSE_ID: &str = "tab-context-close";
pub(crate) const CLOSE_ROW_ID: &str = "tab-context-close-row";
pub(crate) const CLOSE_ICON_ID: &str = "tab-context-close-icon";
const WIDTH: f32 = 190.0;

pub(crate) struct Menu {
    pub generation: u64,
    pub id: BoxId,
    pub view: ViewId,
    position: Point,
}

impl Desktop {
    fn permits_tab_menu(&self, id: BoxId, view: ViewId) -> bool {
        self.application.accepts_workspace_input()
            && !self.modal_open()
            && !self.application.tab_pending(id)
            && self
                .application
                .workspace()
                .can_change_tabs(id, TabCommand::Close(view))
            && self.application.workspace().boxes().iter().any(|item| {
                item.id == id
                    && item.tabs.as_ref().is_some_and(|tabs| {
                        tabs.visible() && tabs.views().iter().any(|tab| tab.view == view)
                    })
            })
    }

    pub(crate) fn open_tab_menu(
        &mut self,
        id: BoxId,
        view: ViewId,
        point: Option<Point>,
    ) -> Task<Message> {
        if self.context_menu_open() || !self.permits_tab_menu(id, view) {
            return Task::none();
        }
        let Some(generation) = self.next_tab_menu_generation.checked_add(1) else {
            return Task::none();
        };
        let Some(item) = self
            .application
            .workspace()
            .boxes()
            .iter()
            .find(|item| item.id == id)
        else {
            return Task::none();
        };
        let Some(index) = item
            .tabs
            .as_ref()
            .and_then(|tabs| tabs.views().iter().position(|tab| tab.view == view))
        else {
            return Task::none();
        };
        let geometry = self.geometry();
        let rect = geometry.rect(item.rect);
        let offset = tab_rail::offset(
            item,
            rect.height,
            self.tab_offsets.get(&id).copied().unwrap_or(0.0),
            geometry.typography,
        );
        let position = point.unwrap_or_else(|| {
            Point::new(
                rect.x + geometry.viewport.x - geometry.offset.x
                    + tab_rail::width(item, rect.width),
                rect.y + geometry.viewport.y - geometry.offset.y
                    + tab_rail::END_INSET
                    + index as f32 * geometry.typography.row(tab_rail::ROW_HEIGHT)
                    - offset,
            )
        });
        if !position.x.is_finite() || !position.y.is_finite() {
            return Task::none();
        }
        self.next_tab_menu_generation = generation;
        self.prepare_context_menu();
        self.tab_menu = Some(Menu {
            generation,
            id,
            view,
            position,
        });
        iced::widget::operation::focus(context_menu::opening_focus(point, CLOSE_ID))
    }

    pub(crate) fn dismiss_tab_menu(&mut self, generation: u64) -> Task<Message> {
        if !self
            .tab_menu
            .as_ref()
            .is_some_and(|menu| menu.generation == generation)
        {
            return Task::none();
        }
        let Some(menu) = self.tab_menu.take() else {
            return Task::none();
        };
        self.focus_version = self.focus_version.saturating_add(1);
        self.focus_tab(menu.view)
    }

    pub(crate) fn close_menu_tab(&mut self, generation: u64) -> Task<Message> {
        let Some(menu) = &self.tab_menu else {
            return Task::none();
        };
        if menu.generation != generation
            || !self.permits_tab_menu(menu.id, menu.view)
            || !self.menu_tab_can_close()
        {
            return Task::none();
        }
        let (id, view) = (menu.id, menu.view);
        let active = self
            .application
            .workspace()
            .boxes()
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.active_view().view);
        self.tab_menu = None;
        self.focus_version = self.focus_version.saturating_add(1);
        let task = self.dispatch(Action::ChangeTabs(id, TabCommand::Close(view)));
        Task::batch([
            task,
            active.map_or_else(Task::none, |active| self.focus_tab(active)),
        ])
    }

    pub(crate) fn reconcile_tab_menu(&mut self) {
        if self
            .tab_menu
            .as_ref()
            .is_some_and(|menu| !self.permits_tab_menu(menu.id, menu.view))
        {
            self.tab_menu = None;
        }
    }

    fn menu_tab_can_close(&self) -> bool {
        self.tab_menu.as_ref().is_some_and(|menu| {
            self.application.workspace().boxes().iter().any(|item| {
                item.id == menu.id
                    && item
                        .tabs
                        .as_ref()
                        .is_some_and(|tabs| tabs.views().len() > 1)
            })
        })
    }

    pub(crate) fn tab_menu_input(&mut self, event: iced::Event) -> Task<Message> {
        let Some(menu) = &self.tab_menu else {
            return Task::none();
        };
        match event {
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                repeat: false,
                ..
            }) => self.dismiss_tab_menu(menu.generation),
            _ => context_menu::input(event),
        }
    }
}

fn bounds(
    position: Point,
    size: Size,
    enabled: bool,
    ty: crate::window::typography::Style,
) -> Rectangle {
    let height = 12.0 + ty.row(context_menu::ROW_HEIGHT) + if enabled { 0.0 } else { ty.row(30.0) };
    context_menu::bounds(position, size, WIDTH, height)
}

pub(crate) fn view(desktop: &Desktop) -> Element<'_, Message> {
    let palette = desktop.box_palette();
    let ty = palette.typography;
    let Some(menu) = &desktop.tab_menu else {
        return space().into();
    };
    let enabled = desktop.menu_tab_can_close();
    let generation = menu.generation;
    let control = context_menu::item(
        CLOSE_ID,
        CLOSE_ICON_ID,
        icons::Icon::Close,
        "Close tab",
        enabled.then_some(Message::CloseMenuTab(generation)),
        palette,
    );
    let mut contents = column![container(control).id(CLOSE_ROW_ID)];
    if !enabled {
        contents = contents.push(
            container(ty.small("Keep at least one tab.").color(palette.muted)).padding([4, 12]),
        );
    }
    let rect = bounds(menu.position, desktop.size, enabled, ty);
    context_menu::surface(
        MENU_ID,
        contents,
        rect,
        Message::DismissTabMenu(generation),
        palette,
    )
}
