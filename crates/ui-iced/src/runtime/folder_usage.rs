//! Root-path waiter ownership matches the application session cache, not view IDs.
use crate::application::{
    Action,
    contracts::directory::Cancellation,
    files::folder_usage::{UsageCompletion, UsageError, UsageRequest},
};
use crate::{Desktop, Message};
use iced::Task;
use std::{collections::HashMap, path::PathBuf, sync::Arc};

#[derive(Default)]
pub(crate) struct Runtime {
    pub workers: Option<Arc<crate::filesystem::folder_usage::UsageWorkers>>,
    pending: HashMap<PathBuf, Pending>,
}
struct Pending {
    generation: u64,
    cancel: Cancellation,
    handle: iced::task::Handle,
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.handle.abort();
    }
}
impl Desktop {
    pub(crate) fn retain_usage_tasks(&mut self) {
        self.usage_runtime.pending.retain(|path, pending| {
            self.application.usage_requests().any(|request| {
                request.directory == *path && request.generation == pending.generation
            })
        });
    }
    pub(crate) fn usage_task(&mut self, request: UsageRequest) -> Task<Message> {
        let Some(workers) = &self.usage_runtime.workers else {
            return Task::done(Message::Action(Action::FolderUsageCompleted(
                UsageCompletion {
                    request,
                    result: Err(UsageError::Unavailable),
                },
            )));
        };
        let path = request.directory.clone();
        let generation = request.generation;
        let is_home = path == self.application.home();
        let excluded = if is_home {
            self.application.safety().excluded_home_usage_folders()
        } else {
            Vec::new()
        };
        let (cancel, future) = workers.submit_with_exclusions(request, excluded, is_home);
        let (task, handle) = Task::perform(future, |result| {
            Message::Action(Action::FolderUsageCompleted(result))
        })
        .abortable();
        self.usage_runtime.pending.insert(
            path,
            Pending {
                generation,
                cancel,
                handle,
            },
        );
        task
    }
}
