//! Bounded background read owner and virtualized list for the right-column subfolder preview.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use iced::widget::{column, container, mouse_area, row, scrollable, space, text};
use iced::{Element, Fill, Padding, Task};

#[cfg(test)]
use crate::application::contracts::directory::{Cancellation, DirectoryReader};
use crate::application::contracts::directory::{
    DirectoryEntry, DirectoryListing, EntryKind, ListingCompletion, ListingRequest,
};
use crate::application::{Action, LoadState, Navigate};
use crate::domain::sorting::{SortField, SortSettings, compare_names};
use crate::domain::{FileView, Scope, ScopeKind, ViewId};
use crate::{
    Desktop, DirectoryBackend, Message, PendingRead,
    boxes::file_list,
    window::{
        appearance as a,
        icons::{self, Icon},
    },
};

#[derive(Debug, Clone)]
pub(crate) struct ColumnFocus {
    pub directory: PathBuf,
    pub target: PathBuf,
}

pub(crate) enum PreviewLoad {
    Loading {
        generation: u64,
    },
    Loaded {
        generation: u64,
        listing: Arc<DirectoryListing>,
        ordered: Vec<usize>,
    },
    Failed {
        message: String,
    },
}

pub(crate) struct ViewPreview {
    pub directory: PathBuf,
    pub sort: SortSettings,
    pub show_hidden: bool,
    pub load: PreviewLoad,
    pub selected: Option<PathBuf>,
    pub hovered: Option<PathBuf>,
    pub scroll_y: f32,
}

impl ViewPreview {
    pub(super) fn item_count(&self) -> Option<usize> {
        match &self.load {
            PreviewLoad::Loaded { ordered, .. } => Some(ordered.len()),
            PreviewLoad::Loading { .. } | PreviewLoad::Failed { .. } => None,
        }
    }

    pub(crate) fn visible_demand_paths(
        &self,
        window_height: f32,
        row_height: f32,
    ) -> impl Iterator<Item = &Path> {
        let row_h = row_height.max(1.0);
        let viewport_h = window_height.max(row_h * 8.0);
        let first = ((self.scroll_y / row_h).floor() as usize).saturating_sub(4);
        let visible_rows = ((viewport_h / row_h).ceil() as usize) + 10;
        let slice = match &self.load {
            PreviewLoad::Loaded {
                listing, ordered, ..
            } => {
                let total = ordered.len();
                let start = first.min(total);
                let end = start.saturating_add(visible_rows).min(total);
                Some((listing.as_ref(), &ordered[start..end]))
            }
            PreviewLoad::Loading { .. } | PreviewLoad::Failed { .. } => None,
        };
        slice.into_iter().flat_map(|(listing, indices)| {
            indices.iter().filter_map(move |&idx| {
                listing.entries().get(idx).map(|entry| entry.path.as_path())
            })
        })
    }
}

#[derive(Default)]
pub(crate) struct Runtime {
    next_generation: u64,
    views: HashMap<ViewId, ViewPreview>,
    pending: HashMap<ViewId, PendingRead>,
    pending_selection: HashMap<ViewId, (PathBuf, PathBuf)>,
    focused_view: Option<ViewId>,
}

impl Runtime {
    pub(crate) fn get(&self, view: ViewId) -> Option<&ViewPreview> {
        self.views.get(&view)
    }

    pub(crate) fn clear(&mut self) {
        self.pending.clear();
        self.views.clear();
        self.pending_selection.clear();
        self.focused_view = None;
    }

    pub(crate) fn set_focused(&mut self, view: Option<ViewId>) {
        self.focused_view = view;
    }

    pub(crate) fn is_focused(&self, view: ViewId) -> bool {
        self.focused_view == Some(view)
            && self
                .views
                .get(&view)
                .is_some_and(|preview| matches!(preview.load, PreviewLoad::Loaded { .. }))
    }

    pub(crate) fn jump_candidates(
        &self,
        view: ViewId,
        scope: &Scope,
    ) -> Vec<crate::application::files::jump::JumpCandidate<'_>> {
        let Some(preview) = self.views.get(&view) else {
            return Vec::new();
        };
        let PreviewLoad::Loaded {
            listing, ordered, ..
        } = &preview.load
        else {
            return Vec::new();
        };
        crate::application::files::jump::listing_jump_candidates(
            listing,
            ordered,
            preview.show_hidden,
            scope,
        )
    }

    pub(crate) fn jump_select_and_reveal(
        &mut self,
        view: ViewId,
        path: PathBuf,
        display_index: usize,
        viewport_h: f32,
        row_h: f32,
    ) -> Task<Message> {
        let Some(preview) = self.views.get_mut(&view) else {
            return Task::none();
        };
        let (generation, total) = match &preview.load {
            PreviewLoad::Loaded {
                generation,
                ordered,
                ..
            } => (*generation, ordered.len()),
            PreviewLoad::Loading { .. } | PreviewLoad::Failed { .. } => return Task::none(),
        };
        preview.selected = Some(path);
        let top = display_index as f32 * row_h;
        let bottom = (display_index + 1) as f32 * row_h;
        let content_h = total as f32 * row_h;
        preview.scroll_y =
            file_list::reveal_scroll_offset(preview.scroll_y, viewport_h, content_h, top, bottom);
        let scroll_id = format!("column-preview-scroll-{}", view.0);
        iced::advanced::widget::operate(file_list::RevealScroll::new(scroll_id, top, bottom))
            .map(move |window| Message::ColumnPreviewScrolled(view, generation, window.offset))
    }

    pub(crate) fn clear_pending(&mut self) {
        self.pending.clear();
    }

    pub(crate) fn invalidate_directory(&mut self, dir: &Path) {
        let affected: Vec<_> = self
            .views
            .iter()
            .filter(|(_, state)| state.directory == dir)
            .map(|(view, _)| *view)
            .collect();
        for view in affected {
            self.pending.remove(&view);
            self.views.remove(&view);
            if self.focused_view == Some(view) {
                self.focused_view = None;
            }
        }
    }

    pub(crate) fn retained_listing(&self, view: ViewId) -> Option<Arc<DirectoryListing>> {
        match &self.views.get(&view)?.load {
            PreviewLoad::Loaded { listing, .. } => Some(Arc::clone(listing)),
            PreviewLoad::Loading { .. } | PreviewLoad::Failed { .. } => None,
        }
    }

    pub(crate) fn entry(&self, view: ViewId, path: &Path) -> Option<&DirectoryEntry> {
        match &self.views.get(&view)?.load {
            PreviewLoad::Loaded { listing, .. } => listing.entry(path),
            PreviewLoad::Loading { .. } | PreviewLoad::Failed { .. } => None,
        }
    }

    pub(crate) fn listed_entry(&self, view: ViewId, path: &Path) -> Option<&DirectoryEntry> {
        let preview = self.views.get(&view)?;
        let PreviewLoad::Loaded { listing, .. } = &preview.load else {
            return None;
        };
        listing
            .entry(path)
            .filter(|entry| preview.show_hidden || !entry.hidden)
    }

    pub(crate) fn visible_entry(
        &self,
        scope: &Scope,
        view: ViewId,
        path: &Path,
    ) -> Option<&DirectoryEntry> {
        let preview = self.views.get(&view)?;
        let PreviewLoad::Loaded { listing, .. } = &preview.load else {
            return None;
        };
        listing
            .entry(path)
            .filter(|entry| preview.show_hidden || !entry.hidden)
            .filter(|entry| preview_entry_allowed(scope, listing, entry))
    }

    pub(crate) fn selected_entry(&self, view: ViewId) -> Option<(PathBuf, PathBuf)> {
        let preview = self.views.get(&view)?;
        let selected = preview.selected.as_ref()?;
        self.listed_entry(view, selected)?;
        Some((preview.directory.clone(), selected.clone()))
    }

    pub(crate) fn select(&mut self, view: ViewId, path: PathBuf) {
        if let Some(preview) = self.views.get_mut(&view) {
            preview.selected = Some(path);
        }
    }

    pub(crate) fn clear_selection(&mut self, view: ViewId) {
        if let Some(preview) = self.views.get_mut(&view) {
            preview.selected = None;
        }
    }

    pub(crate) fn remove_selected(&mut self, view: ViewId, path: &Path) {
        if let Some(preview) = self.views.get_mut(&view)
            && preview.selected.as_deref() == Some(path)
        {
            preview.selected = None;
        }
    }

    pub(crate) fn set_pending_selection(
        &mut self,
        view: ViewId,
        directory: PathBuf,
        target: PathBuf,
    ) {
        self.pending_selection.insert(view, (directory, target));
    }

    pub(crate) fn entry_index(&self, view: ViewId, path: &Path) -> Option<usize> {
        match &self.views.get(&view)?.load {
            PreviewLoad::Loaded {
                listing, ordered, ..
            } => ordered
                .iter()
                .position(|idx| listing.entries().get(*idx).is_some_and(|e| e.path == path)),
            PreviewLoad::Loading { .. } | PreviewLoad::Failed { .. } => None,
        }
    }

    pub(crate) fn scroll_y(&self, view: ViewId) -> f32 {
        self.views.get(&view).map_or(0.0, |state| state.scroll_y)
    }
}

impl Desktop {
    pub(crate) fn sync_column_previews(&mut self) -> Task<Message> {
        if !self.application.accepts_edits()
            || matches!(
                self.application.persistence(),
                crate::application::contracts::storage::PersistenceState::Loading(_)
            )
        {
            self.column_previews.clear();
            self.application.clear_column_previews();
            return Task::none();
        }

        let desired = collect_desired_previews(self);
        self.column_previews.views.retain(|view, current| {
            desired
                .get(view)
                .is_some_and(|target| target.directory == current.directory)
        });
        self.column_previews
            .pending
            .retain(|view, _| self.column_previews.views.contains_key(view));

        let mut tasks = Vec::new();
        for (view, target) in desired {
            if let Some(existing) = self.column_previews.views.get_mut(&view) {
                reconcile_existing_preview(existing, target.sort, target.show_hidden);
                continue;
            }
            tasks.push(self.start_column_preview(view, target));
        }
        self.column_previews.focused_view = self
            .column_previews
            .focused_view
            .filter(|view| self.column_previews.views.contains_key(view));
        self.application.clear_column_previews();
        for (&view, preview) in &self.column_previews.views {
            if let PreviewLoad::Loaded { listing, .. } = &preview.load {
                self.application
                    .set_column_preview(view, Some(Arc::clone(listing)));
            }
        }
        Task::batch(tasks)
    }

    fn start_column_preview(&mut self, view: ViewId, target: DesiredPreview) -> Task<Message> {
        if self.column_previews.focused_view == Some(view) {
            self.column_previews.focused_view = None;
        }
        self.column_previews.next_generation =
            self.column_previews.next_generation.saturating_add(1);
        let generation = self.column_previews.next_generation;
        let request = ListingRequest {
            view,
            generation,
            directory: target.directory.clone(),
        };
        self.column_previews.views.insert(
            view,
            ViewPreview {
                directory: target.directory,
                sort: target.sort,
                show_hidden: target.show_hidden,
                load: PreviewLoad::Loading { generation },
                selected: None,
                hovered: None,
                scroll_y: 0.0,
            },
        );
        let epoch = self.application.workspace_epoch();
        if let Err(reason) = self
            .application
            .safety_policy(Some(view))
            .check_read_directory(&request.directory)
        {
            return Task::done(Message::WorkspaceEpoch(
                epoch,
                Box::new(Message::ColumnPreviewCompleted(ListingCompletion {
                    result: Err(
                        crate::application::contracts::directory::DirectoryError {
                            path: request.directory.clone(),
                            kind: crate::application::contracts::directory::DirectoryErrorKind::PermissionDenied,
                            detail: Some(reason.to_owned()),
                        },
                    ),
                    request,
                })),
            ));
        }
        let scope = self.application.read_scope(view);
        match &self.directory {
            #[cfg(test)]
            DirectoryBackend::Controlled(reader) => {
                let result =
                    reader.list_scoped(&request.directory, &scope, &Cancellation::default());
                Task::done(Message::WorkspaceEpoch(
                    epoch,
                    Box::new(Message::ColumnPreviewCompleted(ListingCompletion {
                        request,
                        result,
                    })),
                ))
            }
            DirectoryBackend::Native(workers) => {
                let future = workers.submit_scoped(request.clone(), scope);
                let cancellation = future.cancellation();
                let (task, handle) = Task::perform(future, move |completion| {
                    Message::WorkspaceEpoch(
                        epoch,
                        Box::new(Message::ColumnPreviewCompleted(completion)),
                    )
                })
                .abortable();
                self.column_previews.pending.insert(
                    view,
                    PendingRead {
                        request,
                        cancellation,
                        handle,
                    },
                );
                task
            }
        }
    }

    pub(super) fn complete_column_preview(
        &mut self,
        completion: ListingCompletion,
    ) -> Task<Message> {
        let view = completion.request.view;
        let generation = completion.request.generation;
        if self
            .column_previews
            .pending
            .get(&view)
            .is_some_and(|pending| pending.request == completion.request)
        {
            self.column_previews.pending.remove(&view);
        }
        if !self.application.accepts_edits() || !self.application.workspace().is_active(view) {
            return Task::none();
        }
        let pending_selected = self.column_previews.pending_selection.remove(&view);
        let Some(preview) = self.column_previews.views.get_mut(&view) else {
            return Task::none();
        };
        let matches_pending = matches!(preview.load, PreviewLoad::Loading { generation: g } if g == generation)
            && preview.directory == completion.request.directory;
        if !matches_pending {
            return Task::none();
        }
        match completion.result {
            Ok(listing) => {
                let ordered = sort_preview_entries(&listing, preview.sort, preview.show_hidden);
                if let Some((dir, target)) = pending_selected
                    && dir == preview.directory
                    && listing
                        .entry(&target)
                        .is_some_and(|entry| preview.show_hidden || !entry.hidden)
                {
                    preview.selected = Some(target);
                    if self.active_view == Some(view) {
                        self.column_previews.focused_view = Some(view);
                    }
                }
                let listing = Arc::new(listing);
                self.application
                    .set_column_preview(view, Some(Arc::clone(&listing)));
                preview.load = PreviewLoad::Loaded {
                    generation,
                    listing,
                    ordered,
                };
            }
            Err(error) => {
                self.application.set_column_preview(view, None);
                preview.load = PreviewLoad::Failed {
                    message: error.to_string(),
                };
            }
        }
        Task::none()
    }

    pub(super) fn focus_column_preview(&mut self, view: ViewId) -> Task<Message> {
        if !self.file_view_input_enabled(view) {
            return Task::none();
        }
        self.type_jump.dismiss_session();
        if let Some(state) = self.views.get_mut(&view) {
            state.earlier_ancestors = false;
        }
        self.column_previews.clear_selection(view);
        self.activate_column_preview(view);
        self.focus_canvas()
    }

    pub(super) fn select_column_preview_entry(
        &mut self,
        view: ViewId,
        path: PathBuf,
    ) -> Task<Message> {
        if !self.file_view_input_enabled(view) || !self.application.workspace().can_browse(view) {
            return Task::none();
        }
        let exists = self
            .column_previews
            .entry(view, &path)
            .is_some_and(|entry| {
                self.column_previews
                    .get(view)
                    .is_some_and(|state| state.show_hidden || !entry.hidden)
            });
        if !exists {
            return Task::none();
        }
        self.type_jump.dismiss_session();
        if let Some(state) = self.views.get_mut(&view) {
            state.earlier_ancestors = false;
        }
        let was_active = self.active_view == Some(view);
        let already_selected = self
            .column_previews
            .views
            .get(&view)
            .is_some_and(|preview| preview.selected.as_deref() == Some(path.as_path()));
        if self.modifiers.command() && already_selected {
            self.column_previews.clear_selection(view);
            self.activate_column_preview(view);
            self.focused_path = None;
            self.focus_version = self.focus_version.saturating_add(1);
            return iced::widget::operation::focus("workspace-canvas");
        }
        if let Some(preview) = self.column_previews.views.get_mut(&view) {
            preview.selected = Some(path.clone());
        }
        let release = if was_active
            && already_selected
            && !self.modifiers.shift()
            && !self.modifiers.command()
        {
            crate::menus::file_drag::ClickRelease::DeselectColumnPreview
        } else {
            crate::menus::file_drag::ClickRelease::Keep
        };
        self.activate_column_preview(view);
        self.focused_path = None;
        self.focus_version = self.focus_version.saturating_add(1);
        self.demand_single_thumbnail(view, &path);
        self.begin_file_drag(view, path, release);
        Task::batch([
            self.fill_thumbnails(),
            iced::widget::operation::focus("workspace-canvas"),
        ])
    }

    pub(crate) fn open_column_preview_entry(
        &mut self,
        view: ViewId,
        path: PathBuf,
    ) -> Task<Message> {
        self.file_drag.clear();
        if !self.file_view_input_enabled(view) || !self.application.workspace().can_browse(view) {
            return Task::none();
        }
        let Some((preview_dir, entry_kind, allowed)) = self.preview_entry_access(view, &path)
        else {
            return Task::none();
        };
        self.type_jump.dismiss_session();
        self.activate_view(view);
        if entry_kind != EntryKind::Directory {
            return self.dispatch(Action::OpenFile(view, path));
        }
        if !allowed {
            return Task::none();
        }
        let presentation = self.views.entry(view).or_default();
        presentation.earlier_ancestors = false;
        presentation.column_focus = Some(ColumnFocus {
            directory: preview_dir.clone(),
            target: path,
        });
        self.dispatch(Action::Navigate(view, Navigate::Folder(preview_dir)))
    }

    pub(super) fn advance_into_preview(&mut self, view: ViewId) -> Task<Message> {
        self.file_drag.clear();
        if !self.file_view_input_enabled(view) || !self.application.workspace().can_browse(view) {
            return Task::none();
        }
        let Some(preview) = self.column_previews.get(view) else {
            return Task::none();
        };
        let target_dir = preview.directory.clone();
        let focus_target = preview.selected.clone();
        self.type_jump.dismiss_session();
        self.activate_view(view);
        let presentation = self.views.entry(view).or_default();
        presentation.earlier_ancestors = false;
        presentation.column_focus = focus_target.map(|target| ColumnFocus {
            directory: target_dir.clone(),
            target,
        });
        self.dispatch(Action::Navigate(view, Navigate::Folder(target_dir)))
    }

    pub(super) fn hover_column_preview_entry(
        &mut self,
        view: ViewId,
        path: PathBuf,
        hovered: bool,
    ) -> Task<Message> {
        if self.workspace_input_enabled()
            && self.application.workspace().can_browse(view)
            && let Some(preview) = self.column_previews.views.get_mut(&view)
        {
            if hovered {
                preview.hovered = Some(path);
            } else if preview.hovered.as_ref() == Some(&path) {
                preview.hovered = None;
            }
        }
        Task::none()
    }

    pub(super) fn scroll_column_preview(
        &mut self,
        view: ViewId,
        generation: u64,
        offset_y: f32,
    ) -> Task<Message> {
        if self.workspace_input_enabled()
            && self.application.workspace().can_browse(view)
            && let Some(preview) = self.column_previews.views.get_mut(&view)
            && matches!(preview.load, PreviewLoad::Loaded { generation: g, .. } if g == generation)
        {
            preview.scroll_y = offset_y;
        }
        Task::none()
    }

    fn preview_entry_access(
        &self,
        view: ViewId,
        path: &Path,
    ) -> Option<(PathBuf, EntryKind, bool)> {
        let scope = self.application.workspace().view(view)?.navigation.scope();
        let preview = self.column_previews.get(view)?;
        let PreviewLoad::Loaded { listing, .. } = &preview.load else {
            return None;
        };
        let entry = listing
            .entry(path)
            .filter(|entry| preview.show_hidden || !entry.hidden)?;
        let allowed = preview_entry_allowed(scope, listing, entry);
        Some((preview.directory.clone(), entry.kind, allowed))
    }
}

struct DesiredPreview {
    directory: PathBuf,
    sort: SortSettings,
    show_hidden: bool,
}

fn collect_desired_previews(desktop: &Desktop) -> HashMap<ViewId, DesiredPreview> {
    let geometry = desktop.geometry();
    let mut out = HashMap::new();
    for item in desktop.application.workspace().boxes() {
        if item.settings.file_view != FileView::Columns
            || !crate::boxes::folder_box::scroller_geometry(geometry, item).two_columns
        {
            continue;
        }
        let active = item.active_view();
        let view = active.view;
        let Some(selected) = desktop
            .views
            .get(&view)
            .and_then(|state| state.selected.as_ref())
        else {
            continue;
        };
        let Some(listing) = desktop.application.load(view).and_then(LoadState::listing) else {
            continue;
        };
        let Some(entry) = listing
            .entry(selected)
            .filter(|entry| item.settings.show_hidden || !entry.hidden)
            .filter(|entry| entry.kind == EntryKind::Directory)
        else {
            continue;
        };
        if !preview_entry_allowed(active.navigation.scope(), listing, entry) {
            continue;
        }
        out.insert(
            view,
            DesiredPreview {
                directory: entry.path.clone(),
                sort: item.settings.sort,
                show_hidden: item.settings.show_hidden,
            },
        );
    }
    out
}

fn reconcile_existing_preview(existing: &mut ViewPreview, sort: SortSettings, show_hidden: bool) {
    if existing.sort == sort && existing.show_hidden == show_hidden {
        return;
    }
    existing.sort = sort;
    existing.show_hidden = show_hidden;
    if let PreviewLoad::Loaded {
        listing, ordered, ..
    } = &mut existing.load
    {
        *ordered = sort_preview_entries(listing, sort, show_hidden);
        if existing
            .selected
            .as_ref()
            .is_some_and(|sel| listing.entry(sel).is_none_or(|e| !show_hidden && e.hidden))
        {
            existing.selected = None;
        }
    }
}

pub(super) fn preview_entry_allowed(
    scope: &Scope,
    listing: &DirectoryListing,
    entry: &DirectoryEntry,
) -> bool {
    entry.kind != EntryKind::Directory
        || scope.kind() == ScopeKind::Free
        || listing
            .entry_target(entry)
            .is_some_and(|resolved| scope.allows(resolved))
}

fn sort_preview_entries(
    listing: &DirectoryListing,
    sort: SortSettings,
    show_hidden: bool,
) -> Vec<usize> {
    let entries = listing.entries();
    let mut indices: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| show_hidden || !entry.hidden)
        .map(|(idx, _)| idx)
        .collect();
    indices.sort_by(|&a, &b| {
        let ae = &entries[a];
        let be = &entries[b];
        let folders = if sort.folders_first {
            (ae.kind != EntryKind::Directory).cmp(&(be.kind != EntryKind::Directory))
        } else {
            Ordering::Equal
        };
        folders
            .then_with(|| {
                if sort.field == SortField::Type {
                    let ax = ae
                        .path
                        .extension()
                        .map(|ext| ext.as_encoded_bytes().to_ascii_lowercase())
                        .unwrap_or_default();
                    let bx = be
                        .path
                        .extension()
                        .map(|ext| ext.as_encoded_bytes().to_ascii_lowercase())
                        .unwrap_or_default();
                    sort.direction.apply(ax.cmp(&bx))
                } else {
                    sort.direction.apply(compare_names(&ae.name, &be.name))
                }
            })
            .then_with(|| compare_names(&ae.name, &be.name))
            .then_with(|| ae.path.cmp(&be.path))
    });
    indices
}

pub(super) fn folder_preview<'a>(
    props: file_list::Props<'a>,
    allowed: bool,
    preview: Option<&'a ViewPreview>,
) -> Element<'a, Message> {
    let palette = props.palette;
    let ty = palette.typography;
    if !allowed {
        return status_note(ty, palette.muted, "Outside configured scope");
    }
    let Some(preview) = preview else {
        return status_note(ty, palette.muted, "Loading…");
    };
    match &preview.load {
        PreviewLoad::Loading { .. } => status_note(ty, palette.muted, "Loading…"),
        PreviewLoad::Failed { message, .. } => status_note(ty, palette.error, message),
        PreviewLoad::Loaded {
            generation,
            listing,
            ordered,
        } => {
            let body = if ordered.is_empty() {
                status_note(ty, palette.muted, "Empty folder")
            } else {
                preview_list(props, preview, *generation, listing, ordered)
            };
            crate::menus::folder_menu::preview_target(props.view, props.interactive, body)
        }
    }
}

fn status_note<'a>(
    ty: crate::window::typography::Style,
    color: iced::Color,
    label: &'a str,
) -> Element<'a, Message> {
    container(ty.small(label).color(color))
        .padding([12, 12])
        .width(Fill)
        .into()
}

fn preview_list<'a>(
    props: file_list::Props<'a>,
    preview: &'a ViewPreview,
    generation: u64,
    listing: &'a DirectoryListing,
    ordered: &'a [usize],
) -> Element<'a, Message> {
    let palette = props.palette;
    let ty = palette.typography;
    let view = props.view;
    let row_h = ty.row(file_list::ROW_HEIGHT);
    let total = ordered.len();
    let viewport_h = props.window.height.max(row_h * 8.0);
    let first = ((preview.scroll_y / row_h).floor() as usize).saturating_sub(4);
    let visible_rows = ((viewport_h / row_h).ceil() as usize) + 10;
    let start = first.min(total);
    let end = start.saturating_add(visible_rows).min(total);

    let mut rows = column![].width(Fill);
    if start > 0 {
        rows = rows.push(space().height(start as f32 * row_h));
    }
    for &entry_idx in &ordered[start..end] {
        if let Some(entry) = listing.entries().get(entry_idx) {
            let outside = !preview_entry_allowed(props.scope, listing, entry);
            rows = rows.push(preview_row(props, listing, entry, outside, preview));
        }
    }
    if end < total {
        rows = rows.push(space().height((total - end) as f32 * row_h));
    }

    a::scroll_area(rows)
        .direction(scrollable::Direction::Vertical(a::scrollbar()))
        .id(format!("column-preview-scroll-{}", view.0))
        .width(Fill)
        .height(Fill)
        .style(palette.scrollable())
        .on_scroll(move |viewport| {
            Message::ColumnPreviewScrolled(view, generation, viewport.absolute_offset().y)
        })
        .into()
}

fn preview_row<'a>(
    props: file_list::Props<'a>,
    listing: &'a DirectoryListing,
    entry: &'a DirectoryEntry,
    outside_scope: bool,
    preview: &'a ViewPreview,
) -> Element<'a, Message> {
    let view = props.view;
    let interactive = props.interactive;
    let palette = props.palette;
    let name = entry.name.to_string_lossy();
    let content = preview_row_content(
        entry,
        &name,
        outside_scope,
        interactive,
        preview,
        props.dragging,
        palette,
    );
    let mut hit = mouse_area(content);
    if interactive && !outside_scope {
        hit = hit
            .on_press(Message::ColumnPreviewSelected(view, entry.path.clone()))
            .on_double_click(Message::ColumnPreviewOpened(view, entry.path.clone()))
            .on_enter(Message::ColumnPreviewHovered(
                view,
                entry.path.clone(),
                true,
            ))
            .on_exit(Message::ColumnPreviewHovered(
                view,
                entry.path.clone(),
                false,
            ))
            .interaction(iced::mouse::Interaction::Pointer);
    }
    let target: Element<'_, Message> = if interactive {
        crate::menus::entry_menu::preview_target(view, entry.path.clone(), hit.into(), palette)
    } else {
        hit.into()
    };
    crate::runtime::metadata::hover_tooltip(target, props, Some(listing), entry)
}

fn preview_row_content<'a>(
    entry: &'a DirectoryEntry,
    name: &str,
    outside_scope: bool,
    interactive: bool,
    preview: &'a ViewPreview,
    dragging: Option<&'a HashSet<PathBuf>>,
    palette: a::Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    let selected = preview.selected.as_deref() == Some(entry.path.as_path());
    let hovered = preview.hovered.as_deref() == Some(entry.path.as_path());
    let drag_dim = if dragging.is_some_and(|set| set.contains(&entry.path)) {
        0.45
    } else {
        1.0
    };
    let icon = Icon::for_entry(entry);
    let icon_size = ty.icon(18.0);
    let muted = palette.muted.scale_alpha(drag_dim);
    let primary = if outside_scope {
        muted
    } else {
        palette.text.scale_alpha(drag_dim)
    };

    let mut line = row![
        container(icons::view(
            icon,
            icon_size,
            Some(palette.icon(icon)),
            drag_dim,
        ))
        .padding(Padding {
            bottom: icon_size / 12.0,
            ..Padding::ZERO
        }),
        ty.text(name.to_owned())
            .color(primary)
            .wrapping(text::Wrapping::None)
            .width(Fill)
    ]
    .spacing(10)
    .align_y(iced::Center);

    if entry.symlink {
        line = line.push(ty.caption("link").color(muted));
    }
    if outside_scope {
        line = line.push(ty.caption("outside scope").color(muted));
    }

    let bg = if selected {
        Some(palette.row_selected.scale_alpha(drag_dim).into())
    } else if interactive && hovered {
        Some(palette.row_hover.scale_alpha(drag_dim).into())
    } else {
        None
    };
    container(line)
        .width(Fill)
        .height(ty.row(file_list::ROW_HEIGHT))
        .padding([0, 8])
        .align_y(iced::Center)
        .clip(true)
        .style(move |_| container::Style {
            background: bg,
            ..container::Style::default()
        })
        .into()
}
