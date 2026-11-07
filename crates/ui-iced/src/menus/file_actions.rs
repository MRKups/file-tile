//! Menu/keyboard file commands and task ownership, separate from box-local controls.
use crate::application::{
    Action, LoadState,
    files::file_operations::{self as ops, ConflictChoice, Item, Phase},
};
use crate::domain::{
    ViewId,
    files::{CreateKind, keep_both_name, validate_name},
};
use crate::{Desktop, Message};
use iced::{Task, widget::operation};
use std::path::PathBuf;

#[derive(Debug)]
pub(crate) enum NamePurpose {
    Create(CreateKind),
    Rename(PathBuf),
}

impl NamePurpose {
    pub fn title(&self) -> String {
        match self {
            Self::Create(kind) => format!("New {}", kind.label()),
            Self::Rename(_) => "Rename".into(),
        }
    }
    pub fn action(&self) -> &'static str {
        if matches!(self, Self::Rename(_)) {
            "Rename"
        } else {
            "Create"
        }
    }
    fn filename(&self, value: &str) -> Result<String, String> {
        match self {
            Self::Create(kind) => kind.filename(value),
            Self::Rename(_) => {
                validate_name(value)?;
                Ok(value.into())
            }
        }
    }
    fn item(&self) -> Item {
        match self {
            Self::Create(kind) => Item::Create(*kind),
            Self::Rename(source) => Item::Rename(source.clone()),
        }
    }
}

#[derive(Debug)]
pub(crate) struct NameDraft {
    pub generation: u64,
    pub directory: PathBuf,
    pub purpose: NamePurpose,
    pub value: String,
    pub error: Option<String>,
}

#[derive(Debug)]
pub(crate) struct TrashDraft {
    pub generation: u64,
    pub directory: PathBuf,
    pub items: Vec<(PathBuf, String)>,
    pub error: Option<String>,
}

pub(crate) fn name_id(view: ViewId) -> String {
    format!("create-name-{}", view.0)
}

impl Desktop {
    pub(crate) fn clear_trash_drafts(&mut self) {
        for state in self.views.values_mut() {
            state.name_draft = None;
            state.trash_draft = None;
        }
    }

    pub(crate) fn anchor_file_prompt(&mut self, view: ViewId, point: iced::Point) {
        let geometry = self.geometry();
        let anchor = self
            .application
            .workspace()
            .boxes()
            .iter()
            .find(|item| item.view(view).is_some())
            .map(|item| {
                let rect = geometry.rect(item.rect);
                iced::Point::new(
                    point.x - rect.x - geometry.viewport.x + geometry.offset.x,
                    point.y - rect.y - geometry.viewport.y + geometry.offset.y,
                )
            });
        self.views.entry(view).or_default().prompt_anchor = anchor;
    }
    pub(crate) fn file_escape(&mut self) -> Option<Task<Message>> {
        if let Some(operation) = self.application.file_operation()
            && self.active_view == Some(operation.view)
            && let Phase::Conflict(job) = &operation.phase
        {
            return Some(self.dispatch(Action::ResolveFileConflict(
                operation.id,
                job.step,
                ConflictChoice::Cancel,
            )));
        }
        let view = self.active_view?;
        let state = self.views.get(&view)?;
        if let Some(draft) = &state.trash_draft {
            let generation = draft.generation;
            return Some(self.file_action_message(Message::TrashPrompt(view, generation, false)));
        }
        let generation = state.name_draft.as_ref()?.generation;
        Some(self.file_action_message(Message::CancelName(view, generation)))
    }
    pub(crate) fn file_editor_active(&self) -> bool {
        self.active_view.is_some_and(|view| {
            self.views
                .get(&view)
                .is_some_and(|state| state.name_draft.is_some() || state.trash_draft.is_some())
                || self.application.file_operation().is_some_and(|operation| {
                    operation.view == view && matches!(operation.phase, Phase::Conflict(_))
                })
        })
    }
    pub(crate) fn file_write_guidance(&self, view: ViewId) -> &'static str {
        let directory = self
            .application
            .workspace()
            .view(view)
            .map(|v| v.navigation.current());
        self.file_write_guidance_opt(view, directory)
    }

    pub(crate) fn file_write_guidance_for(
        &self,
        view: ViewId,
        directory: &std::path::Path,
    ) -> &'static str {
        self.file_write_guidance_opt(view, Some(directory))
    }

    fn file_write_guidance_opt(
        &self,
        view: ViewId,
        directory: Option<&std::path::Path>,
    ) -> &'static str {
        if self.application.workspace().mode() != crate::domain::Mode::Browse {
            return "Switch to Browse to create or paste.";
        }
        if matches!(
            self.application.persistence(),
            crate::application::contracts::storage::PersistenceState::Recovery { .. }
        ) {
            return "File operations are unavailable in recovery. Resolve workspace recovery first.";
        }
        if self.application.file_operation().is_some() {
            return "Finish or dismiss the operation in its destination box.";
        }
        if let Some(directory) = directory
            && let Err(reason) = self.application.write_folder_guidance(view, directory)
        {
            return reason;
        }
        match self.application.load(view) {
            Some(LoadState::Failed { .. }) => {
                "Folder is unavailable. Reload the folder before creating or pasting."
            }
            _ => "Wait for the folder to finish loading, then retry.",
        }
    }
    pub(crate) fn file_view_input_enabled(&self, view: ViewId) -> bool {
        self.workspace_input_enabled()
            && self.application.workspace().can_browse(view)
            && !self
                .views
                .get(&view)
                .is_some_and(|state| state.name_draft.is_some() || state.trash_draft.is_some())
            && !self
                .application
                .file_operation()
                .is_some_and(|operation| operation.view == view)
    }
    pub(crate) fn start_name_in(
        &mut self,
        view: ViewId,
        directory: PathBuf,
        kind: CreateKind,
    ) -> Task<Message> {
        self.begin_name(
            view,
            directory,
            NamePurpose::Create(kind),
            kind.default_name().to_owned(),
            kind != CreateKind::Folder,
        )
    }

    pub(crate) fn start_rename_in(
        &mut self,
        view: ViewId,
        directory: PathBuf,
        source: PathBuf,
    ) -> Task<Message> {
        if let Err(error) = self.application.rename_guidance(view, &source) {
            self.views.entry(view).or_default().hint = Some(error.into());
            return Task::none();
        }
        let Some(entry) = self
            .listed_entry(view, &source)
            .or_else(|| self.column_previews.listed_entry(view, &source))
        else {
            return Task::none();
        };
        let stem = entry.kind != crate::application::contracts::directory::EntryKind::Directory;
        match crate::domain::files::source_name(&source) {
            Ok(value) => self.begin_name(view, directory, NamePurpose::Rename(source), value, stem),
            Err(error) => {
                self.views.entry(view).or_default().hint = Some(error);
                Task::none()
            }
        }
    }

    fn begin_name(
        &mut self,
        view: ViewId,
        directory: PathBuf,
        purpose: NamePurpose,
        value: String,
        stem: bool,
    ) -> Task<Message> {
        if !self.workspace_input_enabled() || !self.application.can_write_folder(view, &directory) {
            self.views.entry(view).or_default().hint =
                Some(self.file_write_guidance_for(view, &directory).into());
            return Task::none();
        }
        for state in self.views.values_mut() {
            state.name_draft = None;
            state.trash_draft = None;
        }
        self.focus_version = self.focus_version.saturating_add(1);
        let generation = self.focus_version;
        self.active_view = Some(view);
        self.focused_path = None;
        let end = if !stem {
            value.chars().count()
        } else {
            value
                .rsplit_once('.')
                .filter(|(stem, _)| !stem.is_empty())
                .map_or(value.chars().count(), |(stem, _)| stem.chars().count())
        };
        self.views.entry(view).or_default().name_draft = Some(NameDraft {
            generation,
            directory,
            purpose,
            value,
            error: None,
        });
        operation::focus(name_id(view)).chain(operation::select_range(name_id(view), 0, end))
    }

    pub(crate) fn file_action_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::NameChanged(view, generation, value) => {
                if !self.workspace_input_enabled() {
                    return Task::none();
                }
                if let Some(draft) = self
                    .views
                    .get_mut(&view)
                    .and_then(|s| s.name_draft.as_mut())
                    && draft.generation == generation
                {
                    draft.value = value;
                    draft.error = None;
                }
            }
            Message::CancelName(view, generation) => {
                if !self.workspace_input_enabled() {
                    return Task::none();
                }
                if let Some(state) = self.views.get_mut(&view)
                    && state
                        .name_draft
                        .as_ref()
                        .is_some_and(|draft| draft.generation == generation)
                {
                    state.name_draft = None;
                    return operation::focus("workspace-canvas");
                }
            }
            Message::SubmitName(view, generation) => {
                return self.submit_file_name(view, generation);
            }
            Message::TrashPrompt(view, generation, false) => {
                if !self.workspace_input_enabled() {
                    return Task::none();
                }
                if let Some(state) = self.views.get_mut(&view)
                    && state
                        .trash_draft
                        .as_ref()
                        .is_some_and(|draft| draft.generation == generation)
                {
                    state.trash_draft = None;
                    return operation::focus("workspace-canvas");
                }
            }
            Message::TrashPrompt(view, generation, true) => {
                return self.submit_trash(view, generation);
            }
            Message::ConflictNameChanged(id, step, value) => {
                if let Some((current, current_step, name)) = &mut self.conflict_name
                    && *current == id
                    && *current_step == step
                {
                    *name = value;
                }
            }
            Message::KeepBoth(id, step) => {
                if let Some((current, current_step, name)) = &self.conflict_name
                    && *current == id
                    && *current_step == step
                    && validate_name(name).is_ok()
                {
                    return self.dispatch(Action::ResolveFileConflict(
                        id,
                        step,
                        ConflictChoice::KeepBoth(name.clone()),
                    ));
                }
            }
            _ => {}
        }
        Task::none()
    }

    fn submit_file_name(&mut self, view: ViewId, generation: u64) -> Task<Message> {
        if !self.workspace_input_enabled() {
            return Task::none();
        }
        let Some(draft) = self
            .views
            .get_mut(&view)
            .and_then(|s| s.name_draft.as_mut())
            .filter(|draft| draft.generation == generation)
        else {
            return Task::none();
        };
        let name = match draft.purpose.filename(&draft.value) {
            Ok(name) => name,
            Err(error) => {
                draft.error = Some(error);
                return Task::none();
            }
        };
        let (directory, item) = (draft.directory.clone(), draft.purpose.item());
        let unavailable = self.file_write_guidance_for(view, &directory);
        let error = if !self.application.can_write_folder(view, &directory) {
            Some(unavailable)
        } else if let Item::Rename(source) = &item {
            self.application.rename_guidance(view, source).err()
        } else {
            None
        };
        if let Some(error) = error {
            if let Some(draft) = self
                .views
                .get_mut(&view)
                .and_then(|s| s.name_draft.as_mut())
            {
                draft.error = Some(error.into());
            }
            return Task::none();
        }
        self.views.entry(view).or_default().name_draft = None;
        if matches!(&item, Item::Rename(source) if source.file_name() == Some(std::ffi::OsStr::new(&name)))
        {
            return self.focus_canvas();
        }
        self.dispatch(Action::StartFileOperation {
            view,
            directory,
            items: vec![(item, name)],
        })
    }

    pub(crate) fn start_trash(&mut self, view: ViewId) -> Task<Message> {
        if let Some((preview_dir, selected)) = self.column_previews.selected_entry(view) {
            return self.start_trash_paths(view, preview_dir, vec![selected]);
        }
        let Some(directory) = self
            .application
            .workspace()
            .view(view)
            .map(|v| v.navigation.current().to_path_buf())
        else {
            return Task::none();
        };
        let selected: Vec<PathBuf> = {
            let state = self.views.get(&view);
            let ordered: Vec<_> = self
                .application
                .ordered_entries(view)
                .filter(|entry| state.is_some_and(|s| s.selection.contains(&entry.path)))
                .map(|entry| entry.path.clone())
                .collect();
            if ordered.is_empty() {
                state.and_then(|s| s.selected.clone()).into_iter().collect()
            } else {
                ordered
            }
        };
        self.start_trash_paths(view, directory, selected)
    }

    pub(crate) fn start_trash_paths(
        &mut self,
        view: ViewId,
        directory: PathBuf,
        selected: Vec<PathBuf>,
    ) -> Task<Message> {
        if !self.workspace_input_enabled() || !self.application.can_write_folder(view, &directory) {
            self.views.entry(view).or_default().hint =
                Some(self.file_write_guidance_for(view, &directory).into());
            return Task::none();
        }
        if selected.is_empty() {
            return Task::none();
        }
        if selected.len() > ops::ITEM_LIMIT {
            self.views.entry(view).or_default().hint = Some(format!(
                "Select at most {} items to move to Trash at once.",
                ops::ITEM_LIMIT
            ));
            return Task::none();
        }
        let mut items = Vec::with_capacity(selected.len());
        for path in selected {
            if let Err(error) = self.application.trash_guidance(view, &path) {
                self.views.entry(view).or_default().hint = Some(error.into());
                return Task::none();
            }
            match crate::domain::files::source_name(&path) {
                Ok(name) => items.push((path, name)),
                Err(error) => {
                    self.views.entry(view).or_default().hint = Some(error);
                    return Task::none();
                }
            }
        }
        for state in self.views.values_mut() {
            state.name_draft = None;
            state.trash_draft = None;
        }
        self.focus_version = self.focus_version.saturating_add(1);
        let generation = self.focus_version;
        self.active_view = Some(view);
        self.focused_path = None;
        let state = self.views.entry(view).or_default();
        state.hint = None;
        state.trash_draft = Some(TrashDraft {
            generation,
            directory,
            items,
            error: None,
        });
        operation::focus("file-trash-cancel")
    }

    fn submit_trash(&mut self, view: ViewId, generation: u64) -> Task<Message> {
        if !self.workspace_input_enabled() {
            return Task::none();
        }
        let Some(draft) = self
            .views
            .get(&view)
            .and_then(|s| s.trash_draft.as_ref())
            .filter(|draft| draft.generation == generation)
        else {
            return Task::none();
        };
        let directory = draft.directory.clone();
        let draft_items = draft.items.clone();
        let unavailable = self.file_write_guidance_for(view, &directory);
        let error = if !self.application.can_write_folder(view, &directory) {
            Some(unavailable)
        } else {
            draft_items
                .iter()
                .find_map(|(path, _)| self.application.trash_guidance(view, path).err())
        };
        if let Some(error) = error {
            if let Some(draft) = self
                .views
                .get_mut(&view)
                .and_then(|s| s.trash_draft.as_mut())
            {
                draft.error = Some(error.into());
            }
            return Task::none();
        }
        for (path, _) in &draft_items {
            self.column_previews.remove_selected(view, path);
        }
        if let Some(state) = self.views.get_mut(&view) {
            state.trash_draft = None;
            for (path, _) in &draft_items {
                state.selection.remove(path);
                if state.selected.as_ref() == Some(path) {
                    state.selected = None;
                }
                if state.selection_anchor.as_ref() == Some(path) {
                    state.selection_anchor = None;
                }
            }
        }
        self.dispatch(Action::StartFileOperation {
            view,
            directory,
            items: draft_items
                .into_iter()
                .map(|(path, name)| (Item::Trash(path), name))
                .collect(),
        })
    }

    pub(crate) fn file_key(
        &mut self,
        key: &iced::keyboard::Key,
        modifiers: iced::keyboard::Modifiers,
    ) -> Option<Task<Message>> {
        let view = self.active_view?;
        if !self.file_view_input_enabled(view)
            || modifiers.shift()
            || !(modifiers.command() || modifiers.control())
            || self.focused_path.is_some()
            || self
                .box_settings
                .as_ref()
                .is_some_and(|draft| draft.focused || draft.range.focused.is_some())
            || self
                .views
                .get(&view)
                .is_some_and(|state| state.name_draft.is_some() || state.trash_draft.is_some())
        {
            return None;
        }
        if modifiers.alt() {
            return match key.as_ref() {
                iced::keyboard::Key::Character("v" | "V") => Some(self.move_into(view)),
                _ => None,
            };
        }
        match key.as_ref() {
            iced::keyboard::Key::Named(iced::keyboard::key::Named::Backspace) => {
                Some(self.start_trash(view))
            }
            iced::keyboard::Key::Character("a" | "A") => {
                let paths: Vec<_> = self
                    .application
                    .ordered_entries(view)
                    .map(|entry| entry.path.clone())
                    .collect();
                let state = self.views.entry(view).or_default();
                state.selected = paths.first().cloned();
                state.selection_anchor = paths.first().cloned();
                state.selection = paths.into_iter().collect();
                Some(self.focus_canvas())
            }
            iced::keyboard::Key::Character("c" | "C") => Some(self.copy_selected_files(view)),
            iced::keyboard::Key::Character("x" | "X") => Some(self.cut_selected_files(view)),
            iced::keyboard::Key::Character("v" | "V") => Some(self.paste_into(view)),
            _ => None,
        }
    }

    pub(crate) fn run_file_operation(&mut self, job: ops::Job) -> Task<Message> {
        let Some(worker) = &self.file_worker else {
            return Task::done(Message::Action(Action::FileOperationCompleted(ops::Completion {
                job, parent: None, rename_identity: None, outcome: ops::Outcome::Failed("File-operation worker is unavailable. Inspect the destination and relaunch.".into()),
            })));
        };
        let id = job.operation;
        let (task, handle) = Task::perform(worker.submit(job), |receipt| {
            Message::Action(Action::FileOperationCompleted(receipt))
        })
        .abortable();
        // Never abort this task on navigation, Stop, or a close request.
        self.file_waiter = Some((id, handle));
        task
    }

    pub(crate) fn observe_file_action_presentation(&mut self, action: &Action) {
        match action {
            Action::StartFileOperation {
                view,
                directory,
                items,
            } => {
                if items.iter().any(|(item, _)| {
                    matches!(item, Item::Copy(_) | Item::Move(_) | Item::Create(_))
                }) {
                    let is_primary = self
                        .application
                        .workspace()
                        .view(*view)
                        .is_some_and(|v| v.navigation.current() == directory);
                    if is_primary {
                        if let Some(presentation) = self.views.get_mut(view) {
                            presentation.selection.clear();
                            presentation.pending_selection.clear();
                            presentation.selected = None;
                            presentation.reveal_selected = false;
                        }
                    } else {
                        self.column_previews.clear_selection(*view);
                    }
                }
            }
            Action::FileOperationCompleted(receipt) => {
                self.record_completed_file_presentation(receipt);
            }
            _ => {}
        }
    }

    fn record_completed_file_presentation(&mut self, receipt: &ops::Completion) {
        let Some(operation) = self.application.file_operation() else {
            return;
        };
        if operation.id != receipt.job.operation
            || !matches!(
                &operation.phase,
                Phase::Running(active) if active.step == receipt.job.step
            )
            || !matches!(receipt.outcome, ops::Outcome::Complete)
        {
            return;
        }
        let view = operation.view;
        let is_primary = self
            .application
            .workspace()
            .view(view)
            .is_some_and(|v| v.navigation.current() == receipt.job.directory);
        self.column_previews
            .invalidate_directory(&receipt.job.directory);
        if let Item::Move(src) | Item::Rename(src) | Item::Trash(src) = &receipt.job.item
            && let Some(parent) = src.parent()
        {
            self.column_previews.invalidate_directory(parent);
        }
        if let Item::Move(source) | Item::Rename(source) = &receipt.job.item {
            for state in self.views.values_mut() {
                state.selection.remove(source);
                state.pending_selection.remove(source);
                if state.selected.as_ref() == Some(source) {
                    state.selected = None;
                }
            }
        }
        let dest = receipt.job.directory.join(&receipt.job.name);
        if !is_primary {
            match &receipt.job.item {
                Item::Copy(_) | Item::Move(_) => {
                    self.column_previews.set_pending_selection(
                        view,
                        receipt.job.directory.clone(),
                        dest,
                    );
                    let verb = if matches!(receipt.job.item, Item::Move(_)) {
                        "Moved"
                    } else {
                        "Copied"
                    };
                    self.views.entry(view).or_default().hint =
                        Some(format!("{verb} {}", receipt.job.name));
                }
                Item::Create(_) | Item::Rename(_) => {
                    self.column_previews.set_pending_selection(
                        view,
                        receipt.job.directory.clone(),
                        dest,
                    );
                }
                Item::Trash(_) => {}
            }
            return;
        }
        match &receipt.job.item {
            Item::Copy(_) | Item::Move(_) => {
                let state = self.views.entry(view).or_default();
                state.pending_selection.insert(dest.clone());
                state.selection.insert(dest.clone());
                state.selected = Some(dest.clone());
                state.selection_anchor = Some(dest);
                state.reveal_selected = true;
                let verb = if matches!(receipt.job.item, Item::Move(_)) {
                    "Moved"
                } else {
                    "Copied"
                };
                let count = state.selection.len();
                state.hint = Some(if count <= 1 {
                    format!("{verb} {}", receipt.job.name)
                } else {
                    format!("{verb} {count} items")
                });
            }
            Item::Create(_) | Item::Rename(_) => {
                let state = self.views.entry(view).or_default();
                state.selection.clear();
                state.pending_selection.clear();
                state.pending_selection.insert(dest.clone());
                state.selection.insert(dest.clone());
                state.selected = Some(dest.clone());
                state.selection_anchor = Some(dest);
                state.reveal_selected = true;
            }
            Item::Trash(_) => {}
        }
    }

    pub(crate) fn reconcile_file_actions(&mut self) -> Task<Message> {
        for (view, state) in &mut self.views {
            if state.name_draft.as_ref().is_some_and(|draft| {
                !self.application.workspace().is_active(*view)
                    || !self.application.accepts_edits()
                    || !self
                        .application
                        .has_active_directory(*view, &draft.directory)
                    || self.application.workspace().mode() != crate::domain::Mode::Browse
            }) {
                state.name_draft = None;
            }
            if state.trash_draft.as_ref().is_some_and(|draft| {
                !self.application.workspace().is_active(*view)
                    || !self.application.accepts_edits()
                    || !self
                        .application
                        .has_active_directory(*view, &draft.directory)
                    || self.application.workspace().mode() != crate::domain::Mode::Browse
            }) {
                state.trash_draft = None;
            }
        }
        if let Some(operation) = self.application.file_operation() {
            if !matches!(operation.phase, Phase::Running(_)) {
                self.file_waiter = None;
            }
            if let Phase::Conflict(job) = &operation.phase {
                if self
                    .conflict_name
                    .as_ref()
                    .is_none_or(|(id, step, _)| *id != operation.id || *step != job.step)
                {
                    self.conflict_name = Some((operation.id, job.step, keep_both_name(&job.name)));
                    if self.active_view == Some(operation.view)
                        && !self.modal_open()
                        && !self.context_menu_open()
                    {
                        return operation::focus("file-conflict-name")
                            .chain(operation::select_all("file-conflict-name"));
                    }
                }
                return Task::none();
            }
        } else {
            self.file_waiter = None;
        }
        self.conflict_name = None;
        Task::none()
    }

    pub(crate) fn reveal_closing_file_operation(&mut self) -> Task<Message> {
        if !self.application.files_closing() {
            return Task::none();
        }
        let Some(view) = self
            .application
            .file_operation()
            .map(|operation| operation.view)
        else {
            return Task::none();
        };
        let geometry = self.geometry();
        let Some(rect) = self
            .application
            .workspace()
            .boxes()
            .iter()
            .find(|item| item.view(view).is_some())
            .map(|item| geometry.rect(item.rect))
        else {
            return Task::none();
        };
        let mut offset = self.canvas_offset;
        if rect.x < offset.x || rect.x + rect.width.min(300.0) > offset.x + geometry.viewport.width
        {
            offset.x = rect.x.max(0.0);
        }
        if rect.y < offset.y
            || rect.y + rect.height.min(180.0) > offset.y + geometry.viewport.height
        {
            offset.y = rect.y.max(0.0);
        }
        if offset == self.canvas_offset {
            return Task::none();
        }
        self.canvas_offset = offset;
        operation::scroll_to(
            "workspace-canvas",
            operation::AbsoluteOffset {
                x: offset.x,
                y: offset.y,
            },
        )
    }
}
