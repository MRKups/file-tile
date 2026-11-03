//! One off-thread native observation owner, bounded hints, and a nonblocking UI future.
//!
//! Saved-path/box bounds limit registration to at most two shared roots per view.
//! A failed stream discards partial registration and reports unavailable updates;
//! replacing the observation set retries it. There is no filesystem polling.
//! Shutdown joins the owner after the runtime exits. A blocked native/path check
//! can delay teardown, never ordinary input or layout admission.

mod coalesce;
mod native;

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
};
use std::task::{Context, Poll, Waker};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use crate::application::contracts::watching::{
    DirectoryObservation, ObservationTag, WatchBatch, WatchState, WatchUpdate,
};
use crate::diagnostics::{Activity, Outcome, Subsystem, session};
use crate::domain::Workspace;

const INPUT_CAPACITY: usize = 64;

enum Signal {
    Wake,
    Changed {
        epoch: u64,
        tags: Vec<ObservationTag>,
        recheck: bool,
    },
    Fault {
        epoch: u64,
        tags: Vec<ObservationTag>,
        reason: String,
    },
}

#[derive(Default)]
struct Output {
    pending: HashMap<ObservationTag, WatchUpdate>,
    waker: Option<Waker>,
    closed: bool,
}

#[derive(Default)]
struct Overflow {
    epoch: u64,
    tags: HashSet<ObservationTag>,
    all: bool,
}

impl Overflow {
    fn record(&mut self, epoch: u64, tags: Vec<ObservationTag>) {
        if epoch < self.epoch {
            return;
        }
        if epoch > self.epoch {
            *self = Self {
                epoch,
                ..Self::default()
            };
        }
        if self.all {
            return;
        }
        for tag in tags {
            self.tags.insert(tag);
            if self.tags.len() > Workspace::MAX_BOXES {
                self.tags.clear();
                self.all = true;
                break;
            }
        }
    }
}

struct Shared {
    // A latest-wins mailbox, not an application model. No lock spans native or disk work.
    desired: Mutex<Option<Vec<DirectoryObservation>>>,
    sender: mpsc::SyncSender<Signal>,
    stopped: AtomicBool,
    overflow: AtomicBool,
    overflow_tags: Mutex<Overflow>,
    fault_epoch: AtomicU64,
    output: Arc<Mutex<Output>>,
}

impl Shared {
    fn signal(&self, signal: Signal) {
        let fault = match &signal {
            Signal::Fault { epoch, .. } => Some(*epoch),
            _ => None,
        };
        if let Err(mpsc::TrySendError::Full(signal)) = self.sender.try_send(signal) {
            if let Some(epoch) = fault {
                self.fault_epoch.fetch_max(epoch, Ordering::AcqRel);
            }
            let (epoch, tags) = match signal {
                Signal::Changed { epoch, tags, .. } | Signal::Fault { epoch, tags, .. } => {
                    (epoch, tags)
                }
                // The desired mailbox and stopped flag already retain a lost wake.
                Signal::Wake => return,
            };
            self.overflow_tags
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .record(epoch, tags);
            self.overflow.store(true, Ordering::Release);
        }
    }

    fn publish(&self, updates: impl IntoIterator<Item = WatchUpdate>) {
        let waker = {
            let mut output = self
                .output
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            for update in updates {
                let pending = output
                    .pending
                    .entry(update.tag)
                    .or_insert_with(|| WatchUpdate {
                        tag: update.tag,
                        state: None,
                        changed: false,
                    });
                if update.state.is_some() {
                    pending.state = update.state;
                }
                pending.changed |= update.changed;
            }
            output.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    fn retain_output(&self, current: &HashSet<ObservationTag>) {
        let mut output = self
            .output
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        output.pending.retain(|tag, _| current.contains(tag));
    }

    fn close(&self) {
        let waker = {
            let mut output = self
                .output
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            output.closed = true;
            output.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

pub struct DirectoryWatcher {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl DirectoryWatcher {
    pub fn new() -> io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(INPUT_CAPACITY);
        let shared = Arc::new(Shared {
            desired: Mutex::new(None),
            sender,
            stopped: AtomicBool::new(false),
            overflow: AtomicBool::new(false),
            overflow_tags: Mutex::new(Overflow::default()),
            fault_epoch: AtomicU64::new(0),
            output: Arc::new(Mutex::new(Output::default())),
        });
        let owner = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("file-tile-folder-watch".into())
            .spawn(move || run(owner, receiver))?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    /// Replace pending configuration without waiting for OS registration or queue space.
    pub fn replace(&self, observations: Vec<DirectoryObservation>) -> io::Result<()> {
        let tags: HashSet<_> = observations.iter().map(|item| item.tag).collect();
        if observations.len() > Workspace::MAX_BOXES
            || tags.len() != observations.len()
            || observations.iter().any(|item| {
                item.tag.generation == 0
                    || item.tag.view.0 == 0
                    || crate::application::contracts::configuration::validate_saved_path(
                        &item.directory,
                    )
                    .is_err()
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid or excessive directory observations",
            ));
        }
        if self.shared.stopped.load(Ordering::Acquire) {
            return Err(io::Error::other("Folder observation owner is stopped"));
        }
        *self
            .shared
            .desired
            .lock()
            .map_err(|_| io::Error::other("Folder observation coordination failed"))? =
            Some(observations);
        self.shared.signal(Signal::Wake);
        Ok(())
    }

    /// One runtime waiter drains merged receipts, then receives None after shutdown.
    /// Dropping it cannot drop native registration on the UI thread.
    pub fn next(&self) -> WatchFuture {
        WatchFuture {
            output: Arc::clone(&self.shared.output),
        }
    }

    pub(crate) fn shutdown(&mut self) -> io::Result<()> {
        self.shared.stopped.store(true, Ordering::Release);
        self.shared.signal(Signal::Wake);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            return Err(io::Error::other(
                "Folder observation owner failed; collect application error output",
            ));
        }
        Ok(())
    }
}

impl Drop for DirectoryWatcher {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            eprintln!("Folder observation shutdown failed: {error}");
        }
    }
}

pub struct WatchFuture {
    output: Arc<Mutex<Output>>,
}

impl Future for WatchFuture {
    type Output = Option<WatchBatch>;
    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let mut output = self
            .output
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !output.pending.is_empty() {
            Poll::Ready(Some(WatchBatch {
                updates: output.pending.drain().map(|(_, update)| update).collect(),
            }))
        } else if output.closed {
            Poll::Ready(None)
        } else {
            output.waker = Some(context.waker().clone());
            Poll::Pending
        }
    }
}

struct Owner {
    epoch: u64,
    observations: Vec<DirectoryObservation>,
    current: HashSet<ObservationTag>,
    native: Option<native::Native>,
    stamp: Vec<(std::path::PathBuf, u64, u64)>,
    pending: coalesce::Pending,
}

struct CloseOutput(Arc<Shared>);
impl Drop for CloseOutput {
    fn drop(&mut self) {
        self.0.stopped.store(true, Ordering::Release);
        self.0.close();
    }
}

impl Owner {
    fn recover_overflow(&mut self, shared: &Arc<Shared>) {
        let fault_epoch = shared.fault_epoch.swap(0, Ordering::AcqRel);
        if fault_epoch != 0 && fault_epoch == self.epoch && self.native.is_some() {
            self.native = None;
            session().record(
                Subsystem::Watcher,
                Activity::Register,
                Outcome::NativeFailure,
                self.epoch,
                self.current.len() as u64,
                None,
            );
            shared.publish(self.current.iter().map(|tag| WatchUpdate { tag: *tag, state: Some(WatchState::Unavailable("Native observation failed during an event burst; reattaching current folders.".into())), changed: false }));
        }
        if shared.overflow.swap(false, Ordering::AcqRel) {
            session().record(
                Subsystem::Watcher,
                Activity::Overflow,
                Outcome::Partial,
                self.epoch,
                0,
                None,
            );
            let overflow = std::mem::take(
                &mut *shared
                    .overflow_tags
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()),
            );
            if overflow.epoch == self.epoch {
                let tags = if overflow.all {
                    self.current.clone()
                } else {
                    overflow.tags.intersection(&self.current).copied().collect()
                };
                self.pending.add(tags, Instant::now(), true);
            }
        }
    }

    fn signal(&mut self, shared: &Arc<Shared>, signal: Signal) {
        match signal {
            Signal::Changed {
                epoch,
                tags,
                recheck,
            } if epoch == self.epoch && self.native.is_some() => {
                self.pending.add(
                    tags.into_iter().filter(|tag| self.current.contains(tag)),
                    Instant::now(),
                    recheck,
                );
            }
            Signal::Fault {
                epoch,
                tags,
                reason,
            } if epoch == self.epoch && self.native.is_some() => {
                self.native = None;
                session().record(
                    Subsystem::Watcher,
                    Activity::Register,
                    Outcome::NativeFailure,
                    epoch,
                    tags.len() as u64,
                    None,
                );
                shared.publish(
                    tags.into_iter()
                        .filter(|tag| self.current.contains(tag))
                        .map(|tag| WatchUpdate {
                            tag,
                            state: Some(WatchState::Unavailable(reason.clone())),
                            changed: false,
                        }),
                );
                self.pending
                    .add(self.current.iter().copied(), Instant::now(), true);
            }
            Signal::Wake | Signal::Changed { .. } | Signal::Fault { .. } => {}
        }
    }

    fn configure(&mut self, shared: &Arc<Shared>) {
        self.native = None;
        if self.observations.is_empty() {
            self.stamp.clear();
            return;
        }
        let plan = native::Plan::new(&self.observations);
        let Some(epoch) = self.epoch.checked_add(1) else {
            shared.publish(plan.states(Some("Native observation identities are exhausted. Save and relaunch; reload all boxes in the meantime.")));
            return;
        };
        self.epoch = epoch;
        let started = Instant::now();
        session().record(
            Subsystem::Watcher,
            Activity::Register,
            Outcome::Started,
            epoch,
            self.current.len() as u64,
            None,
        );
        let result = native::Native::start(&plan, Arc::clone(shared), epoch);
        session().record(
            Subsystem::Watcher,
            Activity::Register,
            if result.is_ok() {
                Outcome::Complete
            } else {
                Outcome::Unavailable
            },
            epoch,
            self.current.len() as u64,
            Some(started.elapsed()),
        );
        shared.publish(plan.states(result.as_ref().err().map(String::as_str)));
        self.stamp = plan.stamp;
        self.native = result.ok();
        // Reconfiguration can leave a notification gap. One tagged catch-up covers it.
        self.pending
            .add(self.current.iter().copied(), Instant::now(), false);
    }

    fn apply_desired(&mut self, shared: &Arc<Shared>) {
        let desired = shared
            .desired
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        if let Some(desired) = desired {
            self.observations = desired;
            self.current = self.observations.iter().map(|item| item.tag).collect();
            self.pending.retain(&self.current);
            shared.retain_output(&self.current);
            self.configure(shared);
        }
    }

    fn flush(&mut self, shared: &Arc<Shared>) {
        let recheck = self.pending.recheck;
        let tags = self.pending.take();
        shared.publish(tags.into_iter().map(|tag| WatchUpdate {
            tag,
            state: None,
            changed: true,
        }));
        // Listing admission does not wait for path revalidation or native registration.
        if recheck {
            let plan = native::Plan::new(&self.observations);
            if self.native.is_none() || self.stamp != plan.stamp {
                self.configure(shared);
            } else {
                shared.publish(plan.states(None));
            }
        }
    }
}

fn run(shared: Arc<Shared>, receiver: mpsc::Receiver<Signal>) {
    let _close = CloseOutput(Arc::clone(&shared));
    let mut owner = Owner {
        epoch: 0,
        observations: Vec::new(),
        current: HashSet::new(),
        native: None,
        stamp: Vec::new(),
        pending: coalesce::Pending::default(),
    };
    while !shared.stopped.load(Ordering::Acquire) {
        owner.apply_desired(&shared);
        owner.recover_overflow(&shared);
        if owner
            .pending
            .deadline()
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            owner.flush(&shared);
        }
        let received = match owner.pending.deadline() {
            Some(deadline) => {
                receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            }
            None => receiver
                .recv()
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected),
        };
        match received {
            Ok(signal) => owner.signal(&shared, signal),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    drop(owner);
}
