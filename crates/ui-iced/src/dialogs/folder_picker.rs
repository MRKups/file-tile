//! One native sheet waiter shared by Settings, box configuration, and shortcuts.
use crate::Message;
use iced::Task;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

type Choice = Result<Option<PathBuf>, String>;

struct PendingPicker {
    cancelled: Arc<AtomicBool>,
    handle: iced::task::Handle,
}

impl PendingPicker {
    fn abort(self) {
        self.cancelled.store(true, Ordering::Release);
        self.handle.abort();
    }
}

#[derive(Default)]
pub(crate) struct Picker {
    generation: u64,
    pending: Option<PendingPicker>,
}

impl Drop for Picker {
    fn drop(&mut self) {
        if let Some(pending) = self.pending.take() {
            pending.abort();
            #[cfg(target_os = "macos")]
            {
                let _ = crate::platform::cancel_chooser();
            }
        }
    }
}

impl Picker {
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }

    pub fn finish(&mut self, generation: u64) -> bool {
        if generation != self.generation || !self.pending() {
            return false;
        }
        self.pending = None;
        true
    }

    pub fn reconcile(&mut self, active: Option<u64>) -> Task<Message> {
        if !self.pending() || active == Some(self.generation) {
            return Task::none();
        }
        if let Some(pending) = self.pending.take() {
            pending.abort();
        }
        dismiss_native_sheet()
    }

    pub fn start(
        &mut self,
        directory: PathBuf,
        receipt: fn(u64, Choice) -> Message,
    ) -> Option<(u64, Task<Message>)> {
        self.start_kind(directory, receipt, true)
    }

    pub fn start_target(
        &mut self,
        directory: PathBuf,
        receipt: fn(u64, Choice) -> Message,
    ) -> Option<(u64, Task<Message>)> {
        self.start_kind(directory, receipt, false)
    }

    fn start_kind(
        &mut self,
        directory: PathBuf,
        receipt: fn(u64, Choice) -> Message,
        folder: bool,
    ) -> Option<(u64, Task<Message>)> {
        if self.pending() {
            return None;
        }
        self.generation = self.generation.checked_add(1)?;
        let generation = self.generation;
        let cancelled = Arc::new(AtomicBool::new(false));
        let (task, handle) = choose(
            directory,
            generation,
            receipt,
            folder,
            Arc::clone(&cancelled),
        )
        .abortable();
        self.pending = Some(PendingPicker { cancelled, handle });
        Some((generation, task))
    }
}

fn choose(
    directory: PathBuf,
    generation: u64,
    receipt: fn(u64, Choice) -> Message,
    folder: bool,
    cancelled: Arc<AtomicBool>,
) -> Task<Message> {
    #[cfg(target_os = "macos")]
    {
        iced::window::latest().then(move |id| match id {
            Some(id) => {
                let directory = directory.clone();
                let cancelled = Arc::clone(&cancelled);
                iced::window::run(id, move |_| {
                    if folder {
                        crate::platform::choose_folder(directory, cancelled)
                    } else {
                        crate::platform::choose_target(directory, cancelled)
                    }
                })
                .then(move |choice| Task::perform(choice, move |result| receipt(generation, result)))
            }
            None => Task::done(receipt(
                generation,
                Err("Cannot open the chooser: no application window is available. Close configuration and try again.".into()),
            )),
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (directory, folder, cancelled);
        Task::done(receipt(
            generation,
            Err("Native selection is available on macOS only. Enter an absolute path or ~/path instead.".into()),
        ))
    }
}

fn dismiss_native_sheet() -> Task<Message> {
    #[cfg(target_os = "macos")]
    {
        if crate::platform::cancel_chooser() {
            return Task::none();
        }
        iced::window::latest().then(|id| match id {
            Some(id) => iced::window::run(id, |_| {
                let _ = crate::platform::cancel_chooser();
            })
            .then(|()| Task::none()),
            None => Task::none(),
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        Task::none()
    }
}
