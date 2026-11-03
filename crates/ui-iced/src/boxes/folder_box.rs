use std::path::Path;

use iced::widget::{Stack, column, container, mouse_area, row, scrollable, space};
use iced::{Element, Fill, Rectangle};

use crate::application::{Action, LoadState};
use crate::domain::FolderBox;

use crate::{
    Message, ViewPresentation, boxes::file_list, boxes::folder_status, boxes::navigation_header,
    boxes::tab_rail, canvas::geometry::CanvasGeometry, widgets::retained_view,
    window::appearance as a,
};

pub struct Props<'a> {
    pub order: Option<&'a [usize]>,
    pub order_revision: u64,
    pub sort_status: Option<&'a str>,
    pub usage: Option<
        &'a Result<
            crate::application::files::folder_usage::Estimate,
            crate::application::files::folder_usage::UsageError,
        >,
    >,
    pub folder_usages: &'a crate::application::files::folder_usage::UsageBook,
    pub palette: a::Palette,
    pub window_palette: a::Palette,
    pub metadata_paths: Option<&'a [std::path::PathBuf]>,
    pub metadata: Option<&'a crate::runtime::metadata::Values>,
    pub item: &'a FolderBox,
    pub load: Option<&'a LoadState>,
    pub presentation: Option<&'a ViewPresentation>,
    pub operation: Option<&'a crate::application::files::file_operations::Operation>,
    pub conflict_name: Option<&'a str>,
    pub files_closing: bool,
    pub thumbnails: Option<&'a crate::runtime::thumbnails::Values>,
    pub home: &'a Path,
    pub geometry: CanvasGeometry,
    pub interactive: bool,
    pub dimmed: bool,
    pub watch: Option<&'a crate::application::contracts::watching::WatchState>,
    pub mode: crate::domain::Mode,
    pub tabs_input: bool,
    pub tab_error: Option<&'a str>,
    pub opening_status: Option<&'a str>,
    pub dragged_tab: Option<crate::domain::ViewId>,
    pub sort_open: bool,
    pub dragging: Option<&'a std::collections::HashSet<std::path::PathBuf>>,
    pub column_preview: Option<&'a crate::boxes::file_columns::ViewPreview>,
}

/// Normal boxes pin header/status and scroll only entries. Tiny boxes scroll the
/// whole composition instead of clipping navigation and recovery controls away.
pub(crate) fn is_compact(
    width: f32,
    height: f32,
    settings: crate::domain::BoxSettings,
    ty: crate::window::typography::Style,
) -> bool {
    let header = navigation_header::height(width, settings, ty);
    height < header + ty.row(34.0) * 2.0
}

pub fn view(mut props: Props<'_>) -> Element<'_, Message> {
    if props.operation.is_some()
        || props
            .presentation
            .is_some_and(|state| state.name_draft.is_some() || state.trash_draft.is_some())
    {
        props.interactive = false;
        props.tabs_input = false;
    }
    let palette = props.palette;
    let window_palette = props.window_palette;
    let active = props.item.active_view();
    let box_rect = props.geometry.rect(props.item.rect);
    let rail_width = tab_rail::width(props.item, box_rect.width);
    let rect = Rectangle {
        x: box_rect.x + rail_width,
        width: (box_rect.width - rail_width).max(1.0),
        ..box_rect
    };
    let compact = is_compact(
        rect.width,
        rect.height,
        props.item.settings,
        palette.typography,
    ) || rect.width < navigation_header::minimum_width(props.item.settings);
    let content_width = if compact {
        rect.width
            .max(navigation_header::minimum_width(props.item.settings))
    } else {
        rect.width
    };
    let body = contents(&props, rect, compact, content_width);
    let body = retained_view::view(
        active.view,
        props.item.views().map(|view| view.view).collect(),
        body,
    );
    let body = with_tab_rail(&props, rail_width, body);
    let mut shield = mouse_area(container(space()).width(Fill).height(Fill));
    if !props.interactive {
        shield = shield.on_press(Message::Action(Action::BeginDraw(
            props.item.rect.origin,
            props.geometry.extent,
        )));
    }
    let prompt = crate::menus::file_prompt::view(
        active.view,
        props.presentation,
        props.operation,
        props.conflict_name,
        props.files_closing,
        palette,
        box_rect.height,
    );
    let box_seed = props.item.id.0;
    let mut layers = Stack::new();
    if let Some(texture) = a::surface_texture::overlay(palette, box_seed) {
        layers = layers.push(texture);
    }
    layers = layers.push(crate::widgets::input_boundary::view(prompt.is_some(), body));
    let has_prompt = prompt.is_some();
    if let Some(prompt) = prompt {
        layers = layers.push(
            container(space())
                .id(crate::menus::file_prompt::BLOCK_ID)
                .width(Fill)
                .height(Fill),
        );
        layers = layers.push(crate::menus::file_prompt::anchored(
            props.presentation.and_then(|state| state.prompt_anchor),
            prompt,
        ));
    }
    if props.dimmed {
        layers = layers.push(
            container(space())
                .width(Fill)
                .height(Fill)
                .style(move |_| window_palette.configure_scrim()),
        );
    }
    let layers = if has_prompt {
        layers
    } else {
        layers.push(shield)
    };
    container(layers.width(Fill).height(Fill))
        .width(box_rect.width)
        .height(box_rect.height)
        .clip(true)
        .style(move |_| a::surface_texture::box_panel(palette, box_seed))
        .into()
}

fn with_tab_rail<'a>(
    props: &Props<'a>,
    width: f32,
    body: Element<'a, Message>,
) -> Element<'a, Message> {
    let rail: Element<'a, Message> = if width > 0.0 {
        tab_rail::view(tab_rail::Props {
            palette: props.palette,
            item: props.item,
            home: props.home,
            mode: props.mode,
            interactive: props.tabs_input,
            width,
            dragged: props.dragged_tab,
        })
    } else {
        space().width(0).into()
    };
    // Keep the retained body at the same child slot when the rail is hidden.
    row![rail, body].width(Fill).height(Fill).into()
}

fn status<'a>(props: &Props<'a>) -> Element<'a, Message> {
    folder_status::view(
        props.load,
        props
            .tab_error
            .or(props.opening_status)
            .or_else(|| props.presentation.and_then(|state| state.hint.as_deref())),
        props.watch,
        props.item.settings.show_hidden,
        props.palette,
        props.sort_status,
        props.usage,
    )
}

fn contents<'a>(
    props: &Props<'a>,
    rect: Rectangle,
    compact: bool,
    content_width: f32,
) -> Element<'a, Message> {
    let active = props.item.active_view();
    let header = navigation_header::view(
        navigation_header::Props {
            palette: props.palette,
            view: active.view,
            navigation: &active.navigation,
            home: props.home,
            draft: props.presentation.and_then(|state| state.draft.as_deref()),
            invalid: props.presentation.is_some_and(|state| state.invalid),
            interactive: props.interactive,
            sort_open: props.sort_open,
            settings: props.item.settings,
        },
        content_width,
    );
    let list_props = file_list::Props {
        order: props.order,
        order_revision: props.order_revision,
        usage: props.usage,
        folder_usages: Some(props.folder_usages),
        palette: props.palette,
        metadata_paths: props.metadata_paths,
        clipped_left: (props.geometry.offset.x - rect.x).max(0.0)
            + props.presentation.map_or(0.0, |state| state.scroll_x),
        clipped_width: ((rect.x + rect.width)
            .min(props.geometry.offset.x + props.geometry.viewport.width)
            - rect.x.max(props.geometry.offset.x))
        .max(0.0),
        width: content_width,
        metadata: props.metadata,
        settings: props.item.settings,
        scope: active.navigation.scope(),
        view: active.view,
        load: props.load,
        interactive: props.interactive,
        selected: props
            .presentation
            .and_then(|state| state.selected.as_deref()),
        selection: props.presentation.map(|state| &state.selection),
        thumbnails: props.thumbnails,
        hovered: props
            .presentation
            .and_then(|state| state.hovered.as_deref()),
        window: file_window(props, rect, compact, content_width),
        revision: props.presentation.map_or(0, |state| state.listing_revision),
        show_hidden: props.item.settings.show_hidden,
        motion: props.presentation.and_then(|state| state.motion.as_ref()),
        dragging: props.dragging,
        column_preview: props.column_preview,
    };
    let footer = status(props);
    if compact {
        compact_contents(
            props,
            active.view,
            header,
            list_props,
            footer,
            content_width,
        )
    } else {
        let files = if props.item.settings.file_view == crate::domain::FileView::Columns {
            crate::boxes::file_columns::view(
                list_props,
                &active.navigation,
                props
                    .presentation
                    .is_some_and(|state| state.earlier_ancestors),
                props.column_preview,
            )
        } else {
            file_list::view(list_props)
        };
        column![header, files, footer]
            .width(Fill)
            .height(Fill)
            .into()
    }
}

fn compact_contents<'a>(
    props: &Props<'a>,
    view: crate::domain::ViewId,
    header: Element<'a, Message>,
    list_props: file_list::Props<'a>,
    footer: Element<'a, Message>,
    content_width: f32,
) -> Element<'a, Message> {
    a::scroll_area(
        column![
            header,
            crate::menus::folder_menu::target(
                view,
                props.interactive,
                file_list::content(list_props)
            ),
            footer
        ]
        .width(content_width),
    )
    .style(props.palette.scrollable())
    .direction(scrollable::Direction::Both {
        vertical: a::scrollbar(),
        horizontal: a::scrollbar(),
    })
    .id(file_list::scroll_id(view))
    .width(Fill)
    .height(Fill)
    .on_scroll(move |viewport| {
        Message::FileViewportScrolled(view, viewport.absolute_offset(), viewport.bounds().height)
    })
    .into()
}

fn file_window(
    props: &Props<'_>,
    rect: Rectangle,
    compact: bool,
    content_width: f32,
) -> file_list::ScrollWindow {
    let scroll = props
        .presentation
        .map_or(file_list::ScrollWindow::default(), |state| state.scroll);
    let header =
        navigation_header::height(content_width, props.item.settings, props.palette.typography)
            + if !compact && props.item.settings.file_view == crate::domain::FileView::Columns {
                crate::boxes::file_columns::header_height(props.palette.typography)
            } else {
                0.0
            };
    let top = rect.y + if compact { 0.0 } else { header };
    let bottom = rect.y + rect.height
        - if compact {
            0.0
        } else {
            folder_status::height(props.palette.typography)
        };
    let clipped_top = (props.geometry.offset.y - top).max(0.0);
    let height = (bottom.min(props.geometry.offset.y + props.geometry.viewport.height)
        - top.max(props.geometry.offset.y))
    .max(0.0);
    let height = if scroll.height > 0.0 {
        height.min(scroll.height)
    } else {
        height
    };
    let offset = scroll.offset + clipped_top;
    file_list::ScrollWindow {
        offset: if compact {
            (offset - header).max(0.0)
        } else {
            offset
        },
        height,
    }
}
