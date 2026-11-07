//! Iced runtime adapter and presentation. The application core has no Iced types.
//!
//! Submodules are grouped into `runtime` (background task owners and native menu bridges),
//! `window` (shell, placement, palettes, and fonts), `widgets` (reusable controls and
//! input/scroll primitives), `canvas` (workspace grid, header, toolbar, and layout gestures),
//! `boxes` (folder boxes, tab rails, shortcuts, list/grid/column views, and entry motion), `menus`
//! (context menus, sort popup, file prompts, and drag-and-drop), and `dialogs` (Settings,
//! Configure, Workspaces, Recovery, Updates, and About).

pub(crate) use file_tile_core::{
    GIT_COMMIT, VERSION, application, diagnostics, domain, filesystem, persistence, platform,
};

mod boxes;
mod canvas;
mod dialogs;
mod menus;
mod runtime;
#[cfg(test)]
mod tests;
mod widgets;
mod window;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use iced::event::{Event, Status};
use iced::keyboard;
use iced::widget::operation;
use iced::{Element, Point, Size, Subscription, Task, Vector, window as iced_window};

use crate::application::contracts::directory::{Cancellation, ListingRequest};
#[cfg(test)]
use crate::application::contracts::directory::{DirectoryReader, ListingCompletion};
use crate::application::contracts::storage::StartupOperation;
use crate::application::{Action, Application, Effect, LoadState, Navigate};
use crate::domain::ViewId;
#[cfg(test)]
use crate::filesystem::ControlledDirectory;
use crate::filesystem::{DirectoryWorkers, NativeDirectory};
use crate::persistence::StorageWorker;
use boxes::{file_list, launchers, tab_drag};
use canvas::{editing, geometry::CanvasGeometry, workspace};
use dialogs::{
    about, app_settings, box_settings, diagnostics as diagnostics_dialog, folder_picker,
    range_editor, storage_dialog, updates, workspace_management,
};
use menus::{context_menu, entry_menu, file_actions, file_drag, folder_menu, sort_menu, tab_menu};
use runtime::{
    folder_usage, input::WindowDrag, launch, metadata, native_menu, opening, sorting, startup,
    thumbnails,
};
use window::{
    appearance, system_font, typography, window_appearance, window_placement, window_shell,
};

#[derive(Debug, Clone)]
enum Message {
    Launcher(launchers::Event),
    DiagnosticLogs(diagnostics_dialog::Action),
    Updates(updates::Action),
    ToggleSort(ViewId),
    SortAnchor(u64, Option<iced::Rectangle>),
    DismissSort,
    SortChoice(u64, sort_menu::Choice),
    ChooseBoxFolder(ViewId),
    BoxFolderChosen(u64, Result<Option<PathBuf>, String>),
    BoxFormScrolled(ViewId, f32),
    CancelRange(ViewId),
    ThumbnailCompleted(crate::application::contracts::preview::PreviewCompletion),
    NameChanged(ViewId, u64, String),
    SubmitName(ViewId, u64),
    CancelName(ViewId, u64),
    TrashPrompt(ViewId, u64, bool),
    ConflictNameChanged(u64, u64, String),
    KeepBoth(u64, u64),
    WorkspaceEpoch(
        Option<crate::application::contracts::storage::SessionId>,
        Box<Message>,
    ),
    OpenWorkspaceManagement,
    ManagementNameChanged(String),
    CreateWorkspace,
    ChooseWorkspace(crate::application::contracts::configuration::WorkspaceId),
    PrepareWorkspaceRename(crate::application::contracts::configuration::WorkspaceId),
    WorkspaceRenameChanged(String),
    ConfirmWorkspaceRename,
    CancelWorkspaceRename,
    PrepareWorkspaceRemoval(crate::application::contracts::configuration::WorkspaceId),
    ConfirmWorkspaceRemoval,
    CancelWorkspaceRemoval,
    RetryWorkspaceManagement,
    CancelWorkspaceManagement,
    NativeQuit(Option<u64>),
    NativeMenu(Option<crate::application::contracts::menu::MenuRequest>),
    OpenTabMenu(crate::domain::BoxId, ViewId, Option<Point>),
    DismissTabMenu(u64),
    CloseMenuTab(u64),
    OpenEntryMenu(ViewId, PathBuf, Option<Point>),
    OpenFolderMenu(ViewId, Point),
    OpenPreviewFolderMenu(ViewId, Point),
    DismissFolderMenu(u64),
    FolderMenuAction(u64, folder_menu::Command),
    DismissEntryMenu(u64),
    ReplaceContextMenu(context_menu::Identity, Option<Box<Message>>),
    EntryMenuAction(u64, entry_menu::Command),
    ColumnNavigate(ViewId, PathBuf),
    EarlierAncestors(ViewId),
    ColumnAdvance(ViewId),
    ColumnPreviewCompleted(crate::application::contracts::directory::ListingCompletion),
    ColumnPreviewFocused(ViewId),
    ColumnPreviewSelected(ViewId, PathBuf),
    ColumnPreviewOpened(ViewId, PathBuf),
    ColumnPreviewHovered(ViewId, PathBuf, bool),
    ColumnPreviewScrolled(ViewId, f32),
    TreeChosen(ViewId, PathBuf),
    TreeScrolled(ViewId, f32),
    RangeStartChanged(ViewId, String),
    RangeStopsChanged(ViewId, iced::widget::text_editor::Action),
    CommitRange(ViewId),
    RangeStage(ViewId, range_editor::Stage),
    TabRailScrolled(crate::domain::BoxId, f32),
    FocusTab(ViewId),
    BeginTabDrag(crate::domain::BoxId, ViewId, Point),
    MoveTabDrag(Point),
    FinishTabDrag,
    Action(Action),
    Input(Event, Status),
    WindowEvent(iced_window::Id, iced_window::Event),
    WindowSampled {
        generation: u64,
        kind: window_placement::SampleKind,
        parts: Vec<window_placement::SamplePart>,
    },
    ViewportLaidOut(Size),
    WindowAppearanceObserved(window_appearance::Capability),
    SystemThemeChanged(iced::theme::Mode),
    FileViewportScrolled(ViewId, iced::widget::scrollable::AbsoluteOffset, f32),
    ListingScrollObserved {
        view: ViewId,
        generation: u64,
        window: file_list::ScrollWindow,
    },
    MotionFrame(std::time::Instant),
    CanvasScrolled(Vector),
    DragWindow,
    LayoutPressed {
        id: crate::domain::BoxId,
        kind: crate::domain::LayoutKind,
        position: Point,
    },
    LayoutPointerMoved(Point),
    FinishLayout,
    FolderChanges(Option<crate::application::contracts::watching::WatchBatch>),
    PathChanged(ViewId, String),
    PathSubmitted(ViewId),
    EntrySelected(ViewId, PathBuf),
    FolderFocused(ViewId),
    EntryOpened(ViewId, PathBuf),
    EntryHovered(ViewId, PathBuf, bool),
    FlushExternalDrop,
    StorageDetails(bool),
    PrepareStartup(StartupOperation),
    ConfirmStartup,
    CancelStartupConfirmation,
    About(about::Action),
    OpenAppSettings,
    AppSettingsChanged(String),
    AppAppearanceEdited(app_settings::AppearanceEdit),
    ChooseAppFolder,
    AppFolderChosen(u64, Result<Option<PathBuf>, String>),
    AppFontPicker(app_settings::FontPickerEdit),
    AppFontExplorer(app_settings::FontExplorerEdit),
    AppTypographyChanged(crate::application::preferences::typography::TypographyPreferences),
    AppAccessSafetyEdited(app_settings::AccessSafetyEdit),
    AppCapabilitiesChanged(crate::domain::CapabilityPreferences),
    ApplyAppSettings,
    CancelAppSettings,
    BoxFolderChanged(ViewId, String),
    CommitBoxFolder(ViewId),
    BlurBoxFolder(ViewId),
    BeginCardDrag,
    EndCardDrag,
    PathFocusChecked {
        focused: Option<ViewId>,
        box_folder: bool,
        boundary: Option<range_editor::Field>,
        version: u64,
    },
}

#[derive(Debug, Default)]
struct ViewPresentation {
    prompt_anchor: Option<Point>,
    name_draft: Option<file_actions::NameDraft>,
    trash_draft: Option<file_actions::TrashDraft>,
    selection: std::collections::HashSet<PathBuf>,
    pending_selection: std::collections::HashSet<PathBuf>,
    reveal_selected: bool,
    selection_anchor: Option<PathBuf>,
    draft: Option<String>,
    draft_revision: u64,
    submitted_revision: Option<(u64, u64)>,
    invalid: bool,
    selected: Option<PathBuf>,
    hovered: Option<PathBuf>,
    hint: Option<String>,
    scroll: file_list::ScrollWindow,
    scroll_x: f32,
    listing_revision: u64,
    earlier_ancestors: bool,
    column_focus: Option<boxes::file_columns::ColumnFocus>,
    motion: Option<boxes::entry_motion::ViewMotion>,
}

enum DirectoryBackend {
    #[cfg(test)]
    Controlled(ControlledDirectory),
    Native(Arc<DirectoryWorkers>),
}

#[cfg(test)]
impl DirectoryBackend {
    fn list(
        &self,
        path: &std::path::Path,
    ) -> Result<
        crate::application::contracts::directory::DirectoryListing,
        crate::application::contracts::directory::DirectoryError,
    > {
        match self {
            Self::Controlled(reader) => reader.list(path),
            Self::Native(_) => Err(
                crate::application::contracts::directory::DirectoryError::new(
                    path.to_path_buf(),
                    crate::application::contracts::directory::DirectoryErrorKind::Unavailable,
                ),
            ),
        }
    }
}

struct PendingRead {
    request: ListingRequest,
    cancellation: Cancellation,
    handle: iced::task::Handle,
}

impl Drop for PendingRead {
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.handle.abort();
    }
}

struct Desktop {
    launchers: launchers::Runtime,
    logs: diagnostics_dialog::Logs,
    updates: updates::Updates,
    usage_runtime: folder_usage::Runtime,
    sort_runtime: sorting::Runtime,
    column_previews: boxes::file_columns::Runtime,
    sort_menu: Option<sort_menu::Menu>,
    next_sort_menu_generation: u64,
    thumbnails: thumbnails::Runtime,
    file_worker: Option<Arc<crate::filesystem::file_operations::FileOperationWorker>>,
    file_waiter: Option<(u64, iced::task::Handle)>,
    conflict_name: Option<(u64, u64, String)>,
    modifiers: keyboard::Modifiers,
    system_font: system_font::SystemFont,
    native_menu: native_menu::Runtime,
    tab_menu: Option<tab_menu::Menu>,
    next_tab_menu_generation: u64,
    entry_menu: Option<entry_menu::Menu>,
    next_entry_menu_generation: u64,
    folder_menu: Option<folder_menu::Menu>,
    next_folder_menu_generation: u64,
    open_workers: Option<Arc<crate::filesystem::OpenWorkers>>,
    pending_open: Option<opening::Pending>,
    metadata_workers: Option<Arc<crate::filesystem::MetadataWorkers>>,
    metadata_reads: HashMap<ViewId, metadata::Pending>,
    configuration_reads: HashMap<u64, PendingRead>,
    application: Application,
    directory: DirectoryBackend,
    pending_reads: HashMap<ViewId, PendingRead>,
    views: HashMap<ViewId, ViewPresentation>,
    active_view: Option<ViewId>,
    focused_path: Option<ViewId>,
    focus_version: u64,
    size: Size,
    pointer: Option<Point>,
    window_drag: WindowDrag,
    canvas_offset: Vector,
    storage: Option<Arc<StorageWorker>>,
    storage_root: Option<PathBuf>,
    storage_details: bool,
    startup_confirmation: Option<StartupOperation>,
    window_runtime: window_placement::WindowRuntime,
    settings_picker: folder_picker::Picker,
    window_appearance: window_appearance::Capability,
    system_appearance: crate::application::preferences::appearance::SystemAppearance,
    about: Option<about::State>,
    app_settings: Option<app_settings::Draft>,
    workspace_management: Option<workspace_management::Draft>,
    box_settings: Option<box_settings::Draft>,
    tab_offsets: HashMap<crate::domain::BoxId, f32>,
    tab_drag: Option<tab_drag::Drag>,
    file_drag: file_drag::State,
    edit_anchor: Option<editing::Anchor>,
    folder_watcher: Option<Arc<crate::filesystem::DirectoryWatcher>>,
    watch_listener: Option<iced::task::Handle>,
}

#[cfg(test)]
impl Default for Desktop {
    fn default() -> Self {
        Self::new(
            Application::new("/Users/demo/Documents".into(), "/Users/demo".into()),
            DirectoryBackend::Controlled(ControlledDirectory::default()),
        )
    }
}

impl Desktop {
    fn new(application: Application, directory: DirectoryBackend) -> Self {
        Self {
            launchers: launchers::Runtime::default(),
            logs: diagnostics_dialog::Logs::default(),
            updates: updates::Updates::default(),
            usage_runtime: folder_usage::Runtime::default(),
            sort_runtime: sorting::Runtime::default(),
            column_previews: boxes::file_columns::Runtime::default(),
            sort_menu: None,
            next_sort_menu_generation: 0,
            thumbnails: thumbnails::Runtime::default(),
            file_worker: None,
            file_waiter: None,
            conflict_name: None,
            modifiers: keyboard::Modifiers::empty(),
            system_font: system_font::SystemFont::default(),
            native_menu: native_menu::Runtime::default(),
            tab_menu: None,
            next_tab_menu_generation: 0,
            entry_menu: None,
            next_entry_menu_generation: 0,
            folder_menu: None,
            next_folder_menu_generation: 0,
            open_workers: None,
            pending_open: None,
            metadata_workers: None,
            metadata_reads: HashMap::new(),
            configuration_reads: HashMap::new(),
            application,
            directory,
            pending_reads: HashMap::new(),
            views: HashMap::new(),
            active_view: None,
            focused_path: None,
            focus_version: 0,
            size: window_shell::INITIAL_SIZE,
            pointer: None,
            window_drag: WindowDrag::Idle,
            canvas_offset: Vector::ZERO,
            storage: None,
            storage_root: None,
            storage_details: false,
            startup_confirmation: None,
            window_runtime: window_placement::WindowRuntime::default(),
            settings_picker: folder_picker::Picker::default(),
            window_appearance: window_appearance::Capability::default(),
            system_appearance:
                crate::application::preferences::appearance::SystemAppearance::default(),
            about: None,
            app_settings: None,
            workspace_management: None,
            box_settings: None,
            tab_offsets: HashMap::new(),
            tab_drag: None,
            file_drag: file_drag::State::default(),
            edit_anchor: None,
            folder_watcher: None,
            watch_listener: None,
        }
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let launch = launch::Launch::from_env()?;
    let log_writer = crate::diagnostics::session().start(&launch.state_dir).inspect_err(|error| {
        eprintln!("Cannot start diagnostic logging ({:?}). Check the application data folder and relaunch. The application will continue without file logging.", error.kind());
    }).ok();
    crate::diagnostics::install_panic_hook();
    let result = run_desktop(launch);
    if result.is_err() {
        crate::diagnostics::session().record(
            crate::diagnostics::Subsystem::Session,
            crate::diagnostics::Activity::Shutdown,
            crate::diagnostics::Outcome::Failed,
            0,
            0,
            None,
        );
    } else if let Some(writer) = log_writer {
        writer.finish_healthy();
    }
    result
}

fn run_desktop(launch: launch::Launch) -> Result<(), Box<dyn std::error::Error>> {
    let system_font = system_font::SystemFont::load();
    let workers = Arc::new(DirectoryWorkers::new(Arc::new(NativeDirectory), 2, 8)
        .map_err(|error| std::io::Error::other(format!("Cannot start directory workers: {error}. Relaunch and collect error output if it persists.")))?);
    let metadata_workers = Arc::new(crate::filesystem::MetadataWorkers::new(Arc::new(
        NativeDirectory,
    ))?);
    let open_workers = Arc::new(crate::filesystem::OpenWorkers::new()?);
    let sort_workers = Arc::new(crate::filesystem::sorting::SortWorkers::new(Arc::new(
        NativeDirectory,
    ))?);
    let file_worker = Arc::new(crate::filesystem::file_operations::FileOperationWorker::new()?);
    let thumbnail_workers = Arc::new(crate::filesystem::PreviewWorkers::new()?);
    let usage_workers = Arc::new(crate::filesystem::folder_usage::UsageWorkers::new()?);
    let boot = move || {
        let (mut desktop, task) =
            startup::boot(&launch, DirectoryBackend::Native(Arc::clone(&workers)));
        desktop.metadata_workers = Some(Arc::clone(&metadata_workers));
        desktop.sort_runtime.workers = Some(Arc::clone(&sort_workers));
        desktop.usage_runtime.workers = Some(Arc::clone(&usage_workers));
        desktop.open_workers = Some(Arc::clone(&open_workers));
        desktop.file_worker = Some(Arc::clone(&file_worker));
        desktop.thumbnails.workers = Some(Arc::clone(&thumbnail_workers));
        desktop.system_font = system_font.clone();
        (
            desktop,
            Task::batch([task, iced::system::theme().map(Message::SystemThemeChanged)]),
        )
    };
    iced::application(boot, Desktop::update, Desktop::view)
        .subscription(Desktop::subscription)
        .title("file-tile")
        .window(window_shell::settings())
        .style(|desktop: &Desktop, _| {
            let palettes = appearance::resolve(desktop.preview_appearance());
            iced::theme::Style {
                background_color: desktop
                    .window_appearance
                    .clear_color(palettes.window.background),
                text_color: palettes.window.text,
            }
        })
        .default_font(typography::Style::default().font())
        .font(typography::ARGON_REGULAR)
        .font(typography::ARGON_MEDIUM)
        .font(typography::CANTARELL_REGULAR)
        .font(typography::CANTARELL_BOLD)
        .centered()
        .run()?;
    Ok(())
}

impl Desktop {
    fn geometry(&self) -> CanvasGeometry {
        CanvasGeometry::new(self.size, self.canvas_offset, self.application.workspace())
            .with_typography(self.application.typography())
    }

    fn view(&self) -> Element<'_, Message> {
        window_appearance::observe(workspace::view(self), Message::WindowAppearanceObserved)
    }

    fn has_active_entry_motion(&self) -> bool {
        self.file_drag.is_animating()
            || self.views.values().any(|state| state.motion.is_some())
            || self
                .entry_menu
                .as_ref()
                .is_some_and(entry_menu::Menu::is_hover_waiting)
    }

    fn tick_entry_motion(&mut self, now: std::time::Instant) {
        self.file_drag.tick(now);
        if let Some(menu) = &mut self.entry_menu {
            menu.tick(now);
        }
        for state in self.views.values_mut() {
            if state
                .motion
                .as_mut()
                .is_some_and(|motion| !motion.tick(now))
            {
                state.motion = None;
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let motion = if self.has_active_entry_motion() {
            iced_window::frames().map(Message::MotionFrame)
        } else {
            Subscription::none()
        };
        Subscription::batch([
            iced::event::listen_with(|event, status, id| match event {
                Event::Window(event) => Some(Message::WindowEvent(id, event)),
                Event::Mouse(_) | Event::Keyboard(_) => Some(Message::Input(event, status)),
                _ => None,
            }),
            iced::system::theme_changes().map(Message::SystemThemeChanged),
            motion,
        ])
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        if let Message::MotionFrame(now) = message {
            self.tick_entry_motion(now);
            return Task::none();
        }
        if let Message::WorkspaceEpoch(epoch, message) = message {
            return if epoch == self.application.workspace_epoch() {
                self.update(*message)
            } else {
                crate::diagnostics::session().record(
                    crate::diagnostics::Subsystem::Session,
                    crate::diagnostics::Activity::Receipt,
                    crate::diagnostics::Outcome::WorkspaceMismatch,
                    0,
                    0,
                    None,
                );
                Task::none()
            };
        }
        let task = match message {
            Message::Launcher(event) => self.launcher_event(event),
            Message::DiagnosticLogs(action) => self.diagnostic_logs_action(action),
            Message::Updates(action) => self.updates_action(action),
            Message::ThumbnailCompleted(receipt) => self.thumbnail_completed(receipt),
            Message::FlushExternalDrop => self.flush_external_drop(),
            Message::NativeQuit(epoch) => self.native_quit(epoch),
            Message::NativeMenu(request) => self.native_menu(request),
            Message::WindowAppearanceObserved(capability) => {
                self.window_appearance = capability;
                Task::none()
            }
            Message::SystemThemeChanged(mode) => {
                let next = appearance::system_appearance(mode);
                self.system_appearance = next;
                if let Some(draft) = &mut self.app_settings {
                    draft.observe_system_appearance(next);
                }
                Task::none()
            }
            message => self.update_message(message),
        };
        let outbound = self.reconcile_outbound_drag();
        let picker = self.reconcile_picker();
        let column_previews = self.sync_column_previews();
        self.sync_updates();
        self.sync_application_menu();
        self.sync_diagnostics();
        Task::batch([task, outbound, picker, column_previews])
    }

    fn active_picker_generation(&self) -> Option<u64> {
        if let Some(draft) = &self.app_settings {
            if self.modal_open()
                && self.native_menu.error().is_none()
                && !(self.storage_details && self.application.storage_detail().is_some())
            {
                return draft.picker();
            }
            return None;
        }
        if !self.workspace_input_enabled() {
            return None;
        }
        self.box_settings
            .as_ref()
            .filter(|draft| !draft.range_page)
            .and_then(box_settings::Draft::picker)
            .or_else(|| {
                self.launchers
                    .draft
                    .as_ref()
                    .and_then(launchers::Draft::picker)
            })
    }

    fn clear_active_pickers(&mut self) {
        self.clear_pickers_except(None);
    }

    fn clear_pickers_except(&mut self, active: Option<u64>) {
        if let Some(draft) = &mut self.app_settings
            && (active.is_none() || draft.picker() != active)
        {
            draft.clear_picker();
        }
        if let Some(draft) = &mut self.box_settings
            && (active.is_none() || draft.picker() != active)
        {
            draft.clear_picker();
        }
        if let Some(draft) = &mut self.launchers.draft
            && (active.is_none() || draft.picker() != active)
        {
            draft.clear_picker();
        }
    }

    fn reconcile_picker(&mut self) -> Task<Message> {
        let active = self.active_picker_generation();
        self.clear_pickers_except(active);
        self.settings_picker.reconcile(active)
    }

    fn update_message(&mut self, message: Message) -> Task<Message> {
        match message {
            message @ (Message::ToggleSort(_)
            | Message::SortAnchor(..)
            | Message::DismissSort
            | Message::SortChoice(..)) => self.sort_menu_message(message),
            message @ (Message::NameChanged(..)
            | Message::SubmitName(..)
            | Message::CancelName(..)
            | Message::TrashPrompt(..)
            | Message::ConflictNameChanged(..)
            | Message::KeepBoth(..)) => self.file_action_message(message),
            message @ (Message::OpenTabMenu(..)
            | Message::DismissTabMenu(_)
            | Message::CloseMenuTab(_)
            | Message::OpenEntryMenu(..)
            | Message::OpenFolderMenu(..)
            | Message::OpenPreviewFolderMenu(..)
            | Message::DismissFolderMenu(_)
            | Message::FolderMenuAction(..)
            | Message::DismissEntryMenu(_)
            | Message::ReplaceContextMenu(..)
            | Message::EntryMenuAction(..)) => self.context_menu_message(message),
            message @ (Message::ColumnNavigate(..)
            | Message::EarlierAncestors(_)
            | Message::ColumnAdvance(_)
            | Message::ColumnPreviewCompleted(_)
            | Message::ColumnPreviewFocused(_)
            | Message::ColumnPreviewSelected(..)
            | Message::ColumnPreviewOpened(..)
            | Message::ColumnPreviewHovered(..)
            | Message::ColumnPreviewScrolled(..)) => self.column_message(message),
            message @ (Message::BeginTabDrag(..)
            | Message::MoveTabDrag(_)
            | Message::FinishTabDrag
            | Message::TabRailScrolled(..)
            | Message::FocusTab(_)) => self.tab_message(message),
            Message::FolderChanges(batch) => self.folder_changes(batch),
            message @ (Message::LayoutPressed { .. }
            | Message::LayoutPointerMoved(_)
            | Message::FinishLayout) => self.editing_message(message),
            Message::Action(action) => self.dispatch(action),
            Message::Input(event, status) => self.input(event, status),
            message @ (Message::WindowEvent(_, _) | Message::WindowSampled { .. }) => {
                self.window_message(message)
            }
            Message::ViewportLaidOut(size) => self.adopt_viewport(size),
            message @ (Message::FileViewportScrolled(..)
            | Message::ListingScrollObserved { .. }
            | Message::FolderFocused(_)
            | Message::EntrySelected(..)
            | Message::EntryOpened(..)
            | Message::EntryHovered(..)) => self.listing_message(message),
            Message::CanvasScrolled(offset) => self.canvas_scrolled(offset),
            Message::DragWindow => self.drag_window(),
            message @ (Message::PathChanged(_, _) | Message::PathSubmitted(_)) => {
                self.path_message(message)
            }
            message @ Message::PathFocusChecked { .. } => self.path_focus_checked(message),
            message => self.update_dialog_message(message),
        }
    }

    fn update_dialog_message(&mut self, message: Message) -> Task<Message> {
        match message {
            message @ (Message::OpenWorkspaceManagement
            | Message::ManagementNameChanged(_)
            | Message::CreateWorkspace
            | Message::ChooseWorkspace(_)
            | Message::PrepareWorkspaceRename(_)
            | Message::WorkspaceRenameChanged(_)
            | Message::ConfirmWorkspaceRename
            | Message::CancelWorkspaceRename
            | Message::PrepareWorkspaceRemoval(_)
            | Message::ConfirmWorkspaceRemoval
            | Message::CancelWorkspaceRemoval
            | Message::RetryWorkspaceManagement
            | Message::CancelWorkspaceManagement) => self.workspace_management_message(message),
            Message::About(action) => self.about_message(action),
            message @ (Message::OpenAppSettings
            | Message::AppSettingsChanged(_)
            | Message::AppAppearanceEdited(_)
            | Message::AppFontPicker(_)
            | Message::AppFontExplorer(_)
            | Message::AppTypographyChanged(_)
            | Message::AppAccessSafetyEdited(_)
            | Message::AppCapabilitiesChanged(_)
            | Message::ChooseAppFolder
            | Message::AppFolderChosen(..)
            | Message::ApplyAppSettings
            | Message::CancelAppSettings) => self.app_settings_message(message),
            message @ (Message::BoxFolderChanged(..)
            | Message::TreeChosen(..)
            | Message::TreeScrolled(..)
            | Message::RangeStartChanged(..)
            | Message::CancelRange(_)
            | Message::RangeStopsChanged(..)
            | Message::CommitRange(_)
            | Message::RangeStage(..)
            | Message::ChooseBoxFolder(_)
            | Message::BoxFolderChosen(..)
            | Message::BoxFormScrolled(..)
            | Message::CommitBoxFolder(_)
            | Message::BlurBoxFolder(_)
            | Message::BeginCardDrag
            | Message::EndCardDrag) => self.box_settings_message(message),
            message @ (Message::StorageDetails(_)
            | Message::PrepareStartup(_)
            | Message::ConfirmStartup
            | Message::CancelStartupConfirmation) => self.storage_message(message),
            _ => Task::none(),
        }
    }

    fn dispatch(&mut self, action: Action) -> Task<Message> {
        if !self.permits_ui_action(&action) {
            return Task::none();
        }
        self.observe_thumbnail_demand(&action);
        let mut effects = self.suspend_configuration_if_modal();
        if self.cancel_tab_drag_for(&action) {
            return Task::none();
        }
        self.supersede_box_folder_blur(&action);
        let save_failed_before = self.application.save_failed();
        let recovery_visible_before =
            self.storage_details && self.application.storage_detail().is_some();
        let epoch_before = self.application.workspace_epoch();
        let active_before: HashMap<_, _> = self
            .application
            .workspace()
            .boxes()
            .iter()
            .map(|item| (item.id, item.active_view().view))
            .collect();
        let settings_before = self.visibility_before(&action);
        let undo_before = self.undo_views_before(&action);
        let restored = matches!((&action, self.application.persistence()),
            (Action::StorageCompleted(crate::application::contracts::storage::StorageCompletion::Loaded { session, .. }),
            crate::application::contracts::storage::PersistenceState::Loading(current)) if session == current);
        let closing = matches!(action, Action::RequestClose);
        let clear_focus = self.prepare_action_focus(&action);
        if matches!(action, Action::SelectMode(_) | Action::Escape) {
            for presentation in self.views.values_mut() {
                presentation.hovered = None;
            }
        }
        let typed_view = match &action {
            Action::Navigate(view, Navigate::Typed(_))
                if self.application.workspace().can_browse(*view) =>
            {
                Some(*view)
            }
            _ => None,
        };
        let completed_view = self.listing_completion_context(&action);
        effects.extend(self.application.update(action));
        let switched = epoch_before != self.application.workspace_epoch() && !restored;
        if switched {
            self.clear_workspace_context();
        }
        self.sync_editing_views(undo_before);
        let (recovery_opened, modal_effects) = self.reconcile_runtime_context(
            restored,
            closing,
            !save_failed_before && self.application.save_failed(),
            recovery_visible_before,
        );
        effects.extend(modal_effects);
        let mut task = self.complete_listing_task(completed_view);
        self.sync_box_settings();
        self.sync_launchers();
        self.reconcile_context_menus();
        task = Task::batch([
            task,
            self.reconcile_box_settings(settings_before),
            self.effects(effects),
        ]);
        if restored {
            task = Task::batch([task, self.restore_window_if_ready()]);
        }
        if let Some(view) = typed_view
            && matches!(self.application.load(view), Some(LoadState::Failed { .. }))
        {
            self.views.entry(view).or_default().invalid = true;
        }
        if switched {
            task = Task::batch([
                task,
                if self.workspace_management.is_some() {
                    operation::focus("workspace-management-cancel")
                } else {
                    operation::focus("workspace-canvas")
                },
            ]);
        }
        if clear_focus {
            task = Task::batch([task, self.focus_canvas()]);
        }
        if recovery_opened {
            task = Task::batch([
                task,
                storage_dialog::focus_initial(&self.application, self.startup_confirmation),
            ]);
        }
        Task::batch([
            task,
            self.restore_box_form(),
            self.fill_thumbnails(),
            if restored || switched {
                Task::none()
            } else {
                self.reconcile_tab_activation(active_before)
            },
        ])
    }

    fn clear_workspace_context(&mut self) {
        if let Some(draft) = &mut self.workspace_management {
            draft.clear_return_focus();
        }
        self.thumbnails.clear();
        self.column_previews.clear();
        self.application.clear_column_previews();
        self.clear_context_menus();
        self.reset_restored_views();
        self.configuration_reads.clear();
        self.metadata_reads.clear();
        self.pending_open = None;
        self.launchers = launchers::Runtime::default();
        self.box_settings = None;
        self.tab_drag = None;
        self.file_drag.clear();
        self.edit_anchor = None;
    }

    fn reconcile_runtime_context(
        &mut self,
        restored: bool,
        closing: bool,
        new_save_failure: bool,
        recovery_visible_before: bool,
    ) -> (bool, Vec<Effect>) {
        self.focused_path = self
            .focused_path
            .filter(|view| self.application.workspace().can_browse(*view));
        if matches!(
            self.application.persistence(),
            crate::application::contracts::storage::PersistenceState::Loading(_)
        ) {
            self.pending_reads.clear();
            self.storage_details = false;
            self.startup_confirmation = None;
        }
        if restored {
            self.reset_restored_views();
        }
        if closing {
            self.pending_reads.clear();
            self.column_previews.clear_pending();
        }
        self.sync_configuration_dialogs(restored, new_save_failure);
        let recovery_opened = !recovery_visible_before
            && self.storage_details
            && self.application.storage_detail().is_some();
        if recovery_opened {
            self.focused_path = None;
            self.focus_version = self.focus_version.saturating_add(1);
        }
        if !self.application.workspace_management_open() {
            self.workspace_management = None;
        }
        let effects = self.suspend_configuration_if_modal();
        self.retain_configuration_reads();
        self.pending_reads.retain(|view, pending| matches!(self.application.load(*view), Some(LoadState::Loading { request, .. }) if request == &pending.request));
        (recovery_opened, effects)
    }

    fn sync_configuration_dialogs(&mut self, restored: bool, new_save_failure: bool) {
        if new_save_failure
            || self.application.close_failed()
            || restored && self.application.can_retry_startup()
        {
            self.about = None;
            self.application.update(Action::CancelAppSettings);
            self.app_settings = None;
            if !self.application.workspace_management_pending() {
                self.application.update(Action::CancelWorkspaceManagement);
                self.workspace_management = None;
            }
            self.storage_details = true;
        } else if self.application.storage_detail().is_none() {
            self.storage_details = false;
        }
        if !self.application.accepts_edits() {
            self.about = None;
        }
        if !self.application.app_settings_open() {
            self.app_settings = None;
        }
        if self.modal_open() {
            self.window_drag = WindowDrag::Idle;
            self.cancel_card_drag();
            self.tab_drag = None;
            self.file_drag.clear();
        }
    }
}
