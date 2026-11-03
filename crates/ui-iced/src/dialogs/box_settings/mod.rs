//! Non-modal per-box drafts, inspector placement, and visibility reconciliation.

use iced::widget::operation;
use iced::{Point, Rectangle, Size, Task};

use crate::application::{Action, LoadState};
use crate::domain::{BoxId, BoxSettings, NavigationIntent, ViewId};
use crate::{Desktop, Message, boxes::file_list, boxes::navigation_header};

pub(crate) fn folder_id(view: ViewId) -> String {
    format!("box-settings-folder-{}", view.0)
}
pub(crate) const CARD_ID: &str = "box-settings-card";
pub(crate) const FIELDS_ID: &str = "box-settings-fields";
pub(crate) const HANDLE_ID: &str = "box-settings-handle";
pub(crate) const CLOSE_ID: &str = "box-settings-close";
pub(crate) const FORWARD_ID: &str = "box-settings-forward";
pub(crate) const UP_ID: &str = "box-settings-up";
pub(crate) const START_ID: &str = "box-settings-start";
pub(crate) const PROHIBIT_DELETE_ID: &str = "box-prohibit-delete";
pub(crate) const NAVIGATION_VALUE_ID: &str = "box-settings-navigation-value";
pub(crate) const FILE_VIEW_VALUE_ID: &str = "box-settings-file-view-value";
pub(crate) const TABS_ID: &str = "box-settings-tabs";
const ROW_HEIGHT: f32 = 44.0;
mod selection;
mod view;
pub(crate) use view::view;
pub(crate) const CHOOSE_ID: &str = "box-settings-choose-folder";

pub(crate) struct Draft {
    pub range_page: bool,
    form_restore: bool,
    form_offset: f32,
    picker: Option<u64>,
    picker_error: Option<String>,
    pub range: crate::dialogs::range_editor::Draft,
    pub tree_offset: f32,
    scope_kind: crate::domain::ScopeKind,
    pub id: BoxId,
    pub view: ViewId,
    pub folder: String,
    edited: bool,
    revision: u64,
    submitted: Option<(u64, u64)>,
    pub focused: bool,
    position: Point,
    drag: Option<(Point, Point)>,
}

pub(crate) fn dimensions(size: Size, header: f32) -> Size {
    Size::new(
        (size.width - 24.0).clamp(1.0, 860.0),
        (size.height - header - 24.0).clamp(1.0, 1120.0),
    )
}

fn clamp(position: Point, size: Size, header: f32) -> Rectangle {
    let dimensions = dimensions(size, header);
    Rectangle {
        x: position
            .x
            .clamp(12.0, (size.width - dimensions.width - 12.0).max(12.0)),
        y: position.y.clamp(
            header + 12.0,
            (size.height - dimensions.height - 12.0).max(header + 12.0),
        ),
        width: dimensions.width,
        height: dimensions.height,
    }
}

fn initial_position(size: Size, header: f32, target: Rectangle) -> Point {
    let dimensions = dimensions(size, header);
    let candidates = [
        Point::new(size.width - dimensions.width - 24.0, header + 16.0),
        Point::new(24.0, header + 16.0),
        Point::new(
            size.width - dimensions.width - 24.0,
            size.height - dimensions.height - 24.0,
        ),
        Point::new(24.0, size.height - dimensions.height - 24.0),
    ];
    candidates
        .into_iter()
        .min_by(|a, b| {
            let area = |point| {
                clamp(point, size, header)
                    .intersection(&target)
                    .map_or(0.0, |rect| rect.width * rect.height)
            };
            area(*a).total_cmp(&area(*b))
        })
        .unwrap_or(Point::ORIGIN)
}

impl Draft {
    pub fn bounds(&self, desktop: &Desktop) -> Rectangle {
        clamp(
            self.position,
            desktop.size,
            desktop.geometry().header_height,
        )
    }

    pub(crate) fn picker(&self) -> Option<u64> {
        self.picker
    }

    pub(crate) fn clear_picker(&mut self) {
        self.picker = None;
    }
}

impl Desktop {
    pub(crate) fn visibility_before(&self, action: &Action) -> Vec<(ViewId, BoxSettings)> {
        match action {
            Action::Undo => self
                .application
                .workspace()
                .boxes()
                .iter()
                .flat_map(|item| item.views().map(|view| (view.view, item.settings)))
                .collect(),
            Action::SetBoxSetting(id, _) => self
                .application
                .workspace()
                .boxes()
                .iter()
                .find(|item| item.id == *id)
                .into_iter()
                .flat_map(|item| item.views().map(|view| (view.view, item.settings)))
                .collect(),
            _ => Vec::new(),
        }
    }
    pub(crate) fn supersede_box_folder_blur(&mut self, action: &Action) {
        if let Action::EditScope(view, _) | Action::ConfigurePath(view, _) = action
            && let Some(draft) = &mut self.box_settings
            && draft.view == *view
        {
            draft.picker = None;
            draft.focused = false;
            draft.range.focused = None;
            if matches!(action, Action::ConfigurePath(..)) {
                draft.edited = false;
                draft.submitted = None;
            }
        }
        if let Action::ChangeTabs(id, command) = action
            && self.workspace_input_enabled()
            && self.application.workspace().can_change_tabs(*id, *command)
            && let Some(draft) = &mut self.box_settings
            && draft.id == *id
        {
            draft.picker = None;
            draft.focused = false;
        }
        if let Action::Navigate(view, _) = action
            && self.workspace_input_enabled()
            && self.application.workspace().can_browse(*view)
            && let Some(draft) = &mut self.box_settings
            && draft.view == *view
        {
            // Explicit navigation supersedes blur from the same pointer action.
            draft.picker = None;
            draft.focused = false;
        }
    }
    pub(crate) fn path_focus_checked(&mut self, message: Message) -> Task<Message> {
        let Message::PathFocusChecked {
            focused,
            box_folder,
            boundary,
            version,
        } = message
        else {
            return Task::none();
        };
        if version != self.focus_version {
            return Task::none();
        }
        if let Some(draft) = &mut self.box_settings {
            draft.range.focused = boundary;
            if boundary.is_some() {
                draft.focused = false;
                self.focused_path = None;
                return Task::none();
            }
            let blurred = draft.focused && !box_folder;
            draft.focused = box_folder;
            if box_folder {
                self.focused_path = None;
                return operation::focus(folder_id(draft.view));
            }
            if blurred {
                self.focused_path = focused;
                let view = draft.view;
                return self.box_settings_message(Message::BlurBoxFolder(view));
            }
        }
        self.focused_path = focused;
        if let Some(view) = focused {
            self.active_view = Some(view);
            // A captured click can leave another editor focused beneath the stack.
            return operation::focus(navigation_header::path_id(view));
        }
        Task::none()
    }
    pub(crate) fn sync_box_settings(&mut self) {
        let modal = self.modal_open();
        let header = self.geometry().header_height;
        let workspace = self.application.workspace();
        let item = workspace
            .boxes()
            .iter()
            .find(|item| workspace.configuring(item.id));
        let Some(item) = item.filter(|_| self.application.accepts_edits()) else {
            if self.box_settings.take().is_some() {
                self.focused_path = None;
                self.focus_version = self.focus_version.saturating_add(1);
            }
            return;
        };
        if self
            .box_settings
            .as_ref()
            .is_some_and(|draft| draft.id == item.id && draft.view == item.active_view().view)
        {
            if let Some(draft) = &mut self.box_settings {
                let kind = item.active_view().navigation.scope().kind();
                if draft.scope_kind != kind {
                    draft.picker = None;
                    draft.range_page = false;
                    draft.scope_kind = kind;
                    draft.range = crate::dialogs::range_editor::Draft::new();
                }
                if modal || !self.application.accepts_workspace_input() {
                    draft.range.suspend();
                }
                let applied = draft.range.sync(
                    item.active_view().navigation.scope().range(),
                    self.application.scope_pending(draft.view),
                    self.application.scope_error(draft.view).is_some(),
                );
                if applied && draft.range_page {
                    draft.range_page = false;
                    draft.form_restore = true;
                }
                draft.position = clamp(draft.position, self.size, header).position();
                if !self.application.accepts_workspace_input() {
                    draft.drag = None;
                    draft.focused = false;
                }
                if modal {
                    // A modal suspends Configure and dismisses any active folder sheet.
                    draft.picker = None;
                }
                if !draft.edited {
                    draft.folder = navigation_header::display_path(
                        item.active_view().navigation.current(),
                        self.application.home(),
                    );
                }
                if draft.submitted.is_some_and(|(generation, _)| !matches!(self.application.load(item.active_view().view), Some(LoadState::Loading { request, intent: NavigationIntent::Configure, .. }) if request.generation == generation)) {
                    draft.submitted = None;
                }
            }
            return;
        }
        let geometry = self.geometry();
        let rect = geometry.rect(item.rect);
        let target = Rectangle {
            x: rect.x + geometry.viewport.x - geometry.offset.x,
            y: rect.y + geometry.viewport.y - geometry.offset.y,
            ..rect
        };
        self.box_settings = Some(Draft {
            range_page: false,
            form_restore: false,
            form_offset: 0.0,
            picker: None,
            picker_error: None,
            range: crate::dialogs::range_editor::Draft::new(),
            tree_offset: 0.0,
            scope_kind: item.active_view().navigation.scope().kind(),
            id: item.id,
            view: item.active_view().view,
            folder: navigation_header::display_path(
                item.active_view().navigation.current(),
                self.application.home(),
            ),
            edited: false,
            revision: 0,
            submitted: None,
            focused: false,
            position: initial_position(self.size, geometry.header_height, target),
            drag: None,
        });
        self.focused_path = None;
        self.focus_version = self.focus_version.saturating_add(1);
        if let Some(draft) = &mut self.box_settings {
            draft
                .range
                .sync(item.active_view().navigation.scope().range(), false, false);
        }
    }

    pub(crate) fn box_settings_message(&mut self, message: Message) -> Task<Message> {
        if matches!(
            message,
            Message::TreeChosen(..)
                | Message::TreeScrolled(..)
                | Message::RangeStartChanged(..)
                | Message::RangeStopsChanged(..)
                | Message::CommitRange(_)
                | Message::CancelRange(_)
                | Message::RangeStage(..)
        ) {
            return self.range_message(message);
        }
        if let Message::BoxFolderChosen(generation, result) = message {
            return self.box_folder_chosen(generation, result);
        }
        if !self.workspace_input_enabled() {
            return Task::none();
        }
        if let Message::BoxFolderChanged(view, _)
        | Message::CommitBoxFolder(view)
        | Message::BlurBoxFolder(view) = &message
            && !self
                .box_settings
                .as_ref()
                .is_some_and(|draft| draft.view == *view && !draft.range_page)
        {
            return Task::none();
        }
        match message {
            Message::ChooseBoxFolder(view) => self.choose_box_folder(view),
            Message::BoxFormScrolled(view, offset) => {
                if let Some(draft) = &mut self.box_settings
                    && draft.view == view
                    && !draft.range_page
                {
                    draft.form_offset = offset;
                }
                Task::none()
            }
            Message::BoxFolderChanged(_, folder) => {
                let Some(draft) = &mut self.box_settings else {
                    return Task::none();
                };
                draft.folder = folder;
                draft.picker = None;
                draft.picker_error = None;
                draft.edited = true;
                draft.revision = draft.revision.saturating_add(1);
                draft.submitted = None;
                draft.focused = true;
                let view = draft.view;
                self.focused_path = None;
                self.focus_version = self.focus_version.saturating_add(1);
                self.dispatch(Action::ConfiguredFolderEdited(view))
            }
            Message::CommitBoxFolder(_) => self.commit_box_folder(true),
            Message::BlurBoxFolder(_) => self.commit_box_folder(false),
            Message::BeginCardDrag => {
                let header = self.geometry().header_height;
                if let Some(draft) = &mut self.box_settings
                    && let Some(pointer) = self.pointer
                {
                    let bounds = clamp(draft.position, self.size, header);
                    let position = bounds.position();
                    draft.drag = Some((pointer, position));
                }
                Task::none()
            }
            Message::EndCardDrag => {
                self.cancel_card_drag();
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn commit_box_folder(&mut self, explicit: bool) -> Task<Message> {
        let Some(draft) = &self.box_settings else {
            return Task::none();
        };
        if (!draft.edited && !explicit)
            || draft
                .submitted
                .is_some_and(|(_, revision)| revision == draft.revision)
        {
            return Task::none();
        }
        let view = draft.view;
        let folder = draft.folder.clone();
        let revision = draft.revision;
        let edited = draft.edited;
        let task = self.dispatch(if edited {
            Action::ConfigureFolder(view, folder)
        } else {
            Action::ConfigureCurrentFolder(view)
        });
        if let Some(LoadState::Loading {
            request,
            intent: NavigationIntent::Configure,
            ..
        }) = self.application.load(view)
            && let Some(draft) = &mut self.box_settings
        {
            draft.submitted = Some((request.generation, revision));
        }
        task
    }

    pub(crate) fn completed_box_folder(&mut self, view: ViewId, generation: u64, succeeded: bool) {
        if let Some(draft) = &mut self.box_settings
            && draft.view == view
            && draft.submitted == Some((generation, draft.revision))
        {
            draft.submitted = None;
            if succeeded && let Some(item) = self.application.workspace().view(view) {
                draft.folder = navigation_header::display_path(
                    item.navigation.current(),
                    self.application.home(),
                );
                draft.edited = false;
            }
        }
    }

    pub(crate) fn cancel_card_drag(&mut self) {
        if let Some(draft) = &mut self.box_settings {
            draft.drag = None;
        }
    }

    pub(crate) fn drag_card_to(&mut self, pointer: Point) -> bool {
        let header = self.geometry().header_height;
        if let Some(draft) = &mut self.box_settings
            && let Some((anchor, position)) = draft.drag
        {
            draft.position = clamp(position + (pointer - anchor), self.size, header).position();
            return true;
        }
        false
    }

    pub(crate) fn reconcile_box_settings(
        &mut self,
        before: Vec<(ViewId, BoxSettings)>,
    ) -> Task<Message> {
        let ty = crate::window::typography::Style(self.application.typography());
        let changed: Vec<_> = before
            .into_iter()
            .filter_map(|(view, old)| {
                self.application
                    .workspace()
                    .view(view)
                    .filter(|item| old != item.settings)
                    .map(|item| {
                        (
                            view,
                            old.file_view,
                            item.settings.file_view,
                            old.show_hidden != item.settings.show_hidden,
                        )
                    })
            })
            .collect();
        Task::batch(
            changed
                .into_iter()
                .map(|(view, old_layout, layout, hidden_changed)| {
                    self.reconcile_view_settings(view, old_layout, layout, hidden_changed, ty)
                }),
        )
    }

    fn reconcile_view_settings(
        &mut self,
        view: ViewId,
        old_layout: crate::domain::FileView,
        layout: crate::domain::FileView,
        hidden_changed: bool,
        ty: crate::window::typography::Style,
    ) -> Task<Message> {
        let width = self
            .application
            .workspace()
            .boxes()
            .iter()
            .find(|item| item.views().any(|candidate| candidate.view == view))
            .map_or(1.0, |item| {
                let rect = self.geometry().rect(item.rect);
                rect.width - crate::boxes::tab_rail::width(item, rect.width)
            });
        let show_hidden = self
            .application
            .workspace()
            .view(view)
            .is_some_and(|item| item.settings.show_hidden);
        let Some(presentation) = self.views.get_mut(&view) else {
            return Task::none();
        };
        if hidden_changed {
            presentation.motion = None;
        }
        if old_layout != layout {
            let old_columns = if old_layout == crate::domain::FileView::Grid {
                crate::boxes::file_grid::columns(width, ty)
            } else {
                1
            };
            let old_height = if old_layout == crate::domain::FileView::Grid {
                ty.row(crate::boxes::file_grid::TILE_HEIGHT)
            } else {
                ty.row(file_list::ROW_HEIGHT)
            };
            let index =
                (presentation.scroll.offset / old_height).floor().max(0.0) as usize * old_columns;
            let new_columns = if layout == crate::domain::FileView::Grid {
                crate::boxes::file_grid::columns(width, ty)
            } else {
                1
            };
            let new_height = if layout == crate::domain::FileView::Grid {
                ty.row(crate::boxes::file_grid::TILE_HEIGHT)
            } else {
                ty.row(file_list::ROW_HEIGHT)
            };
            presentation.scroll.offset = (index / new_columns) as f32 * new_height;
            presentation.hovered = None;
            presentation.earlier_ancestors = false;
        }
        let listing = self.application.load(view).and_then(LoadState::listing);
        let visible = |path: &std::path::Path| {
            if !show_hidden
                && path
                    .file_name()
                    .is_some_and(|name| name.as_encoded_bytes().first() == Some(&b'.'))
            {
                return false;
            }
            listing.is_none_or(|listing| {
                listing
                    .entry(path)
                    .is_some_and(|entry| show_hidden || !entry.hidden)
            })
        };
        presentation.selection.retain(|path| visible(path));
        for selection in [
            &mut presentation.selected,
            &mut presentation.hovered,
            &mut presentation.selection_anchor,
        ] {
            if selection.as_ref().is_some_and(|path| !visible(path)) {
                *selection = None;
            }
        }
        if old_layout != layout {
            operation::scroll_to(
                file_list::scroll_id(view),
                operation::AbsoluteOffset {
                    x: 0.0,
                    y: presentation.scroll.offset,
                },
            )
            .chain(file_list::observe_scroll(
                view,
                presentation.listing_revision,
            ))
        } else {
            file_list::observe_scroll(view, presentation.listing_revision)
        }
    }
}
