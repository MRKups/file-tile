//! Configure card rendering, section controls, and keyboard-accessible pickers.

use iced::widget::{
    button, column, container, mouse_area, opaque, pick_list, pin, row, rule, scrollable, space,
    tooltip,
};
use iced::{Center, Element, Fill, Padding, mouse};

use crate::application::Action;
use crate::domain::{BoxId, BoxSetting};
use crate::{
    Desktop, Message,
    widgets::{focus_control, toggle_switch::toggle_switch},
    window::appearance as a,
};

use super::{
    CARD_ID, CLOSE_ID, Draft, FIELDS_ID, FILE_VIEW_VALUE_ID, FORWARD_ID, HANDLE_ID,
    NAVIGATION_VALUE_ID, PROHIBIT_DELETE_ID, ROW_HEIGHT, START_ID, TABS_ID, UP_ID, selection,
};

pub(crate) fn view<'a>(desktop: &'a Desktop, draft: &'a Draft) -> Element<'a, Message> {
    let Some(item) = desktop.application.workspace().view(draft.view) else {
        return space().into();
    };
    let rect = draft.bounds(desktop);
    let number = desktop
        .application
        .workspace()
        .boxes()
        .iter()
        .position(|item| item.id == draft.id)
        .map_or(1, |index| index + 1);
    let palette = desktop.configure_palette();
    let header = header(number, palette);
    let body = if draft.range_page {
        crate::dialogs::range_editor::page(desktop, draft)
    } else {
        let view = draft.view;
        a::scroll_area(container(fields(desktop, item, draft)).padding(Padding {
            left: 26.0,
            right: 30.0,
            ..Padding::ZERO
        }))
        .id(FIELDS_ID)
        .on_scroll(move |viewport| Message::BoxFormScrolled(view, viewport.absolute_offset().y))
        .direction(scrollable::Direction::Vertical(a::scrollbar()))
        .style(palette.scrollable())
        .height(Fill)
        .into()
    };
    let panel = container(
        column![
            container(header).padding(Padding {
                left: 26.0,
                right: 26.0,
                ..Padding::ZERO
            }),
            container(body).padding(Padding {
                right: 10.0,
                ..Padding::ZERO
            })
        ]
        .spacing(20),
    )
    .id(CARD_ID)
    .padding(Padding {
        top: 24.0,
        right: 0.0,
        bottom: 26.0,
        left: 0.0,
    })
    .width(rect.width)
    .height(rect.height)
    .style(move |_| palette.inspector_panel());
    pin(opaque(panel)).x(rect.x).y(rect.y).into()
}

fn header(number: usize, palette: a::Palette) -> Element<'static, Message> {
    let ty = palette.typography;
    row![
        mouse_area(
            container(
                column![
                    ty.caption("CONFIGURE")
                        .font(ty.heading_font())
                        .color(palette.muted),
                    ty.heading(format!("Box {number:02}"))
                ]
                .spacing(5)
            )
            .width(Fill)
            .height(ty.row(44.0))
            .id(HANDLE_ID)
        )
        .interaction(mouse::Interaction::Grab)
        .on_press(Message::BeginCardDrag)
        .on_release(Message::EndCardDrag),
        container(
            tooltip(
                focus_control::themed(
                    CLOSE_ID,
                    Message::Action(Action::ConfigureBox(None)),
                    button(
                        container(ty.text("×").size(ty.size(a::TITLE_SIZE)))
                            .center_x(ty.row(26.0))
                            .center_y(ty.row(26.0)),
                    )
                    .padding(0)
                    .width(ty.row(26.0))
                    .height(ty.row(26.0))
                    .style(move |_, status| palette.text_button(status))
                    .on_press(Message::Action(Action::ConfigureBox(None))),
                    palette
                ),
                ty.small("Close settings"),
                tooltip::Position::Bottom
            )
            .style(move |_| palette.tooltip())
        )
        .id(CLOSE_ID),
    ]
    .align_y(iced::Top)
    .spacing(12)
    .into()
}

fn fields<'a>(
    desktop: &'a Desktop,
    item: crate::domain::ViewRef<'a>,
    draft: &'a Draft,
) -> Element<'a, Message> {
    let (editor, feedback) = selection::folder_editor(desktop, item, draft);
    let palette = desktop.configure_palette();
    let ty = palette.typography;
    let id = draft.id;
    let settings = item.settings;
    let inline_details = settings.file_view != crate::domain::FileView::Grid;
    let options = iced::widget::Column::new()
        .push(scope_control(item, palette))
        .push(
            ty.caption(item.navigation.scope().kind().explanation())
                .color(palette.muted),
        )
        .push(section_divider(palette))
        .push(toggle(
            id,
            FORWARD_ID,
            "Forward button",
            settings.show_forward,
            BoxSetting::Forward,
            palette,
        ))
        .push(toggle(
            id,
            UP_ID,
            "Up button",
            settings.show_up,
            BoxSetting::Up,
            palette,
        ))
        .push(toggle(
            id,
            START_ID,
            "Return to start button",
            settings.show_start,
            BoxSetting::ReturnToStart,
            palette,
        ))
        .push(section_divider(palette))
        .push(file_view_control(item, palette))
        .push(toggle_enabled(
            id,
            "box-sizes",
            "File sizes",
            settings.show_sizes,
            inline_details,
            BoxSetting::FileSizes,
            palette,
        ))
        .push(toggle_enabled(
            id,
            "box-dates",
            "Modified dates",
            settings.show_dates,
            inline_details,
            BoxSetting::ModifiedDates,
            palette,
        ))
        .push(section_divider(palette))
        .push(toggle(
            id,
            PROHIBIT_DELETE_ID,
            "Prohibit deletion",
            settings.prohibit_delete,
            BoxSetting::ProhibitDelete,
            palette,
        ));
    // Feedback changes do not move focused controls into another widget slot.
    column![
        tabs_control(desktop, id),
        container(editor).padding(Padding {
            bottom: 20.0,
            ..Padding::ZERO
        }),
        feedback,
        options
    ]
    .into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TabChoice {
    None,
    Vertical,
}
impl std::fmt::Display for TabChoice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::None => "None",
            Self::Vertical => "Vertical",
        })
    }
}
fn tabs_control(desktop: &Desktop, id: BoxId) -> Element<'_, Message> {
    let palette = desktop.configure_palette();
    let ty = palette.typography;
    let visible = desktop
        .application
        .workspace()
        .boxes()
        .iter()
        .find(|item| item.id == id)
        .is_some_and(|item| item.tabs.as_ref().is_some_and(|tabs| tabs.visible()));
    let select = pick_list(
        [TabChoice::None, TabChoice::Vertical],
        Some(if visible {
            TabChoice::Vertical
        } else {
            TabChoice::None
        }),
        move |choice| {
            Message::Action(Action::ChangeTabs(
                id,
                crate::domain::TabCommand::Visible(choice == TabChoice::Vertical),
            ))
        },
    )
    .width(155)
    .padding([8, 12])
    .text_size(ty.size(a::CONTROL_SIZE))
    .font(ty.font())
    .style(move |_, status| palette.inspector_picker(status))
    .menu_style(move |_| palette.picker_menu());
    let action = Message::Action(Action::ChangeTabs(
        id,
        crate::domain::TabCommand::Visible(!visible),
    ));
    let keys = vec![
        (
            iced::keyboard::key::Named::ArrowUp,
            iced::keyboard::Modifiers::empty(),
            Message::Action(Action::ChangeTabs(
                id,
                crate::domain::TabCommand::Visible(false),
            )),
        ),
        (
            iced::keyboard::key::Named::ArrowDown,
            iced::keyboard::Modifiers::empty(),
            Message::Action(Action::ChangeTabs(
                id,
                crate::domain::TabCommand::Visible(true),
            )),
        ),
    ];
    let mut fields = column![
        container(
            row![
                ty.control("Tabs").width(Fill),
                container(focus_control::themed_highlight_keys(
                    TABS_ID,
                    action,
                    a::picker(palette, select),
                    keys,
                    palette
                ))
                .id(TABS_ID)
            ]
            .spacing(12)
            .align_y(Center)
        )
        .height(ty.row(ROW_HEIGHT))
        .align_y(Center)
    ];
    if let Some(error) = desktop.application.tab_error(id) {
        fields = fields.push(ty.caption(error).color(palette.error));
    }
    if desktop.application.tab_pending(id) {
        fields = fields.push(ty.caption("Checking tab change…").color(palette.muted));
    }
    container(fields)
        .padding(Padding {
            bottom: 20.0,
            ..Padding::ZERO
        })
        .into()
}

fn scope_control(
    item: crate::domain::ViewRef<'_>,
    palette: a::Palette,
) -> Element<'static, Message> {
    use crate::domain::ScopeKind;
    let ty = palette.typography;
    let view = item.view;
    let kind = item.navigation.scope().kind();
    let action = move |kind| {
        Message::Action(Action::EditScope(
            view,
            crate::application::workspace::scopes::ScopeEdit::Select(kind),
        ))
    };
    let index = ScopeKind::ALL
        .iter()
        .position(|candidate| *candidate == kind)
        .unwrap_or(0);
    let keys = vec![
        (
            iced::keyboard::key::Named::ArrowUp,
            iced::keyboard::Modifiers::empty(),
            action(ScopeKind::ALL[index.saturating_sub(1)]),
        ),
        (
            iced::keyboard::key::Named::ArrowDown,
            iced::keyboard::Modifiers::empty(),
            action(ScopeKind::ALL[(index + 1).min(3)]),
        ),
    ];
    let picker = pick_list(ScopeKind::ALL, Some(kind), action)
        .width(155)
        .padding([8, 12])
        .text_size(ty.size(a::CONTROL_SIZE))
        .font(ty.font())
        .style(move |_, status| palette.inspector_picker(status))
        .menu_style(move |_| palette.picker_menu());
    container(
        row![
            ty.control("Navigation").width(Fill),
            container(focus_control::themed_highlight_keys(
                NAVIGATION_VALUE_ID,
                action(ScopeKind::ALL[(index + 1) % 4]),
                a::picker(palette, picker),
                keys,
                palette
            ))
            .id(NAVIGATION_VALUE_ID)
        ]
        .spacing(12)
        .align_y(Center),
    )
    .height(ty.row(ROW_HEIGHT))
    .align_y(Center)
    .into()
}

fn file_view_control(
    item: crate::domain::ViewRef<'_>,
    palette: a::Palette,
) -> Element<'static, Message> {
    use crate::domain::FileView;
    let ty = palette.typography;
    let id = item.id;
    let value = item.settings.file_view;
    let action = move |view| Message::Action(Action::SetBoxSetting(id, BoxSetting::FileView(view)));
    let index = FileView::ALL
        .iter()
        .position(|candidate| *candidate == value)
        .unwrap_or(0);
    let keys = vec![
        (
            iced::keyboard::key::Named::ArrowUp,
            iced::keyboard::Modifiers::empty(),
            action(FileView::ALL[index.saturating_sub(1)]),
        ),
        (
            iced::keyboard::key::Named::ArrowDown,
            iced::keyboard::Modifiers::empty(),
            action(FileView::ALL[(index + 1).min(2)]),
        ),
    ];
    let picker = pick_list(FileView::ALL, Some(value), action)
        .width(155)
        .padding([8, 12])
        .text_size(ty.size(a::CONTROL_SIZE))
        .font(ty.font())
        .style(move |_, status| palette.inspector_picker(status))
        .menu_style(move |_| palette.picker_menu());
    container(
        row![
            ty.control("File view").width(Fill),
            container(focus_control::themed_highlight_keys(
                FILE_VIEW_VALUE_ID,
                action(FileView::ALL[(index + 1) % 3]),
                a::picker(palette, picker),
                keys,
                palette
            ))
            .id(FILE_VIEW_VALUE_ID)
        ]
        .spacing(12)
        .align_y(Center),
    )
    .height(ty.row(ROW_HEIGHT))
    .align_y(Center)
    .into()
}

fn section_divider(palette: a::Palette) -> Element<'static, Message> {
    container(rule::horizontal(1).style(move |_| palette.rule()))
        .padding(Padding {
            top: 17.0,
            bottom: 9.0,
            ..Padding::ZERO
        })
        .into()
}

fn toggle(
    id: BoxId,
    control: &'static str,
    label: &'static str,
    checked: bool,
    setting: fn(bool) -> BoxSetting,
    palette: a::Palette,
) -> Element<'static, Message> {
    toggle_enabled(id, control, label, checked, true, setting, palette)
}

fn toggle_enabled(
    id: BoxId,
    control: &'static str,
    label: &'static str,
    checked: bool,
    enabled: bool,
    setting: fn(bool) -> BoxSetting,
    palette: a::Palette,
) -> Element<'static, Message> {
    let ty = palette.typography;
    let shown = enabled && checked;
    let row_cell = container(
        row![
            ty.control(label)
                .color(if enabled { palette.text } else { palette.muted })
                .width(Fill),
            toggle_switch(shown, palette)
                .identity(id.0)
                .on_toggle_maybe(enabled.then_some(move |value| {
                    Message::Action(Action::SetBoxSetting(id, setting(value)))
                })),
        ]
        .spacing(12)
        .align_y(Center),
    )
    .id(control)
    .height(ty.row(ROW_HEIGHT))
    .align_y(Center);
    let next = enabled.then(|| Message::Action(Action::SetBoxSetting(id, setting(!shown))));
    let body: Element<'static, Message> = if let Some(press) = next.clone() {
        mouse_area(row_cell)
            .on_press(press)
            .interaction(mouse::Interaction::Pointer)
            .into()
    } else {
        row_cell.into()
    };
    focus_control::themed_optional(control, next, body, palette)
}
