//! Runtime cancellation for tree expansion and scope resolution, not view listing state.
#[cfg(test)]
use crate::application::contracts::directory::{
    Cancellation, DirectoryListing, DirectoryReader, ListingCompletion, ListingOrigin,
};
use crate::application::{Action, contracts::directory::ListingRequest};
use crate::{Desktop, DirectoryBackend, Message, PendingRead};
use iced::Task;
use std::collections::HashMap;

impl Desktop {
    pub(crate) fn suspend_configuration_if_modal(&mut self) -> Vec<crate::application::Effect> {
        if self.modal_open() {
            self.application.suspend_box_configuration()
        } else {
            vec![]
        }
    }
    pub(crate) fn configuration_read(
        &mut self,
        request: ListingRequest,
        resolve: bool,
    ) -> Task<Message> {
        let safety = self.application.safety_policy(Some(request.view));
        match &self.directory {
            #[cfg(test)]
            DirectoryBackend::Controlled(reader) => {
                let result = if resolve {
                    reader
                        .resolve_directory(&request.directory, &Cancellation::default())
                        .map(|directory| {
                            DirectoryListing::new(
                                directory,
                                ListingOrigin::Controlled,
                                vec![],
                                vec![],
                            )
                        })
                } else {
                    reader.list_permitted(
                        &request.directory,
                        &crate::domain::Scope::default(),
                        &safety,
                        &Cancellation::default(),
                    )
                };
                let completion = ListingCompletion { request, result };
                Task::done(Message::Action(if resolve {
                    Action::ScopeResolved(completion)
                } else {
                    Action::TreeCompleted(completion)
                }))
            }
            DirectoryBackend::Native(workers) => {
                let future = if resolve {
                    workers.resolve(request.clone())
                } else {
                    workers.submit_permitted(
                        request.clone(),
                        crate::domain::Scope::default(),
                        safety,
                    )
                };
                let cancellation = future.cancellation();
                let (task, handle) = Task::perform(future, move |completion| {
                    Message::Action(if resolve {
                        Action::ScopeResolved(completion)
                    } else {
                        Action::TreeCompleted(completion)
                    })
                })
                .abortable();
                self.configuration_reads.insert(
                    request.generation,
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
    pub(crate) fn retain_configuration_reads(&mut self) {
        let active: HashMap<_, _> = self
            .application
            .folder_tree()
            .into_iter()
            .flat_map(|tree| tree.requests())
            .chain(self.application.scope_request())
            .map(|request| (request.generation, request))
            .collect();
        self.configuration_reads.retain(|generation, pending| {
            active
                .get(generation)
                .is_some_and(|request| **request == pending.request)
        });
    }
}
