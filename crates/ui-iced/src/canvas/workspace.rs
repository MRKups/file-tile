use iced::widget::{column, container, opaque, sensor, stack};
use iced::{Element, Fill};

use crate::{
    Desktop, Message, boxes::tab_drag, canvas::workspace_header, dialogs::app_settings,
    dialogs::box_settings, dialogs::storage_dialog, menus::file_drag, window::appearance as a,
};

/// Root composition only. Components receive borrowed state and emit typed messages.
pub(crate) fn view(desktop: &Desktop) -> Element<'_, Message> {
    let epoch = desktop.application.workspace_epoch();
    crate::runtime::workspace_epoch::view(
        epoch,
        crate::menus::context_menu::window_coordinates(
            desktop.context_menu_open(),
            surface(desktop),
        ),
    )
    .map(move |message| {
        if epoch.is_some() {
            Message::WorkspaceEpoch(epoch, Box::new(message))
        } else {
            message
        }
    })
}

fn surface(desktop: &Desktop) -> Element<'_, Message> {
    let geometry = desktop.geometry();
    let palettes =
        a::resolve(desktop.preview_appearance()).with_typography(desktop.application.typography());
    let background = desktop
        .window_appearance
        .background(palettes.window.background);
    // Iced may relayout during native resizing before delivering subscriptions.
    // Sensor publishes the logical size in that redraw, before content is drawn.
    let surface: Element<'_, Message> = sensor(
        container(column![
            workspace_header::view(
                &desktop.application,
                desktop.size.width,
                geometry.header_height,
                desktop.workspace_input_enabled() && desktop.application.file_operation().is_none(),
                desktop.application.can_configure_application() && !desktop.modal_open(),
                palettes.window
            ),
            container(super::surface::view(
                &desktop.application,
                &desktop.views,
                geometry,
                super::surface::Presentation {
                    tab_offsets: &desktop.tab_offsets,
                    tab_drag: desktop.tab_drag.as_ref(),
                    file_drag: &desktop.file_drag,
                    column_previews: &desktop.column_previews,
                    conflict_name: desktop
                        .conflict_name
                        .as_ref()
                        .map(|(_, _, name)| name.as_str()),
                    thumbnails: &desktop.thumbnails.values,
                    sort_menu: desktop.sort_menu_view(),
                },
                palettes
            ))
            .padding([0.0, geometry.side])
            .width(Fill)
            .height(Fill),
        ])
        .width(Fill)
        .height(Fill)
        .style(move |_| container::Style {
            background: Some(background.into()),
            text_color: Some(palettes.window.text),
            ..container::Style::default()
        }),
    )
    .on_resize(Message::ViewportLaidOut)
    .into();
    let inspector = if let Some(draft) = &desktop.launchers.draft {
        crate::boxes::launchers::editor(desktop, draft)
    } else {
        desktop.box_settings.as_ref().map_or_else(
            || iced::widget::space().into(),
            |draft| box_settings::view(desktop, draft),
        )
    };
    // Keep the canvas child at a stable slot when the inspector opens or closes.
    let surface: Element<'_, Message> = crate::widgets::input_boundary::view(
        desktop.modal_open() || desktop.context_menu_open(),
        stack![
            surface,
            tab_drag::view(desktop, palettes.boxes),
            file_drag::view(desktop, palettes.boxes),
            inspector
        ],
    );
    overlay(desktop, surface, palettes.boxes)
}

fn overlay<'a>(
    desktop: &'a Desktop,
    surface: Element<'a, Message>,
    palette: a::Palette,
) -> Element<'a, Message> {
    if desktop.storage_details && desktop.application.storage_detail().is_some() {
        stack![
            surface,
            opaque(storage_dialog::view(
                &desktop.application,
                desktop.startup_confirmation,
                &desktop.logs,
                palette,
            ))
        ]
        .into()
    } else if let Some(error) = desktop.native_menu.error() {
        stack![
            surface,
            opaque(crate::runtime::native_menu::error_view(error, palette))
        ]
        .into()
    } else if let Some(draft) = &desktop.workspace_management {
        stack![
            surface,
            opaque(crate::dialogs::workspace_management::view(
                &desktop.application,
                draft,
                desktop.size,
                palette,
            ))
        ]
        .into()
    } else if let Some(draft) = &desktop.app_settings {
        let settings = crate::widgets::input_boundary::view(
            desktop.about.is_some(),
            opaque(app_settings::view(desktop, draft)),
        );
        if let Some(state) = &desktop.about {
            stack![
                surface,
                settings,
                opaque(crate::dialogs::about::view(
                    state,
                    &desktop.updates,
                    desktop.size,
                    palette,
                ))
            ]
            .into()
        } else {
            stack![surface, settings, iced::widget::space()].into()
        }
    } else if let Some(state) = &desktop.about {
        stack![
            surface,
            opaque(crate::dialogs::about::view(
                state,
                &desktop.updates,
                desktop.size,
                palette,
            ))
        ]
        .into()
    } else if desktop.sort_menu.is_some() {
        stack![surface, crate::menus::sort_menu::view(desktop)].into()
    } else if desktop.tab_menu.is_some() {
        stack![surface, crate::menus::tab_menu::view(desktop)].into()
    } else if desktop.entry_menu.is_some() {
        stack![surface, crate::menus::entry_menu::view(desktop)].into()
    } else if desktop.folder_menu.is_some() {
        stack![surface, crate::menus::folder_menu::view(desktop)].into()
    } else {
        // Keep the background at the same slot when a popup opens or closes.
        stack![surface, iced::widget::space()].into()
    }
}
