//! Runtime waiter ownership for whole-folder ordering; no sorting during rendering.
use crate::application::contracts::directory::Cancellation;
use crate::application::{
    Action,
    files::sorting::{SortCompletion, SortRequest},
};
use crate::{Desktop, Message};
use iced::Task;
use std::{collections::HashMap, sync::Arc};

#[derive(Default)]
pub(crate) struct Runtime {
    pub workers: Option<Arc<crate::filesystem::sorting::SortWorkers>>,
    pending: HashMap<crate::domain::ViewId, Pending>,
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
    pub(crate) fn retain_sort_tasks(&mut self) {
        self.sort_runtime.pending.retain(|view, pending| {
            self.application
                .sort_requests()
                .any(|request| request.view == *view && request.generation == pending.generation)
        });
    }
    pub(crate) fn sort_task(&mut self, request: SortRequest) -> Task<Message> {
        #[cfg(test)]
        if let crate::DirectoryBackend::Controlled(reader) = &self.directory {
            let result =
                crate::filesystem::sorting::prepare(&request, reader, &Cancellation::default());
            return Task::done(Message::Action(Action::SortCompleted(SortCompletion {
                request,
                result,
            })));
        }
        let Some(workers) = &self.sort_runtime.workers else {
            return Task::done(Message::Action(Action::SortCompleted(SortCompletion {
                request,
                result: Err("sorting workers unavailable".into()),
            })));
        };
        let view = request.view;
        let generation = request.generation;
        let (cancel, future) = workers.submit(request);
        let (task, handle) = Task::perform(future, |receipt| {
            Message::Action(Action::SortCompleted(receipt))
        })
        .abortable();
        self.sort_runtime.pending.insert(
            view,
            Pending {
                generation,
                cancel,
                handle,
            },
        );
        task
    }
}
