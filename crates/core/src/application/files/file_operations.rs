//! One application-owned write batch. Native calls, clipboard, and prompts are adapters.
use crate::application::{Action, Application, Effect, LoadState, Navigate};
use crate::diagnostics::{Activity, Outcome as DiagnosticOutcome, Subsystem, session};
use crate::domain::{
    ViewId,
    files::{CreateKind, validate_name},
};
use std::{collections::VecDeque, path::PathBuf};

pub const ITEM_LIMIT: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Copy(PathBuf),
    Move(PathBuf),
    Create(CreateKind),
    Rename(PathBuf),
    Trash(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    pub device: u64,
    pub inode: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub operation: u64,
    pub step: u64,
    pub directory: PathBuf,
    pub parent: Option<Identity>,
    pub rename_identity: Option<Identity>,
    pub safety: crate::domain::SafetyPolicy,
    pub item: Item,
    pub name: String,
}

#[derive(Debug, Clone)]
pub enum Outcome {
    Complete,
    Conflict,
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct Completion {
    pub job: Job,
    pub parent: Option<Identity>,
    pub rename_identity: Option<Identity>,
    pub outcome: Outcome,
}

#[derive(Debug, Clone)]
pub enum ConflictChoice {
    KeepBoth(String),
    Skip,
    Cancel,
}

#[derive(Debug)]
pub enum Phase {
    Running(Job),
    Conflict(Job),
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemStatus {
    Complete,
    Skipped,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone)]
pub struct ItemOutcome {
    pub destination: PathBuf,
    pub status: ItemStatus,
}

#[derive(Debug)]
pub struct Operation {
    pub id: u64,
    pub view: ViewId,
    pub directory: PathBuf,
    pub phase: Phase,
    pub completed: usize,
    pub skipped: usize,
    pub total: usize,
    pub stopped: bool,
    pub error: Option<String>,
    pub outcomes: Vec<ItemOutcome>,
    pending: VecDeque<(Item, String)>,
    parent: Option<Identity>,
    safety: crate::domain::SafetyPolicy,
    step: u64,
}

impl Operation {
    pub fn active(&self) -> bool {
        !matches!(self.phase, Phase::Finished)
    }
    fn next(&mut self) -> Option<Job> {
        if self.stopped || self.error.is_some() {
            for (_, name) in self.pending.drain(..) {
                self.outcomes.push(ItemOutcome {
                    destination: self.directory.join(name),
                    status: ItemStatus::Cancelled,
                });
            }
        }
        let Some((item, name)) = self.pending.pop_front() else {
            self.phase = Phase::Finished;
            return None;
        };
        let Some(step) = self.step.checked_add(1) else {
            self.error =
                Some("File-operation identity limit reached. Inspect results and relaunch.".into());
            self.phase = Phase::Finished;
            return None;
        };
        self.step = step;
        let job = Job {
            operation: self.id,
            step: self.step,
            directory: self.directory.clone(),
            parent: self.parent,
            rename_identity: None,
            safety: self.safety.clone(),
            item,
            name,
        };
        self.phase = Phase::Running(job.clone());
        Some(job)
    }

    fn record(&mut self, job: &Job, status: ItemStatus) {
        self.outcomes.push(ItemOutcome {
            destination: self.directory.join(&job.name),
            status,
        });
    }
}

#[derive(Debug, Default)]
pub(in crate::application) struct Operations {
    pub current: Option<Operation>,
    next_id: u64,
    pub close_requested: bool,
}

impl Application {
    pub fn safety_policy(&self, view: Option<ViewId>) -> crate::domain::SafetyPolicy {
        let prohibit_delete = view
            .and_then(|view| self.workspace.view(view))
            .is_some_and(|view| view.settings.prohibit_delete);
        let mut external_drive_roots = Vec::new();
        let mut network_share_roots = Vec::new();
        for loc in &self.locations {
            match loc.kind {
                crate::platform::volumes::LocationKind::ExternalDrive => {
                    external_drive_roots.push(loc.path.clone());
                }
                crate::platform::volumes::LocationKind::NetworkDrive { .. } => {
                    network_share_roots.push(loc.path.clone());
                }
                crate::platform::volumes::LocationKind::Cloud(_) => {}
            }
        }
        crate::domain::SafetyPolicy {
            preferences: self.safety(),
            home: self.home.clone(),
            state_dir: Some(self.state_dir.clone()),
            prohibit_delete,
            external_drive_roots,
            network_share_roots,
        }
    }

    pub fn write_folder_guidance(
        &self,
        view: ViewId,
        directory: &std::path::Path,
    ) -> Result<(), &'static str> {
        self.safety_policy(Some(view))
            .check_write_directory(directory)
    }

    fn is_workspace_referenced_folder(&self, source: &std::path::Path) -> bool {
        self.loads.values().any(|load| {
            matches!(load, LoadState::Loading { request, .. } if request.directory.starts_with(source))
        }) || self.workspace.visible_views().any(|view| {
            let nav = &view.navigation;
            nav.current().starts_with(source)
                || nav.scope().root().is_some_and(|path| path.starts_with(source))
                || nav.scope().range().is_some_and(|range| {
                    range.start().starts_with(source)
                        || range.stops().iter().any(|path| path.starts_with(source))
                })
        })
    }

    fn is_protected_cloud_root(&self, source: &std::path::Path) -> bool {
        !self.safety().system_folders
            && self.locations.iter().any(|loc| {
                matches!(loc.kind, crate::platform::volumes::LocationKind::Cloud(_))
                    && loc.path == source
            })
    }

    pub fn move_guidance(&self, source: &std::path::Path) -> Result<(), &'static str> {
        if !source.is_absolute() || source.to_str().is_none() || source.file_name().is_none() {
            return Err("Cannot move this native path exactly. Use Finder for this entry.");
        }
        if self.is_protected_cloud_root(source) {
            return Err(
                "System folder protection blocks moving or renaming this item. You can change this in Application Settings.",
            );
        }
        self.safety_policy(None).check_entry_modification(source)?;
        if self.is_workspace_referenced_folder(source) {
            return Err(
                "This folder is open in a box or used by a navigation scope. Navigate away or update the scope before moving it.",
            );
        }
        Ok(())
    }

    fn listing_for_entry(
        &self,
        view: ViewId,
        source: &std::path::Path,
    ) -> Option<&crate::application::contracts::directory::DirectoryListing> {
        let primary = self.load(view).and_then(LoadState::listing);
        if let Some(listing) = primary
            && source.parent() == Some(listing.directory.as_path())
        {
            return Some(listing);
        }
        let preview = self.valid_column_preview(view)?.as_ref();
        if source.parent() == Some(preview.directory.as_path()) {
            return Some(preview);
        }
        primary
    }

    fn has_ready_directory(&self, view: ViewId, directory: &std::path::Path) -> bool {
        let current = self.workspace.view(view).map(|v| v.navigation.current());
        let primary_ready = current.is_some_and(|cur| {
            matches!(self.load(view), Some(LoadState::Ready(listing)) if listing.directory == cur)
        });
        if current == Some(directory) {
            return primary_ready;
        }
        if primary_ready
            && self
                .valid_column_preview(view)
                .is_some_and(|listing| listing.directory == directory)
        {
            return true;
        }
        self.is_sidebar_target(view, directory)
    }

    fn is_sidebar_target(&self, view: ViewId, directory: &std::path::Path) -> bool {
        let Some(item) = self.workspace.box_for_view(view) else {
            return false;
        };
        item.standalone.view == view
            && item.has_sidebar()
            && item.standalone.navigation.scope().allows(directory)
            && (self.pinned_folders().iter().any(|path| path == directory)
                || self.locations.iter().any(|loc| loc.path == directory))
    }

    pub fn rename_guidance(
        &self,
        view: ViewId,
        source: &std::path::Path,
    ) -> Result<(), &'static str> {
        let listing = self
            .listing_for_entry(view, source)
            .ok_or("Reload the folder before renaming.")?;
        let entry = listing
            .entry(source)
            .ok_or("This entry is no longer listed. Reload before renaming.")?;
        if source.parent() != Some(listing.directory.as_path())
            || source.to_str().is_none()
            || (entry.hidden
                && !self
                    .workspace
                    .view(view)
                    .is_some_and(|view| view.settings.show_hidden))
            || (entry.kind == crate::application::contracts::directory::EntryKind::Other
                && !entry.symlink)
        {
            return Err("Cannot rename this native entry exactly. Use Finder for this entry.");
        }
        if self.is_protected_cloud_root(source) {
            return Err(
                "System folder protection blocks moving or renaming this item. You can change this in Application Settings.",
            );
        }
        self.safety_policy(Some(view))
            .check_entry_modification(source)?;
        if entry.kind == crate::application::contracts::directory::EntryKind::Directory
            && !entry.symlink
            && self.is_workspace_referenced_folder(source)
        {
            return Err(
                "This folder is open in a box or used by a navigation scope. Navigate away or update the scope before renaming.",
            );
        }
        Ok(())
    }

    pub fn trash_guidance(
        &self,
        view: ViewId,
        source: &std::path::Path,
    ) -> Result<(), &'static str> {
        let listing = self
            .listing_for_entry(view, source)
            .ok_or("Reload the folder before moving items to Trash.")?;
        let entry = listing
            .entry(source)
            .ok_or("This entry is no longer listed. Reload before moving to Trash.")?;
        if source.parent() != Some(listing.directory.as_path())
            || source.to_str().is_none()
            || source.file_name().is_none()
            || (entry.hidden
                && !self
                    .workspace
                    .view(view)
                    .is_some_and(|view| view.settings.show_hidden))
            || (entry.kind == crate::application::contracts::directory::EntryKind::Other
                && !entry.symlink)
        {
            return Err("Cannot move this native entry to Trash. Use Finder for this entry.");
        }
        if self.is_protected_cloud_root(source) {
            return Err(
                "System folder protection blocks moving this item to Trash. You can change this in Application Settings.",
            );
        }
        self.safety_policy(Some(view)).check_trash(source)?;
        if entry.kind == crate::application::contracts::directory::EntryKind::Directory
            && !entry.symlink
            && self.is_workspace_referenced_folder(source)
        {
            return Err(
                "This folder is open in a box or used by a navigation scope. Navigate away or update the scope before moving it to Trash.",
            );
        }
        Ok(())
    }
    pub fn file_operation(&self) -> Option<&Operation> {
        self.file_operations.current.as_ref()
    }
    pub fn files_closing(&self) -> bool {
        self.file_operations.close_requested
    }
    pub fn files_busy(&self) -> bool {
        self.file_operation().is_some_and(Operation::active)
    }

    /// Product/session admission only. The worker checks actual filesystem access.
    pub fn can_write_folder(&self, view: ViewId, directory: &std::path::Path) -> bool {
        self.accepts_workspace_input()
            && self.workspace.mode() == crate::domain::Mode::Browse
            && !self.files_busy()
            && self
                .file_operation()
                .is_none_or(|op| op.error.is_none() && !op.stopped)
            && !matches!(
                self.persistence,
                crate::application::contracts::storage::PersistenceState::Recovery { .. }
            )
            && self.workspace.can_browse(view)
            && self.has_ready_directory(view, directory)
            && self.write_folder_guidance(view, directory).is_ok()
    }

    pub(in crate::application) fn start_files(
        &mut self,
        view: ViewId,
        directory: PathBuf,
        items: Vec<(Item, String)>,
    ) -> Vec<Effect> {
        let read_policy = self.safety_policy(None);
        if !self.can_write_folder(view, &directory)
            || items.is_empty()
            || items.len() > ITEM_LIMIT
            || items.iter().any(|(item, name)| {
                validate_name(name).is_err()
                    || matches!(
                        item,
                        Item::Copy(path)
                            if !path.is_absolute() || read_policy.check_read_directory(path).is_err()
                    )
                    || matches!(item, Item::Move(path) if self.move_guidance(path).is_err())
                    || matches!(item, Item::Trash(path) if self.trash_guidance(view, path).is_err())
            })
        {
            return vec![];
        }
        if items.iter().any(|(item, _)| matches!(item, Item::Rename(_)))
            && (items.len() != 1 || items.iter().any(|(item, _)| matches!(item, Item::Rename(source) if self.rename_guidance(view, source).is_err()))) {
            return vec![];
        }
        let Some(id) = self.file_operations.next_id.checked_add(1) else {
            return vec![];
        };
        self.file_operations.next_id = id;
        let safety = self.safety_policy(Some(view));
        let mut operation = Operation {
            id,
            view,
            directory,
            total: items.len(),
            phase: Phase::Finished,
            completed: 0,
            skipped: 0,
            stopped: false,
            error: None,
            outcomes: vec![],
            pending: items.into(),
            parent: None,
            safety,
            step: 0,
        };
        let effect = operation.next().map(Effect::RunFileOperation);
        self.file_operations.current = Some(operation);
        effect.into_iter().collect()
    }

    pub(in crate::application) fn files_completed(&mut self, receipt: Completion) -> Vec<Effect> {
        let Some(operation) = &mut self.file_operations.current else {
            return vec![];
        };
        if !matches!(&operation.phase, Phase::Running(job) if job == &receipt.job) {
            return vec![];
        }
        operation.parent = receipt.parent.or(operation.parent);
        let directory = operation.directory.clone();
        let source_directory = match &receipt.job.item {
            Item::Move(source) => source.parent().map(std::path::Path::to_path_buf),
            _ => None,
        };
        let mut completed_ok = false;
        match receipt.outcome {
            Outcome::Conflict if !operation.stopped && !self.file_operations.close_requested => {
                let mut job = receipt.job;
                job.rename_identity = receipt.rename_identity.or(job.rename_identity);
                operation.phase = Phase::Conflict(job);
                return vec![];
            }
            Outcome::Conflict => {
                operation.stopped = true;
                operation.record(&receipt.job, ItemStatus::Cancelled);
            }
            Outcome::Complete => {
                completed_ok = true;
                operation.completed += 1;
                operation.record(&receipt.job, ItemStatus::Complete);
                if let Item::Trash(source) | Item::Move(source) | Item::Rename(source) =
                    &receipt.job.item
                {
                    self.workspace.prune_removed_path(source);
                }
            }
            Outcome::Failed(error) => {
                operation.record(&receipt.job, ItemStatus::Failed);
                let detail = match &receipt.job.item {
                    Item::Move(path) => format!(
                        "\nSource: {}\nDestination: {}",
                        path.display(),
                        operation.directory.join(&receipt.job.name).display()
                    ),
                    Item::Trash(path) => format!("\nItem: {}", path.display()),
                    _ => format!(
                        "\nDestination: {}",
                        operation.directory.join(&receipt.job.name).display()
                    ),
                };
                operation.error = Some(format!("{}: {error}{detail}", receipt.job.name))
            }
        }
        let next = operation.next().map(Effect::RunFileOperation);
        let failed = operation.error.is_some();
        let clean = !operation.active()
            && !operation.stopped
            && operation.error.is_none()
            && operation.skipped == 0;
        if clean {
            self.file_operations.current = None;
        }
        let mut effects: Vec<_> = next.into_iter().collect();
        if completed_ok {
            self.note_path_access_granted(&directory);
        }
        self.invalidate_file_directory(&directory);
        if let Some(source) = source_directory {
            self.invalidate_file_directory(&source);
        }
        if self.file_operations.close_requested && !self.files_busy() {
            self.file_operations.close_requested = false;
            // Failed output needs acknowledgement, not an automatic successful exit.
            if !failed {
                effects.extend(self.request_close());
            }
        }
        effects
    }

    pub(in crate::application) fn resolve_file_conflict(
        &mut self,
        id: u64,
        step: u64,
        choice: ConflictChoice,
    ) -> Vec<Effect> {
        let Some(operation) = &mut self.file_operations.current else {
            return vec![];
        };
        if operation.id != id {
            return vec![];
        }
        let Phase::Conflict(job) = &operation.phase else {
            return vec![];
        };
        if job.step != step {
            return vec![];
        }
        session().record(
            Subsystem::Files,
            Activity::ConflictChoice,
            match &choice {
                ConflictChoice::KeepBoth(_) => DiagnosticOutcome::KeepBoth,
                ConflictChoice::Skip => DiagnosticOutcome::Skip,
                ConflictChoice::Cancel => DiagnosticOutcome::Cancelled,
            },
            id,
            step,
            None,
        );
        match choice {
            ConflictChoice::KeepBoth(name) => {
                let name = match job.item {
                    Item::Create(kind) => match kind.filename(&name) {
                        Ok(name) => name,
                        Err(_) => return vec![],
                    },
                    Item::Copy(_) | Item::Move(_) | Item::Rename(_) => {
                        if validate_name(&name).is_err() {
                            return vec![];
                        }
                        name
                    }
                    Item::Trash(_) => return vec![],
                };
                let mut job = job.clone();
                let Some(step) = operation.step.checked_add(1) else {
                    return vec![];
                };
                operation.step = step;
                job.step = operation.step;
                job.name = name;
                job.parent = operation.parent;
                operation.phase = Phase::Running(job.clone());
                vec![Effect::RunFileOperation(job)]
            }
            ConflictChoice::Skip => {
                let job = job.clone();
                operation.record(&job, ItemStatus::Skipped);
                operation.skipped += 1;
                operation
                    .next()
                    .map(Effect::RunFileOperation)
                    .into_iter()
                    .collect()
            }
            ConflictChoice::Cancel => {
                let job = job.clone();
                operation.record(&job, ItemStatus::Cancelled);
                operation.stopped = true;
                operation.next();
                vec![]
            }
        }
    }

    pub(in crate::application) fn stop_files(&mut self, id: u64) {
        if let Some(operation) = &mut self.file_operations.current
            && operation.id == id
        {
            session().record(
                Subsystem::Files,
                Activity::Stop,
                DiagnosticOutcome::CancelRequested,
                id,
                operation.step,
                None,
            );
            operation.stopped = true;
            if let Phase::Conflict(job) = &operation.phase {
                let job = job.clone();
                operation.record(&job, ItemStatus::Cancelled);
                operation.next();
            }
        }
    }

    pub(in crate::application) fn files_block(&self, action: &Action) -> bool {
        let Some(operation) = self.file_operation() else {
            return false;
        };
        // Keep the destination view available for its local progress/conflict prompt.
        // Unrelated boxes may still browse and open files.
        match action {
            Action::SetSort(view, _) => *view == operation.view,
            Action::SetBoxSetting(id, crate::domain::BoxSetting::HiddenFiles(_)) => self
                .workspace
                .boxes()
                .iter()
                .any(|item| item.id == *id && item.view(operation.view).is_some()),
            Action::Navigate(view, command) => {
                (*view == operation.view
                    || matches!(
                        &operation.phase,
                        Phase::Running(Job {
                            item: Item::Rename(_) | Item::Move(_) | Item::Trash(_),
                            ..
                        }) | Phase::Conflict(Job {
                            item: Item::Rename(_) | Item::Move(_) | Item::Trash(_),
                            ..
                        })
                    ))
                    && !matches!(command, Navigate::Refresh)
            }
            Action::ChangeTabs(id, _) | Action::RemoveBox(id) => self
                .workspace
                .boxes()
                .iter()
                .any(|item| item.id == *id && item.view(operation.view).is_some()),
            Action::SelectMode(_)
            | Action::Escape
            | Action::Undo
            | Action::RemoveSelected
            | Action::OpenWorkspaceManagement
            | Action::SelectWorkspace(_)
            | Action::CreateWorkspace(_)
            | Action::RenameWorkspace { .. }
            | Action::RemoveWorkspace { .. }
            | Action::RestartStartup { .. }
            | Action::ConfigurePath(..)
            | Action::ConfigureFolder(..)
            | Action::EditScope(..) => true,
            _ => false,
        }
    }
}
