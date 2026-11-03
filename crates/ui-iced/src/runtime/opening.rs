//! Runtime ownership for default-app handoffs. No image state or modal UI.
use crate::application::{
    Action,
    contracts::directory::Cancellation,
    files::opening::{OpenCompletion, OpenRequest, Opening},
};
use crate::{Desktop, Message};
use iced::Task;

pub(crate) struct Pending {
    request: OpenRequest,
    cancellation: Cancellation,
    handle: iced::task::Handle,
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.handle.abort();
    }
}

impl Desktop {
    pub(crate) fn external_task(&mut self, request: OpenRequest) -> Task<Message> {
        let Some(workers) = &self.open_workers else {
            return Task::done(Message::Action(Action::ExternalOpened(OpenCompletion {
                request,
                result: Err("File opening worker is unavailable. Retry explicit Open.".into()),
            })));
        };
        let future = workers.open(request.clone());
        let cancellation = future.cancellation();
        let (task, handle) = Task::perform(future, |result| {
            Message::Action(Action::ExternalOpened(result))
        })
        .abortable();
        self.pending_open = Some(Pending {
            request,
            cancellation,
            handle,
        });
        task
    }

    pub(crate) fn retain_pending_open(&mut self) {
        if self.pending_open.as_ref().is_some_and(|pending| {
            !self.application.external_opening().is_some_and(|opening| {
                opening.request == pending.request
                    && opening.phase == Opening::Pending(pending.request.generation)
            })
        }) {
            self.pending_open = None;
        }
    }
}
