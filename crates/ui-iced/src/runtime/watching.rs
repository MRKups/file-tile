//! Runtime ownership of the observation adapter and its single async waiter.

use iced::Task;
#[cfg(not(test))]
use std::sync::Arc;

use crate::application::{
    Action,
    contracts::watching::{DirectoryObservation, WatchBatch, WatchState, WatchUpdate},
};
#[cfg(not(test))]
use crate::filesystem::DirectoryWatcher;

use crate::{Desktop, Message};

impl Desktop {
    pub(crate) fn observe_directories(
        &mut self,
        observations: Vec<DirectoryObservation>,
    ) -> Task<Message> {
        #[cfg(not(test))]
        let watch_locations = self.application.accepts_edits();
        #[cfg(test)]
        let watch_locations = false;
        if observations.is_empty() && !watch_locations && self.folder_watcher.is_none() {
            return Task::none();
        }
        if self.folder_watcher.is_none() {
            // Controlled UI fixtures never observe synthetic /fixture or /Users/demo host paths.
            // Native observer tests explicitly supply a service with private targets.
            #[cfg(not(test))]
            match DirectoryWatcher::new() {
                Ok(watcher) => self.folder_watcher = Some(Arc::new(watcher)),
                Err(error) => return failed(observations, error.to_string()),
            }
        }
        let Some(watcher) = &self.folder_watcher else {
            return Task::none();
        };
        let location_roots = if watch_locations {
            crate::platform::volumes::location_watch_roots(self.application.home())
                .into_iter()
                .filter(|path| {
                    crate::application::contracts::configuration::validate_saved_path(path).is_ok()
                })
                .collect()
        } else {
            Vec::new()
        };
        if let Err(error) = watcher.replace_with_locations(observations.clone(), location_roots) {
            return failed(observations, error.to_string());
        }
        self.listen_for_folder_changes()
    }

    fn listen_for_folder_changes(&mut self) -> Task<Message> {
        if self.watch_listener.is_some() {
            return Task::none();
        }
        let Some(watcher) = &self.folder_watcher else {
            return Task::none();
        };
        let (task, handle) = Task::perform(watcher.next(), Message::FolderChanges).abortable();
        self.watch_listener = Some(handle);
        task
    }

    pub(crate) fn folder_changes(&mut self, batch: Option<WatchBatch>) -> Task<Message> {
        self.watch_listener = None;
        if let Some(batch) = batch {
            let applied = self.dispatch(Action::WatchCompleted(batch));
            Task::batch([applied, self.listen_for_folder_changes()])
        } else {
            self.folder_watcher = None;
            let batch = unavailable(
                self.application.observations(),
                "Folder observation owner stopped. Reload all boxes to retry.".into(),
            );
            self.dispatch(Action::WatchCompleted(batch))
        }
    }
}

fn unavailable(observations: Vec<DirectoryObservation>, reason: String) -> WatchBatch {
    WatchBatch {
        updates: observations
            .into_iter()
            .map(|item| WatchUpdate {
                tag: item.tag,
                state: Some(WatchState::Unavailable(reason.clone())),
                changed: false,
            })
            .collect(),
        locations_changed: false,
    }
}

fn failed(observations: Vec<DirectoryObservation>, reason: String) -> Task<Message> {
    Task::done(Message::Action(Action::WatchCompleted(unavailable(
        observations,
        reason,
    ))))
}
