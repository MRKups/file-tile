//! Header-anchored sorting and visibility. Accepted settings live in the workspace.
use crate::{
    Desktop, Message, menus::context_menu, widgets::focus_control, widgets::icon_button,
    window::appearance as a, window::icons::Icon,
};
use crate::{
    application::{Action, LoadState},
    domain::{
        BoxSetting, Mode, ViewId,
        sorting::{SortDirection, SortField, SortSettings},
    },
};
use iced::{
    Element, Fill, Rectangle, Task,
    widget::{button, column, container, operation, row, scrollable, space},
};
use std::sync::Arc;
mod placement;

pub(crate) fn input(event: iced::Event) -> Task<Message> {
    context_menu::input(event).chain(placement::reveal())
}

const MENU_ID: &str = "sort-menu";
const WIDTH: f32 = 230.0;
pub(crate) fn anchor_id(view: ViewId) -> String {
    format!("sort-anchor-{}", view.0)
}
fn focus_id(view: ViewId) -> String {
    icon_button::focus_id(Icon::Sort, &Message::ToggleSort(view))
}
pub(crate) struct Menu {
    generation: u64,
    view: ViewId,
    mode: Mode,
    listing: Arc<crate::application::contracts::directory::DirectoryListing>,
    anchor: Option<Rectangle>,
}
#[derive(Debug, Clone, Copy)]
pub(crate) enum Choice {
    Field(SortField),
    Direction(SortDirection),
    Folders,
    HiddenFiles,
}
impl Desktop {
    fn sort_menu_valid(&self, menu: &Menu) -> bool {
        self.application.accepts_workspace_input()
            && !self.modal_open()
            && self.application.workspace().mode() == menu.mode
            && self.application.workspace().can_browse(menu.view)
            && !self
                .views
                .get(&menu.view)
                .is_some_and(|state| state.name_draft.is_some())
            && !self
                .application
                .file_operation()
                .is_some_and(|op| op.view == menu.view)
            && matches!(self.application.load(menu.view),Some(LoadState::Ready(listing)) if Arc::ptr_eq(listing,&menu.listing))
    }
    pub(crate) fn reconcile_sort_menu(&mut self) {
        if self
            .sort_menu
            .as_ref()
            .is_some_and(|menu| !self.sort_menu_valid(menu))
        {
            self.sort_menu = None;
        }
    }
    pub(crate) fn sort_menu_view(&self) -> Option<ViewId> {
        self.sort_menu.as_ref().map(|menu| menu.view)
    }
    pub(crate) fn sort_menu_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ToggleSort(view) => self.toggle_sort(view),
            Message::SortAnchor(generation, bounds) => {
                if let Some(menu) = &mut self.sort_menu
                    && menu.generation == generation
                {
                    menu.anchor = bounds;
                }
                Task::none()
            }
            Message::DismissSort => self.dismiss_sort(false),
            Message::SortChoice(generation, choice) => self.choose_sort(generation, choice),
            _ => Task::none(),
        }
    }
    fn toggle_sort(&mut self, view: ViewId) -> Task<Message> {
        if self
            .sort_menu
            .as_ref()
            .is_some_and(|menu| menu.view == view)
        {
            return self.dismiss_sort(false);
        }
        if !self.file_view_input_enabled(view) {
            return Task::none();
        }
        let Some(LoadState::Ready(listing)) = self.application.load(view) else {
            return Task::none();
        };
        let Some(generation) = self.next_sort_menu_generation.checked_add(1) else {
            return Task::none();
        };
        let listing = Arc::clone(listing);
        let mode = self.application.workspace().mode();
        self.clear_context_menus();
        self.prepare_context_menu();
        self.active_view = Some(view);
        self.next_sort_menu_generation = generation;
        self.sort_menu = Some(Menu {
            generation,
            view,
            mode,
            listing,
            anchor: None,
        });
        placement::anchor(view, generation)
            .chain(operation::focus("sort-name"))
            .chain(placement::reveal())
    }
    pub(crate) fn dismiss_sort(&mut self, restore_keyboard_focus: bool) -> Task<Message> {
        let Some(menu) = self.sort_menu.take() else {
            return Task::none();
        };
        self.focus_version = self.focus_version.saturating_add(1);
        if restore_keyboard_focus && self.sort_menu_valid(&menu) {
            operation::focus(focus_id(menu.view))
        } else {
            operation::focus("context-menu-no-selection")
        }
    }
    fn choose_sort(&mut self, generation: u64, choice: Choice) -> Task<Message> {
        let Some(menu) = self.sort_menu.take() else {
            return Task::none();
        };
        if menu.generation != generation || !self.sort_menu_valid(&menu) {
            self.sort_menu = Some(menu);
            self.reconcile_sort_menu();
            return Task::none();
        }
        let Some(item) = self.application.workspace().view(menu.view) else {
            return Task::none();
        };
        let mut settings = item.settings.sort;
        let action = match choice {
            Choice::Field(field) => {
                if settings.field != field {
                    settings.field = field;
                    settings.direction = if matches!(field, SortField::Modified | SortField::Size) {
                        SortDirection::Descending
                    } else {
                        SortDirection::Ascending
                    };
                }
                Action::SetSort(menu.view, settings)
            }
            Choice::Direction(direction) => {
                settings.direction = direction;
                Action::SetSort(menu.view, settings)
            }
            Choice::Folders => {
                settings.folders_first = !settings.folders_first;
                Action::SetSort(menu.view, settings)
            }
            Choice::HiddenFiles => {
                Action::SetBoxSetting(item.id, BoxSetting::HiddenFiles(!item.settings.show_hidden))
            }
        };
        let task = self.dispatch(action);
        if self.sort_menu_valid(&menu) {
            self.sort_menu = Some(menu);
        }
        task
    }
}
pub(crate) fn control(
    view: ViewId,
    settings: SortSettings,
    open: bool,
    interactive: bool,
    palette: a::Palette,
) -> Element<'static, Message> {
    container(icon_button::themed(
        Icon::Sort,
        format!(
            "Sort: {}, {}",
            settings.field.label(),
            settings.field.order_label(settings.direction)
        ),
        interactive.then_some(Message::ToggleSort(view)),
        if open {
            icon_button::Treatment::NavigationSelected
        } else {
            icon_button::Treatment::Navigation
        },
        palette,
    ))
    .id(anchor_id(view))
    .into()
}
pub(crate) fn view(desktop: &Desktop) -> Element<'_, Message> {
    let Some(menu) = &desktop.sort_menu else {
        return space().into();
    };
    let Some(anchor) = menu.anchor else {
        return space().into();
    };
    let Some(item) = desktop.application.workspace().view(menu.view) else {
        return space().into();
    };
    let settings = item.settings.sort;
    let palette = desktop.box_palette();
    let ty = palette.typography;
    let mut contents = column![
        container(
            ty.small("Sort by")
                .font(ty.heading_font())
                .color(palette.text)
        )
        .padding([0, 12])
        .width(Fill)
        .center_y(ty.row(24.0))
    ]
    .spacing(1);
    for (field, id) in
        SortField::ALL
            .into_iter()
            .zip(["sort-name", "sort-modified", "sort-size", "sort-type"])
    {
        contents = contents.push(choice_row(
            id,
            field.label(),
            settings.field == field,
            Message::SortChoice(menu.generation, Choice::Field(field)),
            palette,
        ));
    }
    contents = contents.push(context_menu::divider(palette));
    for (direction, id) in [
        (SortDirection::Ascending, "sort-ascending"),
        (SortDirection::Descending, "sort-descending"),
    ] {
        contents = contents.push(choice_row(
            id,
            settings.field.order_label(direction),
            settings.direction == direction,
            Message::SortChoice(menu.generation, Choice::Direction(direction)),
            palette,
        ));
    }
    contents = contents
        .push(context_menu::divider(palette))
        .push(choice_row(
            "sort-folders",
            "Folders first",
            settings.folders_first,
            Message::SortChoice(menu.generation, Choice::Folders),
            palette,
        ))
        .push(choice_row(
            "sort-hidden",
            "Hidden Files",
            item.settings.show_hidden,
            Message::SortChoice(menu.generation, Choice::HiddenFiles),
            palette,
        ));
    // Eight choice rows, a bounded title, two 9px dividers, ten 1px gaps,
    // and the surface padding. Keep the typography-dependent parts scaled.
    let rect = context_menu::bounds(
        iced::Point::new(
            anchor.x + anchor.width - WIDTH,
            anchor.y + anchor.height + 4.0,
        ),
        desktop.size,
        WIDTH,
        ty.row(context_menu::ROW_HEIGHT) * 8.0
            + ty.row(24.0)
            + 28.0
            + context_menu::SURFACE_PADDING * 2.0,
    );
    context_menu::styled_surface(
        MENU_ID,
        a::scroll_area(contents)
            .id(placement::SCROLL_ID)
            .height(Fill)
            .direction(scrollable::Direction::Vertical(a::scrollbar()))
            .style(palette.scrollable()),
        rect,
        Message::DismissSort,
        iced::widget::container::Style {
            background: Some(palette.contrast_background.into()),
            ..palette.panel()
        },
    )
}
fn choice_row<'a>(
    id: &'static str,
    label: &'static str,
    selected: bool,
    action: Message,
    palette: a::Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    focus_control::themed(
        id,
        action.clone(),
        button(
            container(
                row![
                    ty.text(if selected { "✓" } else { "" }).width(18),
                    ty.text(label)
                ]
                .spacing(6)
                .align_y(iced::Center),
            )
            .width(Fill)
            .center_y(Fill),
        )
        .padding([0, 12])
        .height(ty.row(context_menu::ROW_HEIGHT))
        .width(Fill)
        .on_press(action)
        .style(move |_, status| button::Style {
            text_color: palette.text,
            border: iced::Border {
                radius: 6.0.into(),
                ..iced::Border::default()
            },
            ..palette.text_button(status)
        }),
        palette,
    )
}
