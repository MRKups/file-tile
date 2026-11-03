//! Runtime ownership of automatic startup and local recovery confirmation.

use std::sync::Arc;

use iced::Task;

use crate::application::contracts::configuration::WorkspaceId;
use crate::application::contracts::storage::{
    SessionId, StartupOperation, StorageCompletion, StorageFailure,
};
use crate::application::{Action, Application, Effect};
use crate::persistence::StorageWorker;

use crate::{
    Desktop, DirectoryBackend, Message, runtime::launch::Launch, window::window_placement,
};

pub(crate) fn boot(launch: &Launch, directory: DirectoryBackend) -> (Desktop, Task<Message>) {
    let (mut application, effects) =
        Application::with_storage(launch.directory.clone(), launch.home.clone());
    application.set_state_directory(launch.state_dir.clone());
    let mut desktop = Desktop::new(application, directory);
    desktop.storage_root = Some(launch.state_dir.clone());
    desktop.sync_diagnostics();
    let task = desktop.effects(effects);
    (desktop, task)
}

impl Desktop {
    pub(crate) fn recovery_input(&mut self, event: iced::Event) -> Task<Message> {
        if let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab),
            modifiers,
            repeat: false,
            ..
        }) = &event
        {
            return crate::dialogs::storage_dialog::move_focus(modifiers.shift());
        }
        if matches!(
            event,
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                ..
            })
        ) {
            if self.startup_confirmation.is_some() {
                self.startup_confirmation = None;
                self.focus_version = self.focus_version.saturating_add(1);
                return crate::dialogs::storage_dialog::focus_initial(&self.application, None);
            } else if !self.application.close_failed() {
                self.storage_details = false;
            }
        }
        if self.modal_open() {
            Task::none()
        } else {
            self.dispatch(Action::CancelPointer)
        }
    }
    pub(crate) fn storage_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::StorageDetails(show) => {
                self.storage_details = show;
                self.startup_confirmation = None;
                if show {
                    self.application.update(Action::CancelAppSettings);
                    self.app_settings = None;
                    if !self.application.workspace_management_pending() {
                        self.application.update(Action::CancelWorkspaceManagement);
                        self.workspace_management = None;
                    }
                    self.focus_version = self.focus_version.saturating_add(1);
                    let task = self.dispatch(Action::CancelPointer);
                    if self.application.storage_detail().is_some() {
                        Task::batch([
                            task,
                            crate::dialogs::storage_dialog::focus_initial(&self.application, None),
                        ])
                    } else {
                        task
                    }
                } else {
                    self.dispatch(Action::CancelPointer)
                }
            }
            Message::PrepareStartup(operation) => self.prepare_startup(operation),
            Message::ConfirmStartup => {
                self.startup_confirmation
                    .take()
                    .map_or_else(Task::none, |operation| {
                        self.dispatch(Action::RestartStartup {
                            operation,
                            confirmed: true,
                        })
                    })
            }
            Message::CancelStartupConfirmation => {
                self.startup_confirmation = None;
                self.focus_version = self.focus_version.saturating_add(1);
                crate::dialogs::storage_dialog::focus_initial(&self.application, None)
            }
            _ => Task::none(),
        }
    }
    pub(crate) fn start_storage(
        &mut self,
        session: SessionId,
        token: Option<WorkspaceId>,
    ) -> Task<Message> {
        if self
            .storage
            .as_ref()
            .is_some_and(|worker| worker.is_stopped())
        {
            self.storage = None;
        }
        if self.storage.is_none() {
            let created = self
                .storage_root
                .as_ref()
                .ok_or(StorageFailure::Unavailable)
                .and_then(|root| StorageWorker::new(root.clone()).map_err(StorageFailure::from));
            match created {
                Ok(worker) => self.storage = Some(Arc::new(worker)),
                Err(error) => {
                    return Task::done(Message::Action(Action::StorageCompleted(
                        StorageCompletion::Loaded {
                            session,
                            result: Err(error),
                        },
                    )));
                }
            }
        }
        self.storage.as_ref().map_or_else(Task::none, |storage| {
            let future = token.map_or_else(
                || storage.load(session),
                |token| storage.restore(session, token),
            );
            Task::perform(future, |completion| {
                Message::Action(Action::StorageCompleted(completion))
            })
        })
    }

    pub(crate) fn prepare_startup(&mut self, operation: StartupOperation) -> Task<Message> {
        if !self.application.can_retry_startup() {
            return Task::none();
        }
        let restore = matches!(operation, StartupOperation::RestoreBackup(_));
        if restore || self.application.startup_has_unsaved_edits() {
            self.startup_confirmation = Some(operation);
            self.storage_details = true;
            self.focused_path = None;
            self.focus_version = self.focus_version.saturating_add(1);
            crate::dialogs::storage_dialog::focus_initial(
                &self.application,
                self.startup_confirmation,
            )
        } else {
            self.dispatch(Action::RestartStartup {
                operation,
                confirmed: false,
            })
        }
    }

    pub(crate) fn effects(&mut self, effects: Vec<Effect>) -> Task<Message> {
        let file_focus = self.reconcile_file_actions();
        let reveal = self.reveal_closing_file_operation();
        self.sync_quit_signal();
        self.retain_pending_open();
        self.retain_metadata_reads();
        self.retain_sort_tasks();
        self.retain_usage_tasks();
        let effects = Task::batch(effects.into_iter().map(|effect| match effect {
            Effect::MeasureFolderUsage(request) => self.usage_task(request),
            Effect::SortDirectory(request) => self.sort_task(request),
            Effect::RunFileOperation(job) => self.run_file_operation(job),
            Effect::ManageWorkspace(request) => self.storage.as_ref().map_or_else(
                || {
                    Task::done(Message::Action(Action::StorageCompleted(
                        StorageCompletion::Managed {
                            operation: request.receipt_tag(),
                            result: Err(StorageFailure::Unavailable),
                        },
                    )))
                },
                |storage| {
                    Task::perform(storage.manage(request.clone()), |completion| {
                        Message::Action(Action::StorageCompleted(completion))
                    })
                },
            ),
            Effect::OpenExternal(request) => self.external_task(request),
            Effect::OpenLauncher(request) => self.open_launcher_task(request),
            Effect::ReadMetadata(request) => self.read_metadata(request),
            Effect::ListTree(request) => self.configuration_read(request, false),
            Effect::ResolveScope(request) => self.configuration_read(request, true),
            Effect::CheckWorkspaceBudget(request) => self.storage.as_ref().map_or_else(
                || {
                    Task::done(Message::Action(Action::StorageCompleted(
                        StorageCompletion::BudgetChecked {
                            generation: request.generation,
                            result: Err(StorageFailure::Unavailable),
                        },
                    )))
                },
                |storage| {
                    Task::perform(storage.check_workspace(request.clone()), |completion| {
                        Message::Action(Action::StorageCompleted(completion))
                    })
                },
            ),
            Effect::ObserveDirectories(observations) => self.observe_directories(observations),
            Effect::List(request) => {
                let presentation = self.views.entry(request.view).or_default();
                presentation.submitted_revision =
                    Some((request.generation, presentation.draft_revision));
                self.list_directory(request)
            }
            Effect::LoadConfiguration(session) => self.start_storage(session, None),
            Effect::RestoreBackup { session, token } => self.start_storage(session, Some(token)),
            Effect::SaveWorkspace(request) => {
                self.storage.as_ref().map_or_else(Task::none, |storage| {
                    Task::perform(storage.save(request), |completion| {
                        Message::Action(Action::StorageCompleted(completion))
                    })
                })
            }
            Effect::CaptureWindowForClose(tag) => {
                self.sample_window(window_placement::SampleKind::Close(tag))
            }
            Effect::Close => {
                iced::window::latest().then(|id| id.map_or_else(Task::none, iced::window::close))
            }
        }));
        Task::batch([file_focus, reveal, effects])
    }
}
