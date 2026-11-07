use super::retained_boxes;
use std::collections::HashMap;

impl crate::Desktop {
    pub(crate) fn canvas_scrolled(&mut self, offset: Vector) -> iced::Task<Message> {
        if !self.workspace_input_enabled() {
            return iced::Task::none();
        }
        self.canvas_offset = offset;
        self.dispatch(crate::application::Action::CancelPointer)
    }
}

use iced::widget::{Stack, container, pin, scrollable};
use iced::{Element, Fill, Vector};

use crate::application::Application;
use crate::domain::{Mode, ViewId};

use crate::{
    Message, ViewPresentation,
    boxes::folder_box::{self, Props},
    canvas::canvas_grid,
    canvas::draw_preview,
    canvas::empty_state,
    canvas::geometry::{CanvasGeometry, GUTTER},
    canvas::layout_controls,
    window::appearance as a,
};

#[derive(Clone, Copy)]
pub(crate) struct Presentation<'a> {
    pub tab_offsets: &'a HashMap<crate::domain::BoxId, f32>,
    pub tab_drag: Option<&'a crate::boxes::tab_drag::Drag>,
    pub file_drag: &'a crate::menus::file_drag::State,
    pub column_previews: &'a crate::boxes::file_columns::Runtime,
    pub conflict_name: Option<&'a str>,
    pub thumbnails: &'a crate::runtime::thumbnails::Values,
    pub sort_menu: Option<ViewId>,
}

pub fn view<'a>(
    application: &'a Application,
    presentations: &'a HashMap<ViewId, ViewPresentation>,
    geometry: CanvasGeometry,
    presentation: Presentation<'a>,
    palettes: a::Palettes,
) -> Element<'a, Message> {
    let workspace = application.workspace();
    let preview = workspace.draw_preview();
    let mut layers = Stack::new().push(canvas_grid::view(
        geometry,
        workspace.mode() == Mode::Layout,
        preview
            .map(|preview| (preview.rect, preview.is_valid()))
            .or_else(|| {
                workspace
                    .layout_preview()
                    .map(|preview| (preview.rect, preview.valid))
            }),
        palettes.window,
    ));
    layers = layers.push(boxes(
        application,
        presentations,
        geometry,
        presentation,
        palettes,
    ));
    if workspace.item_count() == 0 && workspace.mode() == Mode::Browse {
        layers = layers.push(empty_state::view(
            geometry.size(),
            application.persistence(),
            application.accepts_workspace_input() && application.file_operation().is_none(),
            palettes.window,
        ));
    }
    if let Some(preview) = preview {
        layers = layers.push(draw_preview::view(
            geometry,
            preview.rect,
            preview.is_valid(),
            palettes.window,
        ));
    }
    if let Some(preview) = workspace.layout_preview() {
        if let Some(rect) = workspace.item_rect(preview.id) {
            layers = layers.push(draw_preview::origin(geometry, rect, palettes.window));
        }
        layers = layers.push(draw_preview::layout(
            geometry,
            preview.rect,
            preview.valid,
            palettes.window,
        ));
        if let Some((_, rect)) = preview.swap {
            layers = layers.push(draw_preview::layout(geometry, rect, true, palettes.window));
        }
    }
    let size = geometry.size();
    let surface = a::scroll_area(layers.width(size.width).height(size.height))
        .style(palettes.window.scrollable())
        .id("workspace-canvas")
        .direction(scrollable::Direction::Both {
            vertical: a::scrollbar(),
            horizontal: a::scrollbar(),
        })
        .on_scroll(|viewport| {
            let offset = viewport.absolute_offset();
            Message::CanvasScrolled(Vector::new(offset.x, offset.y))
        })
        .width(Fill)
        .height(Fill);
    // The editing layer includes the gutter, so external resize targets at the
    // top/left canvas boundary are not clipped by the folder scroller.
    Stack::new()
        .push(container(surface).padding(GUTTER).width(Fill).height(Fill))
        .push(layout_controls::view(
            application,
            geometry,
            application.accepts_workspace_input(),
            presentation.tab_offsets,
            presentation.tab_drag.is_some(),
            palettes,
        ))
        .width(Fill)
        .height(Fill)
        .into()
}

fn boxes<'a>(
    application: &'a Application,
    presentations: &'a HashMap<ViewId, ViewPresentation>,
    geometry: CanvasGeometry,
    presentation: Presentation<'a>,
    palettes: a::Palettes,
) -> Element<'a, Message> {
    let workspace = application.workspace();
    let mut boxes = Stack::new();
    for item in workspace.boxes() {
        let rect = geometry.rect(item.rect);
        let view = item.active_view().view;
        boxes = boxes.push(
            pin(folder_box::view(Props {
                order: application.sort_indices(view),
                order_revision: application.sort_revision(view),
                sort_status: application.sort_status(view),
                usage: application.folder_usage(view),
                folder_usages: application.folder_usages(),
                palette: palettes.boxes,
                window_palette: palettes.window,
                metadata_paths: application.metadata_demand_paths(view),
                metadata: application.metadata_values(view),
                item,
                load: application.load(view),
                presentation: presentations.get(&view),
                operation: application.file_operation().filter(|operation| {
                    operation.view == view
                        && item.active_view().navigation.current() == operation.directory
                }),
                conflict_name: presentation.conflict_name,
                thumbnails: Some(presentation.thumbnails),
                files_closing: application.files_closing(),
                home: application.home(),
                geometry,
                interactive: workspace.can_browse(view) && application.accepts_workspace_input(),
                dimmed: workspace.mode() == Mode::Configure
                    && workspace.selected().is_some()
                    && workspace.selected() != Some(item.id),
                watch: application.watch_state(view),
                mode: workspace.mode(),
                tabs_input: application.accepts_workspace_input()
                    && (workspace.mode() != Mode::Configure || workspace.configuring(item.id)),
                tab_error: application.tab_error(item.id),
                opening_status: application.external_open_status(item.active_view().view),
                dragged_tab: presentation
                    .tab_drag
                    .filter(|drag| drag.id == item.id)
                    .map(|drag| drag.view),
                sort_open: presentation.sort_menu == Some(view),
                dragging: presentation.file_drag.dragged_set(view),
                column_preview: presentation.column_previews.get(view),
                capabilities: application.capabilities(),
            }))
            .x(rect.x)
            .y(rect.y),
        );
    }
    for item in workspace.launchers() {
        let rect = geometry.rect(item.rect);
        boxes = boxes.push(
            pin(crate::boxes::launchers::tile(
                application,
                item,
                rect.size(),
                palettes.boxes,
                palettes.window,
            ))
            .x(rect.x)
            .y(rect.y),
        );
    }
    retained_boxes::view(
        workspace.placements().map(|item| item.id).collect(),
        boxes
            .width(geometry.size().width)
            .height(geometry.size().height)
            .into(),
    )
}
