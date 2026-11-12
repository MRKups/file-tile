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
const MAX_LOCATION_ROOTS: usize = 16;

type DesiredConfig = (Vec<DirectoryObservation>, Vec<std::path::PathBuf>);

enum Signal {
    Wake,
    Changed {
        epoch: u64,
        tags: Vec<ObservationTag>,
        locations: bool,
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
    locations_changed: bool,
    waker: Option<Waker>,
    closed: bool,
}

#[derive(Default)]
struct Overflow {
    epoch: u64,
    tags: HashSet<ObservationTag>,
    locations: bool,
    all: bool,
}

impl Overflow {
    fn record(&mut self, epoch: u64, tags: Vec<ObservationTag>, locations: bool) {
        if epoch < self.epoch {
            return;
        }
        if epoch > self.epoch {
            *self = Self {
                epoch,
                ..Self::default()
            };
        }
        self.locations |= locations;
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
    desired: Mutex<Option<DesiredConfig>>,
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
            let (epoch, tags, locations) = match signal {
                Signal::Changed {
                    epoch,
                    tags,
                    locations,
                    ..
                } => (epoch, tags, locations),
                Signal::Fault { epoch, tags, .. } => (epoch, tags, true),
                // The desired mailbox and stopped flag already retain a lost wake.
                Signal::Wake => return,
            };
            self.overflow_tags
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .record(epoch, tags, locations);
            self.overflow.store(true, Ordering::Release);
        }
    }

    fn publish(&self, updates: impl IntoIterator<Item = WatchUpdate>, locations: bool) {
        let waker = {
            let mut output = self
                .output
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            output.locations_changed |= locations;
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
            if !output.pending.is_empty() || output.locations_changed {
                output.waker.take()
            } else {
                None
            }
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    fn retain_output(&self, current: &HashSet<ObservationTag>, keep_locations: bool) {
        let mut output = self
            .output
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        output.pending.retain(|tag, _| current.contains(tag));
        if !keep_locations {
            output.locations_changed = false;
        }
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
        self.replace_with_locations(observations, Vec::new())
    }

    /// Replace pending folder observations and non-recursive location roots atomically.
    pub fn replace_with_locations(
        &self,
        observations: Vec<DirectoryObservation>,
        location_roots: Vec<std::path::PathBuf>,
    ) -> io::Result<()> {
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
            || location_roots.len() > MAX_LOCATION_ROOTS
            || location_roots.iter().any(|root| {
                crate::application::contracts::configuration::validate_saved_path(root).is_err()
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
            Some((observations, location_roots));
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
        if !output.pending.is_empty() || output.locations_changed {
            Poll::Ready(Some(WatchBatch {
                updates: output.pending.drain().map(|(_, update)| update).collect(),
                locations_changed: std::mem::replace(&mut output.locations_changed, false),
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
    location_roots: Vec<std::path::PathBuf>,
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
            shared.publish(
                self.current.iter().map(|tag| WatchUpdate {
                    tag: *tag,
                    state: Some(WatchState::Unavailable(
                        "Native observation failed during an event burst; reattaching current folders.".into(),
                    )),
                    changed: false,
                }),
                false,
            );
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
                let now = Instant::now();
                let tags = if overflow.all {
                    self.current.clone()
                } else {
                    overflow.tags.intersection(&self.current).copied().collect()
                };
                self.pending.add(tags, now, true);
                if (overflow.locations || overflow.all) && !self.location_roots.is_empty() {
                    self.pending.add_locations(now, true);
                }
            }
        }
    }

    fn signal(&mut self, shared: &Arc<Shared>, signal: Signal) {
        match signal {
            Signal::Changed {
                epoch,
                tags,
                locations,
                recheck,
            } if epoch == self.epoch && self.native.is_some() => {
                let now = Instant::now();
                self.pending.add(
                    tags.into_iter().filter(|tag| self.current.contains(tag)),
                    now,
                    recheck,
                );
                if locations && !self.location_roots.is_empty() {
                    self.pending.add_locations(now, recheck);
                }
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
                    false,
                );
                let now = Instant::now();
                self.pending.add(self.current.iter().copied(), now, true);
                if !self.location_roots.is_empty() {
                    self.pending.add_locations(now, true);
                }
            }
            Signal::Wake | Signal::Changed { .. } | Signal::Fault { .. } => {}
        }
    }

    fn configure(&mut self, shared: &Arc<Shared>) {
        self.native = None;
        if self.observations.is_empty() && self.location_roots.is_empty() {
            self.stamp.clear();
            return;
        }
        let plan = native::Plan::new(&self.observations, &self.location_roots);
        let Some(epoch) = self.epoch.checked_add(1) else {
            shared.publish(
                plan.states(Some(
                    "Native observation identities are exhausted. Save and relaunch; reload all boxes in the meantime.",
                )),
                false,
            );
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
        shared.publish(
            plan.states(result.as_ref().err().map(String::as_str)),
            false,
        );
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
        if let Some((observations, location_roots)) = desired {
            self.observations = observations;
            self.location_roots = location_roots;
            self.current = self.observations.iter().map(|item| item.tag).collect();
            if self.location_roots.is_empty() {
                self.pending.locations = false;
            }
            self.pending.retain(&self.current);
            shared.retain_output(&self.current, !self.location_roots.is_empty());
            self.configure(shared);
        }
    }

    fn flush(&mut self, shared: &Arc<Shared>) {
        let recheck = self.pending.recheck;
        let (tags, locations) = self.pending.take();
        shared.publish(
            tags.into_iter().map(|tag| WatchUpdate {
                tag,
                state: None,
                changed: true,
            }),
            locations,
        );
        // Listing admission does not wait for path revalidation or native registration.
        if recheck {
            let plan = native::Plan::new(&self.observations, &self.location_roots);
            if self.native.is_none() || self.stamp != plan.stamp {
                self.configure(shared);
            } else {
                shared.publish(plan.states(None), false);
            }
        }
    }
}

fn run(shared: Arc<Shared>, receiver: mpsc::Receiver<Signal>) {
    let _close = CloseOutput(Arc::clone(&shared));
    let mut owner = Owner {
        epoch: 0,
        observations: Vec::new(),
        location_roots: Vec::new(),
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
