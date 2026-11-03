//! Authoritative workspace state and effect coordination, independent of Iced.

pub mod contracts;
pub mod files;
pub mod preferences;
#[cfg(test)]
mod tests;
pub mod workspace;

use crate::diagnostics::{Activity, Change, Outcome, Subsystem, configuration_changed, session};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use crate::domain::{
    Cell, DrawOutcome, GridExtent, Mode, NavigationIntent, ViewId, Workspace, expand_path,
};
use contracts::{
    configuration::{self, AppPreferences, WorkspaceIndex},
    directory::{self, DirectoryListing, ListingCompletion, ListingRequest},
    storage::{
        self, PersistenceState, SaveRequest, SaveTag, SessionId, StartupOperation,
        StorageCompletion,
    },
    watching,
};
use files::{file_operations, folder_tree, folder_usage, metadata, opening, sorting};
use preferences::{appearance, settings, typography};
use workspace::{box_configuration, launchers, management, scopes, tabs};

#[derive(Debug, Clone)]
pub enum Action {
    FolderUsageCompleted(folder_usage::UsageCompletion),
    SetSort(ViewId, crate::domain::sorting::SortSettings),
    SortCompleted(sorting::SortCompletion),
    StartFileOperation {
        view: ViewId,
        directory: PathBuf,
        items: Vec<(file_operations::Item, String)>,
    },
    FileOperationCompleted(file_operations::Completion),
    ResolveFileConflict(u64, u64, file_operations::ConflictChoice),
    StopFileOperation(u64),
    DismissFileOperation(u64),
    OpenWorkspaceManagement,
    CancelWorkspaceManagement,
    SelectWorkspace(configuration::WorkspaceId),
    CreateWorkspace(String),
    RenameWorkspace {
        id: configuration::WorkspaceId,
        name: String,
    },
    RemoveWorkspace {
        id: configuration::WorkspaceId,
        confirmed: bool,
    },
    RetryWorkspaceManagement,
    OpenFile(ViewId, PathBuf),
    ExternalOpened(opening::OpenCompletion),
    MetadataNeeded(ViewId, Vec<PathBuf>),
    MetadataCompleted(metadata::MetadataCompletion),
    EditScope(ViewId, scopes::ScopeEdit),
    ScopeResolved(ListingCompletion),
    ToggleTree(ViewId, PathBuf),
    TreeCompleted(ListingCompletion),
    ChangeTabs(crate::domain::BoxId, crate::domain::TabCommand),
    SelectMode(Mode),
    BeginDraw(Cell, GridExtent),
    BeginItemDraw(Cell, GridExtent),
    ConfigureLauncher(crate::domain::BoxId, crate::domain::LauncherConfig),
    ActivateLauncher(crate::domain::BoxId),
    LauncherOpened(launchers::Completion),
    DrawTo(Cell),
    FinishDraw,
    SelectBox(Option<crate::domain::BoxId>),
    ConfigureBox(Option<crate::domain::BoxId>),
    SetBoxSetting(crate::domain::BoxId, crate::domain::BoxSetting),
    ConfigureFolder(ViewId, String),
    ConfigurePath(ViewId, PathBuf),
    ConfigureCurrentFolder(ViewId),
    ConfiguredFolderEdited(ViewId),
    BeginLayout {
        id: crate::domain::BoxId,
        kind: crate::domain::LayoutKind,
        extent: GridExtent,
    },
    LayoutTo(crate::domain::GridDelta),
    FinishLayout,
    RemoveBox(crate::domain::BoxId),
    RemoveSelected,
    Undo,
    CancelPointer,
    Escape,
    Navigate(ViewId, Navigate),
    ReloadAll,
    WatchCompleted(watching::WatchBatch),
    ListingCompleted(ListingCompletion),
    StorageCompleted(StorageCompletion),
    RetrySave,
    WindowPlacementObserved(crate::domain::WindowPlacement),
    WindowPlacementAdjusted(crate::domain::WindowPlacement),
    WindowRestorationUnavailable(String),
    WindowCapturedForClose {
        tag: SaveTag,
        placement: Option<crate::domain::WindowPlacement>,
    },
    RestartStartup {
        operation: StartupOperation,
        confirmed: bool,
    },
    RequestClose,
    CancelClose,
    CloseWithoutSaving,
    OpenAppSettings,
    CancelAppSettings,
    ApplyAppSettings {
        new_box_folder: Option<PathBuf>,
        appearance: appearance::AppearancePreferences,
        user_themes: Vec<appearance::UserTheme>,
        typography: typography::TypographyPreferences,
        safety: crate::domain::SafetyPreferences,
    },
}

#[derive(Debug, Clone)]
pub enum Navigate {
    Folder(PathBuf),
    Typed(String),
    Back,
    Forward,
    Up,
    ReturnToStart,
    Refresh,
}

#[derive(Debug, Clone)]
pub enum Effect {
    MeasureFolderUsage(folder_usage::UsageRequest),
    SortDirectory(sorting::SortRequest),
    RunFileOperation(file_operations::Job),
    ManageWorkspace(storage::ManagementRequest),
    OpenExternal(opening::OpenRequest),
    OpenLauncher(launchers::Request),
    ReadMetadata(metadata::MetadataRequest),
    ResolveScope(ListingRequest),
    ListTree(ListingRequest),
    CheckWorkspaceBudget(storage::BudgetRequest),
    List(ListingRequest),
    ObserveDirectories(Vec<watching::DirectoryObservation>),
    LoadConfiguration(SessionId),
    RestoreBackup {
        session: SessionId,
        token: configuration::WorkspaceId,
    },
    SaveWorkspace(SaveRequest),
    CaptureWindowForClose(SaveTag),
    Close,
}

#[derive(Debug, Clone)]
pub enum LoadState {
    Loading {
        request: ListingRequest,
        intent: NavigationIntent,
        previous: Option<Arc<DirectoryListing>>,
    },
    Ready(Arc<DirectoryListing>),
    Failed {
        message: String,
        previous: Option<Arc<DirectoryListing>>,
    },
}

impl LoadState {
    pub fn listing(&self) -> Option<&DirectoryListing> {
        match self {
            Self::Ready(listing) => Some(listing),
            Self::Loading { previous, .. } | Self::Failed { previous, .. } => previous.as_deref(),
        }
    }

    pub fn retained_listing(&self) -> Option<Arc<DirectoryListing>> {
        match self {
            Self::Ready(listing) => Some(Arc::clone(listing)),
            Self::Loading { previous, .. } | Self::Failed { previous, .. } => previous.clone(),
        }
    }
}

#[derive(Debug)]
pub struct Application {
    workspace: Workspace,
    loads: HashMap<ViewId, LoadState>,
    directory_override: Option<PathBuf>,
    home: PathBuf,
    state_dir: PathBuf,
    next_generation: u64,
    status: Option<String>,
    persistence: PersistenceState,
    configuration: Option<(AppPreferences, WorkspaceIndex)>,
    user_themes: std::sync::Arc<[appearance::UserTheme]>,
    user_themes_edited: bool,
    user_themes_notice: Option<String>,
    restore_queue: VecDeque<ViewId>,
    restore_in_flight: HashSet<ViewId>,
    activation_reads: HashSet<ViewId>,
    configuration_editor: Option<settings::ConfigurationEditor>,
    watching: watching::ObservationBook,
    pending_configuration: Option<box_configuration::PendingFolder>,
    box_configuration_error: Option<(ViewId, String)>,
    pending_tabs: Option<tabs::PendingTabs>,
    next_tab_generation: u64,
    tab_error: Option<(crate::domain::BoxId, String)>,
    pending_scope: Option<scopes::PendingScope>,
    scope_error: Option<(ViewId, String)>,
    folder_tree: Option<folder_tree::FolderTree>,
    metadata: metadata::MetadataBook,
    usage: folder_usage::UsageBook,
    sorting: sorting::SortBook,
    opening: opening::OpeningBook,
    launcher_opening: launchers::OpeningBook,
    file_operations: file_operations::Operations,
    column_previews: HashMap<ViewId, Arc<DirectoryListing>>,
    management: Option<management::PendingManagement>,
}

impl Application {
    pub fn new(start: PathBuf, home: PathBuf) -> Self {
        let state_dir = crate::platform::state_directory(&home);
        Self {
            workspace: Workspace::default(),
            loads: HashMap::new(),
            directory_override: Some(start),
            home,
            state_dir,
            next_generation: 0,
            status: None,
            persistence: PersistenceState::Off,
            configuration: None,
            user_themes: std::sync::Arc::from([]),
            user_themes_edited: false,
            user_themes_notice: None,
            restore_queue: VecDeque::new(),
            restore_in_flight: HashSet::new(),
            activation_reads: HashSet::new(),
            configuration_editor: None,
            watching: watching::ObservationBook::default(),
            pending_configuration: None,
            box_configuration_error: None,
            pending_tabs: None,
            next_tab_generation: 0,
            tab_error: None,
            pending_scope: None,
            scope_error: None,
            folder_tree: None,
            metadata: metadata::MetadataBook::default(),
            usage: folder_usage::UsageBook::default(),
            sorting: sorting::SortBook::default(),
            opening: opening::OpeningBook::default(),
            launcher_opening: launchers::OpeningBook::default(),
            file_operations: file_operations::Operations::default(),
            column_previews: HashMap::new(),
            management: None,
        }
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }
    pub fn load(&self, view: ViewId) -> Option<&LoadState> {
        self.loads.get(&view)
    }
    pub fn home(&self) -> &std::path::Path {
        &self.home
    }
    pub fn state_directory(&self) -> &std::path::Path {
        &self.state_dir
    }
    pub fn set_state_directory(&mut self, state_dir: PathBuf) {
        self.state_dir = state_dir;
    }
    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    pub fn set_column_preview(&mut self, view: ViewId, listing: Option<Arc<DirectoryListing>>) {
        if let Some(listing) = listing {
            self.column_previews.insert(view, listing);
            if self.valid_column_preview(view).is_none() {
                self.column_previews.remove(&view);
            }
        } else {
            self.column_previews.remove(&view);
        }
    }

    pub fn clear_column_previews(&mut self) {
        self.column_previews.clear();
    }

    pub(in crate::application) fn valid_column_preview(
        &self,
        view: ViewId,
    ) -> Option<&Arc<DirectoryListing>> {
        let item = self.workspace.view(view)?;
        if item.settings.file_view != crate::domain::FileView::Columns {
            return None;
        }
        let LoadState::Ready(parent) = self.loads.get(&view)? else {
            return None;
        };
        if parent.directory != item.navigation.current() {
            return None;
        }
        let preview = self.column_previews.get(&view)?;
        let entry = parent.entry(&preview.directory)?;
        if entry.kind != directory::EntryKind::Directory
            || (entry.hidden && !item.settings.show_hidden)
        {
            return None;
        }
        let scope = item.navigation.scope();
        let allowed = scope.kind() == crate::domain::ScopeKind::Free
            || parent
                .entry_target(entry)
                .is_some_and(|resolved| scope.allows(resolved));
        allowed.then_some(preview)
    }

    pub fn has_active_directory(&self, view: ViewId, directory: &std::path::Path) -> bool {
        self.workspace
            .view(view)
            .is_some_and(|item| item.navigation.current() == directory)
            || self
                .valid_column_preview(view)
                .is_some_and(|preview| preview.directory == directory)
    }

    fn sync_column_previews_validity(&mut self) {
        if !self.accepts_edits() {
            self.column_previews.clear();
            return;
        }
        let stale: Vec<_> = self
            .column_previews
            .keys()
            .copied()
            .filter(|view| self.valid_column_preview(*view).is_none())
            .collect();
        for view in stale {
            self.column_previews.remove(&view);
        }
    }

    pub fn update(&mut self, action: Action) -> Vec<Effect> {
        let sync = matches!(
            action,
            Action::FinishDraw
                | Action::SelectWorkspace(_)
                | Action::CreateWorkspace(_)
                | Action::RemoveWorkspace { .. }
                | Action::RemoveBox(_)
                | Action::RemoveSelected
                | Action::Undo
                | Action::ListingCompleted(_)
                | Action::StorageCompleted(_)
                | Action::RestartStartup { .. }
                | Action::RequestClose
                | Action::CancelClose
                | Action::FileOperationCompleted(_)
                | Action::ChangeTabs(..)
        );
        let force = matches!(action, Action::ReloadAll) && self.accepts_workspace_input();
        let mut effects = self.apply(action);
        self.cancel_invalid_scope_context();
        if sync || force {
            effects.extend(self.sync_observations(force));
        }
        effects.extend(self.fill_reload_reads());
        effects.extend(self.sync_folder_tree());
        self.sync_metadata();
        effects.extend(self.sync_sorting());
        effects.extend(self.sync_folder_usage());
        self.sync_column_previews_validity();
        self.sync_opening();
        self.sync_launcher_opening();
        effects.extend(self.fill_metadata());
        effects
    }

    fn apply(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::FolderUsageCompleted(completion) => {
                self.complete_folder_usage(completion);
                return vec![];
            }
            Action::SortCompleted(completion) => {
                self.complete_sort(completion);
                return vec![];
            }
            Action::FileOperationCompleted(receipt) => return self.files_completed(receipt),
            Action::StopFileOperation(id) => {
                self.stop_files(id);
                return vec![];
            }
            Action::DismissFileOperation(id) => {
                if self
                    .file_operation()
                    .is_some_and(|operation| operation.id == id && !operation.active())
                {
                    self.file_operations.current = None;
                }
                return vec![];
            }
            Action::ResolveFileConflict(id, step, choice) => {
                return self.resolve_file_conflict(id, step, choice);
            }
            _ => {}
        }
        if self.files_block(&action) {
            return vec![];
        }
        match action {
            Action::StartFileOperation {
                view,
                directory,
                items,
            } => return self.start_files(view, directory, items),
            Action::StorageCompleted(completion) => return self.storage_completed(completion),
            Action::OpenWorkspaceManagement
            | Action::CancelWorkspaceManagement
            | Action::SelectWorkspace(_)
            | Action::CreateWorkspace(_)
            | Action::RenameWorkspace { .. }
            | Action::RemoveWorkspace { .. }
            | Action::RetryWorkspaceManagement => return self.management_action(action),
            Action::RetrySave => return self.retry_save(),
            Action::OpenAppSettings
            | Action::CancelAppSettings
            | Action::ApplyAppSettings { .. } => return self.configuration_action(action),
            Action::WindowPlacementObserved(placement)
            | Action::WindowPlacementAdjusted(placement) => return self.observe_window(placement),
            Action::WindowRestorationUnavailable(_) => return Vec::new(),
            Action::WindowCapturedForClose { tag, placement } => {
                return self.window_captured_for_close(tag, placement);
            }
            Action::RestartStartup {
                operation,
                confirmed,
            } => return self.restart_startup(operation, confirmed),
            Action::RequestClose => return self.request_close(),
            Action::CancelClose => return self.cancel_close(),
            Action::CloseWithoutSaving => return self.close_without_saving(),
            _ => {}
        }
        self.apply_workspace_action(action)
    }

    fn apply_workspace_action(&mut self, action: Action) -> Vec<Effect> {
        if !self.accepts_edits() {
            return Vec::new();
        }
        if self.configuration_blocks(&action) {
            return Vec::new();
        }
        let mut effects = Vec::new();
        self.supersede_tab_gesture(&action);
        match action {
            Action::SetSort(view, settings) => {
                if self.workspace.change_sort(view, settings) {
                    configuration_changed(Change::BoxSort);
                    return self.save_effect(true);
                }
            }
            Action::SortCompleted(_) | Action::FolderUsageCompleted(_) => {}
            Action::ChangeTabs(id, command) => return self.change_tabs(id, command),
            Action::StartFileOperation { .. }
            | Action::FileOperationCompleted(_)
            | Action::ResolveFileConflict(..)
            | Action::StopFileOperation(_)
            | Action::DismissFileOperation(_) => {}
            Action::OpenFile(view, path) => return self.open_file(view, path),
            Action::ExternalOpened(completion) => self.external_opened(completion),
            Action::MetadataNeeded(view, paths) => return self.request_metadata(view, paths),
            Action::MetadataCompleted(completion) => return self.complete_metadata(completion),
            Action::EditScope(view, edit) => return self.edit_scope(view, edit),
            Action::ScopeResolved(completion) => return self.scope_resolved(completion),
            Action::ToggleTree(view, path) => return self.toggle_tree(view, path),
            Action::TreeCompleted(completion) => return self.tree_completed(completion),
            Action::SelectMode(_) | Action::Escape => return self.workspace_mode_action(action),
            Action::BeginItemDraw(cell, extent) => {
                self.workspace.begin_item_draw(cell, extent);
                self.status = None;
            }
            Action::ConfigureLauncher(id, config) => {
                if self.workspace.configure_launcher(id, config) {
                    self.workspace.configure_box(None);
                    configuration_changed(Change::LauncherSettings);
                    return self.save_effect(true);
                }
            }
            Action::ActivateLauncher(id) => return self.activate_launcher(id),
            Action::LauncherOpened(completion) => self.launcher_opened(completion),
            Action::BeginDraw(cell, extent) => {
                self.workspace.begin_draw(cell, extent);
                self.status = None;
            }
            Action::DrawTo(cell) => self.workspace.draw_to(cell),
            Action::FinishDraw => return self.finish_draw(),
            Action::SelectBox(id) => self.workspace.select_box(id),
            Action::ConfigureBox(_)
            | Action::SetBoxSetting(..)
            | Action::ConfigureFolder(..)
            | Action::ConfigurePath(..)
            | Action::ConfigureCurrentFolder(_)
            | Action::ConfiguredFolderEdited(_) => return self.box_configuration_action(action),
            Action::BeginLayout { .. }
            | Action::LayoutTo(_)
            | Action::FinishLayout
            | Action::RemoveBox(_)
            | Action::RemoveSelected
            | Action::Undo => return self.editing_action(action),
            Action::CancelPointer => self.workspace.cancel_pointer(),
            Action::Navigate(view, command) => effects.extend(self.navigate(view, command)),
            Action::ReloadAll => self.reload_all(),
            Action::WatchCompleted(batch) => self.watch_completed(batch),
            Action::ListingCompleted(completion) => {
                effects.extend(self.complete(completion));
                effects.extend(self.fill_restore_reads());
            }
            Action::StorageCompleted(_)
            | Action::RetrySave
            | Action::WindowPlacementObserved(_)
            | Action::WindowPlacementAdjusted(_)
            | Action::WindowRestorationUnavailable(_)
            | Action::WindowCapturedForClose { .. }
            | Action::RestartStartup { .. }
            | Action::RequestClose
            | Action::CancelClose
            | Action::CloseWithoutSaving => {}
            Action::OpenAppSettings
            | Action::CancelAppSettings
            | Action::ApplyAppSettings { .. } => {}
            Action::OpenWorkspaceManagement
            | Action::CancelWorkspaceManagement
            | Action::SelectWorkspace(_)
            | Action::CreateWorkspace(_)
            | Action::RenameWorkspace { .. }
            | Action::RemoveWorkspace { .. }
            | Action::RetryWorkspaceManagement => {}
        }
        effects
    }

    fn finish_draw(&mut self) -> Vec<Effect> {
        let start = self.new_box_folder().to_path_buf();
        match self.workspace.finish_draw(start.clone()) {
            DrawOutcome::CreatedLauncher(_) => {
                configuration_changed(Change::LauncherCreated);
                self.save_effect(true)
            }
            DrawOutcome::Created(view) => {
                configuration_changed(Change::BoxCreated);
                let mut effects: Vec<_> = self
                    .request(view, start, NavigationIntent::Initialize)
                    .into_iter()
                    .collect();
                effects.extend(self.save_effect(true));
                effects
            }
            DrawOutcome::Invalid(crate::domain::DrawRejection::Overlap) => {
                self.status = Some("No room here. Draw in empty cells.".into());
                Vec::new()
            }
            DrawOutcome::Invalid(crate::domain::DrawRejection::ThinStrip) => {
                self.status = Some(
                    "Cannot create a 1-cell strip longer than 2 cells. Keep shortcuts within 2 × 2, or widen to at least 2 cells for a folder box.".into(),
                );
                Vec::new()
            }
            DrawOutcome::IdentityExhausted => {
                self.status = Some("Cannot create another item: the identifier limit is reached. Continue using existing items.".into());
                Vec::new()
            }
            DrawOutcome::CapacityReached => {
                self.status = Some(format!(
                    "Cannot create another item: the workspace has reached its {}-item limit. Remove an item and retry.",
                    Workspace::MAX_BOXES
                ));
                Vec::new()
            }
            DrawOutcome::ViewCapacityReached => {
                self.status = Some("Cannot create another box: the workspace view limit is reached. Close tabs or boxes and retry.".into());
                Vec::new()
            }
            DrawOutcome::None => Vec::new(),
        }
    }

    fn navigate(&mut self, view: ViewId, command: Navigate) -> Option<Effect> {
        if !self.workspace.can_browse(view) {
            return None;
        }
        let item = self.workspace.view(view)?;
        let nav = &item.navigation;
        let (target, intent) = match command {
            Navigate::Folder(path) => (path, NavigationIntent::Open),
            Navigate::Typed(input) => match expand_path(&input, nav.current(), &self.home) {
                Ok(path) => (path, NavigationIntent::Open),
                Err(error) => {
                    self.pending_configuration = None;
                    self.fail(view, error.to_string());
                    return None;
                }
            },
            Navigate::Back => (nav.back_target()?.to_path_buf(), NavigationIntent::Back),
            Navigate::Forward => (
                nav.forward_target()?.to_path_buf(),
                NavigationIntent::Forward,
            ),
            Navigate::Up => (nav.up_target()?.to_path_buf(), NavigationIntent::Open),
            Navigate::ReturnToStart => (nav.return_target().to_path_buf(), NavigationIntent::Open),
            Navigate::Refresh => (nav.current().to_path_buf(), NavigationIntent::Reload),
        };
        // Disabled history actions have no target and must not orphan a pending commit.
        // A valid navigation target (or invalid typed replacement above) supersedes it.
        if self
            .pending_configuration
            .as_ref()
            .is_some_and(|pending| pending.view == view)
        {
            self.pending_configuration = None;
            self.box_configuration_error = None;
        }
        if target == nav.current()
            && intent != NavigationIntent::Reload
            && !matches!(self.loads.get(&view), Some(LoadState::Failed { .. }))
            && let Some(listing) = self.loads.get(&view).and_then(LoadState::retained_listing)
        {
            // Unchanged navigation supersedes an earlier pending target without adding history.
            self.loads.insert(view, LoadState::Ready(listing));
            let path = self
                .workspace
                .view(view)?
                .navigation
                .current()
                .to_path_buf();
            self.demand_folder_usage(path, std::time::Instant::now());
            return None;
        }
        let effect = self.request(view, target, intent);
        if let Some(Effect::List(request)) = &effect {
            self.demand_usage_read(request.generation);
        }
        effect
    }

    fn request(
        &mut self,
        view: ViewId,
        directory: PathBuf,
        intent: NavigationIntent,
    ) -> Option<Effect> {
        let Some(generation) = self.next_generation.checked_add(1) else {
            self.fail(
                view,
                "Cannot load folder: request identity space exhausted. Wait for workspace saving, then relaunch."
                    .into(),
            );
            return None;
        };
        self.next_generation = generation;
        let request = ListingRequest {
            view,
            generation,
            directory,
        };
        let activity = match intent {
            NavigationIntent::Reload => Activity::Listing,
            NavigationIntent::Activate => Activity::Activate,
            NavigationIntent::Restore | NavigationIntent::Initialize => Activity::Restore,
            NavigationIntent::Configure => Activity::Configure,
            _ => Activity::Navigate,
        };
        session().record(
            Subsystem::Listing,
            activity,
            Outcome::Requested,
            generation,
            view.0,
            None,
        );
        let previous = self.loads.get(&view).and_then(LoadState::retained_listing);
        self.loads.insert(
            view,
            LoadState::Loading {
                request: request.clone(),
                intent,
                previous,
            },
        );
        Some(Effect::List(request))
    }

    fn fail(&mut self, view: ViewId, message: String) {
        let policy = self.safety_policy(Some(view));
        let previous = self
            .loads
            .get(&view)
            .and_then(LoadState::retained_listing)
            .filter(|listing| policy.check_read_directory(&listing.directory).is_ok());
        self.loads
            .insert(view, LoadState::Failed { message, previous });
    }

    pub(in crate::application) fn reconcile_safety_views(
        &mut self,
        previous_safety: crate::domain::SafetyPreferences,
    ) -> Vec<Effect> {
        if previous_safety == self.safety() {
            return Vec::new();
        }
        let home = self.home.clone();
        let next_safety = self.safety();
        let views: Vec<(ViewId, PathBuf)> = self
            .workspace
            .active_views()
            .map(|item| (item.view, item.navigation.current().to_path_buf()))
            .collect();
        let mut effects = Vec::new();
        for (view, current) in views {
            let was_allowed =
                crate::domain::safety::check_read_directory(&current, &home, previous_safety)
                    .is_ok();
            let now_allowed =
                crate::domain::safety::check_read_directory(&current, &home, next_safety).is_ok();
            if was_allowed != now_allowed
                && let Some(effect) = self.navigate(view, Navigate::Refresh)
            {
                effects.push(effect);
            }
        }
        effects
    }

    fn complete(&mut self, completion: ListingCompletion) -> Vec<Effect> {
        let view = completion.request.view;
        let Some(LoadState::Loading {
            request, intent, ..
        }) = self.loads.get(&view)
        else {
            session().record(
                Subsystem::Listing,
                Activity::Receipt,
                Outcome::MissingOwner,
                completion.request.generation,
                view.0,
                None,
            );
            return Vec::new();
        };
        if request != &completion.request {
            session().record(
                Subsystem::Listing,
                Activity::Receipt,
                Outcome::GenerationMismatch,
                completion.request.generation,
                view.0,
                None,
            );
            return Vec::new();
        }
        let intent = *intent;
        let usage_demand = self.finish_usage_read(completion.request.generation)
            || intent != NavigationIntent::Reload;
        self.restore_in_flight.remove(&view);
        if self.workspace.view(view).is_none() {
            return Vec::new();
        }
        match completion.result {
            Ok(listing) => {
                if let Err(reason) = self
                    .safety_policy(Some(view))
                    .check_read_directory(&listing.directory)
                {
                    if intent == NavigationIntent::Configure {
                        let effects = self.cancel_folder_configuration();
                        self.box_configuration_error = Some((view, reason.to_owned()));
                        return effects;
                    }
                    self.fail(view, reason.to_owned());
                    return Vec::new();
                }
                if !self.read_scope(view).allows(&listing.directory) {
                    if intent == NavigationIntent::Configure {
                        let effects = self.cancel_folder_configuration();
                        self.box_configuration_error = Some((view, "Cannot configure folder outside this Range. Edit its boundaries first.".into()));
                        return effects;
                    }
                    self.fail(view, "Cannot enter folder outside this navigation scope. Choose an allowed folder or edit the scope.".into());
                    return Vec::new();
                }
                if intent == NavigationIntent::Configure {
                    if configuration::validate_saved_path(&listing.directory).is_err() {
                        let effects = self.cancel_folder_configuration();
                        self.box_configuration_error = Some((view, "Cannot configure folder: resolved path exceeds the saved-path limit. Choose a shorter folder path.".into()));
                        return effects;
                    }
                    self.pending_configuration = None;
                    self.box_configuration_error = None;
                }
                let changed =
                    self.commit_listed_navigation(view, listing.directory.clone(), intent);
                let usage_path = (usage_demand
                    && listing.origin == directory::ListingOrigin::Local)
                    .then(|| listing.directory.clone());
                self.loads.insert(view, LoadState::Ready(Arc::new(listing)));
                if let Some(path) = usage_path {
                    self.demand_folder_usage(path, std::time::Instant::now());
                }
                if changed {
                    self.save_effect(true)
                } else {
                    Vec::new()
                }
            }
            Err(error) => {
                if intent == NavigationIntent::Configure {
                    let effects = self.cancel_folder_configuration();
                    self.box_configuration_error = Some((view, error.describe("configure folder")));
                    effects
                } else {
                    self.fail(view, error.to_string());
                    Vec::new()
                }
            }
        }
    }
}
