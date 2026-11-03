//! One blocking owner, one pending publication, and one replaceable budget check.

use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll, Waker};
use std::thread::{self, JoinHandle};

use crate::application::contracts::storage::{
    BudgetRequest, SaveOutcome, SaveRequest, SaveTag, SessionId, StorageCompletion, StorageFailure,
};

use super::store::Store;
use crate::diagnostics::{Activity, Outcome, Subsystem, Work, session};

#[derive(Default)]
struct Reply {
    result: Option<StorageCompletion>,
    completed: bool,
    waker: Option<Waker>,
}

#[derive(Clone)]
enum Request {
    Check(BudgetRequest),
    Load(SessionId),
    Restore {
        session: SessionId,
        token: crate::application::contracts::configuration::WorkspaceId,
    },
    Save(SaveRequest),
    Manage(crate::application::contracts::storage::ManagementRequest),
}

impl Request {
    fn record(&self, id: u64, outcome: Outcome) {
        let (activity, detail) = match self {
            Self::Check(r) => (Activity::Validate, r.generation),
            Self::Load(_) => (Activity::Load, 0),
            Self::Restore { .. } => (Activity::Restore, 0),
            Self::Save(r) => (Activity::Save, r.tag.revision),
            Self::Manage(r) => (Activity::Manage, r.attempt),
        };
        session().record(Subsystem::Storage, activity, outcome, id, detail, None);
    }
    fn failed(&self, error: StorageFailure) -> StorageCompletion {
        match self {
            Self::Manage(request) => StorageCompletion::Managed {
                operation: request.receipt_tag(),
                result: Err(error),
            },
            Self::Check(request) => StorageCompletion::BudgetChecked {
                generation: request.generation,
                result: Err(error),
            },
            Self::Load(session) | Self::Restore { session, .. } => StorageCompletion::Loaded {
                session: *session,
                result: Err(error),
            },
            Self::Save(request) => StorageCompletion::Saved {
                tag: request.tag,
                result: Err(error),
            },
        }
    }
}

struct Job {
    diagnostic: Work,
    request: Request,
    reply: Arc<Mutex<Reply>>,
}

impl Job {
    fn finish(&self, result: StorageCompletion) {
        let waker = {
            let mut reply = self.reply.lock().unwrap_or_else(|e| e.into_inner());
            if reply.completed {
                return;
            }
            self.request
                .record(self.diagnostic.id, completion_outcome(&result));
            reply.completed = true;
            reply.result = Some(result);
            reply.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // Includes worker panic/disconnection, so futures cannot wait forever.
        self.finish(self.request.failed(StorageFailure::Unavailable));
    }
}

struct QueueState {
    pending: Option<Job>,
    validation: Option<Job>,
    stopped: bool,
    phase: Phase,
    latest: Option<SaveTag>,
}

#[derive(Clone, Copy)]
enum Phase {
    New,
    Loading,
    Ready(SessionId),
    Recovery,
    Managing,
}

struct Queue {
    state: Mutex<QueueState>,
    available: Condvar,
}

pub struct StorageWorker {
    queue: Arc<Queue>,
    thread: Option<JoinHandle<()>>,
}

impl StorageWorker {
    pub fn is_stopped(&self) -> bool {
        self.queue.state.lock().map_or(true, |state| state.stopped)
    }
    pub fn new(root: PathBuf) -> io::Result<Self> {
        let queue = Arc::new(Queue {
            state: Mutex::new(QueueState {
                pending: None,
                validation: None,
                stopped: false,
                phase: Phase::New,
                latest: None,
            }),
            available: Condvar::new(),
        });
        let worker_queue = Arc::clone(&queue);
        let thread = thread::Builder::new()
            .name("file-tile-storage".into())
            .spawn(move || worker_loop(worker_queue, root))?;
        Ok(Self {
            queue,
            thread: Some(thread),
        })
    }

    pub fn load(&self, session: SessionId) -> StorageFuture {
        self.submit(Request::Load(session))
    }

    pub fn save(&self, request: SaveRequest) -> StorageFuture {
        self.submit(Request::Save(request))
    }
    pub fn manage(
        &self,
        request: crate::application::contracts::storage::ManagementRequest,
    ) -> StorageFuture {
        self.submit(Request::Manage(request))
    }
    pub fn check_workspace(&self, request: BudgetRequest) -> StorageFuture {
        self.submit(Request::Check(request))
    }

    pub fn restore(
        &self,
        session: SessionId,
        token: crate::application::contracts::configuration::WorkspaceId,
    ) -> StorageFuture {
        self.submit(Request::Restore { session, token })
    }

    fn submit(&self, request: Request) -> StorageFuture {
        let reply = Arc::new(Mutex::new(Reply::default()));
        let job = Job {
            diagnostic: Work::new(Subsystem::Storage),
            request: request.clone(),
            reply: Arc::clone(&reply),
        };
        job.request.record(job.diagnostic.id, Outcome::Requested);
        match self.queue.state.lock() {
            Ok(mut state) => {
                let error = admission_error(&state, &request);
                if let Some(error) = error {
                    job.finish(request.failed(error));
                } else {
                    job.request.record(job.diagnostic.id, Outcome::Queued);
                    match &request {
                        Request::Manage(request) => {
                            state.phase = Phase::Managing;
                            state.latest = Some(request.source.tag);
                        }
                        Request::Load(_) | Request::Restore { .. } => {
                            state.phase = Phase::Loading;
                            state.latest = None;
                        }
                        Request::Save(request) => state.latest = Some(request.tag),
                        Request::Check(_) => {}
                    }
                    if matches!(request, Request::Check(_)) {
                        if let Some(previous) = state.validation.replace(job) {
                            previous.finish(previous.request.failed(StorageFailure::StaleRequest));
                        }
                    } else if let Some(previous) = state.pending.replace(job)
                        && let Request::Save(request) = &previous.request
                    {
                        previous.finish(StorageCompletion::Saved {
                            tag: request.tag,
                            result: Ok(SaveOutcome::Superseded),
                        });
                    }
                    self.queue.available.notify_one();
                }
            }
            Err(_) => job.finish(request.failed(StorageFailure::Unavailable)),
        }
        StorageFuture {
            reply,
            failure: request.failed(StorageFailure::Unavailable),
        }
    }

    pub(crate) fn shutdown(&mut self) -> io::Result<()> {
        stop(&self.queue);
        if self
            .thread
            .take()
            .is_some_and(|thread| thread.join().is_err())
        {
            return Err(io::Error::other(
                "Storage worker failed; collect application error output",
            ));
        }
        Ok(())
    }
}

fn admission_error(state: &QueueState, request: &Request) -> Option<StorageFailure> {
    if state.stopped {
        return Some(StorageFailure::Unavailable);
    }
    match request {
        Request::Manage(request) if !matches!(state.phase, Phase::Ready(session) if session == request.source.tag.session) => {
            Some(StorageFailure::StaleRequest)
        }
        Request::Manage(request)
            if state.latest.is_some_and(|latest| {
                request.source.tag.workspace != latest.workspace
                    || request.source.tag.revision < latest.revision
                    || request.source.tag.attempt < latest.attempt
            }) =>
        {
            Some(StorageFailure::StaleRequest)
        }
        Request::Check(_) if matches!(state.phase, Phase::Managing) => {
            Some(StorageFailure::StaleRequest)
        }
        Request::Load(_) if !matches!(state.phase, Phase::New | Phase::Recovery) => {
            Some(StorageFailure::StaleRequest)
        }
        Request::Restore { .. } if !matches!(state.phase, Phase::Recovery) => {
            Some(StorageFailure::StaleRequest)
        }
        Request::Save(request) if !matches!(state.phase, Phase::Ready(session) if session == request.tag.session) => {
            Some(StorageFailure::StaleRequest)
        }
        Request::Save(request)
            if state.latest.is_some_and(|latest| {
                request.tag.session != latest.session
                    || request.tag.workspace != latest.workspace
                    || request.tag.revision < latest.revision
                    || request.tag.attempt <= latest.attempt
            }) =>
        {
            Some(StorageFailure::StaleRequest)
        }
        _ => None,
    }
}

impl Drop for StorageWorker {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            eprintln!("Storage shutdown failed: {error}");
        }
    }
}

pub struct StorageFuture {
    reply: Arc<Mutex<Reply>>,
    failure: StorageCompletion,
}

impl Future for StorageFuture {
    type Output = StorageCompletion;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let future = self.get_mut();
        let Ok(mut reply) = future.reply.lock() else {
            return Poll::Ready(future.failure.clone());
        };
        if let Some(result) = reply.result.take() {
            Poll::Ready(result)
        } else {
            reply.waker = Some(context.waker().clone());
            Poll::Pending
        }
    }
}

struct WorkerLifetime(Arc<Queue>);

impl Drop for WorkerLifetime {
    fn drop(&mut self) {
        stop(&self.0);
    }
}

fn stop(queue: &Queue) {
    let pending = {
        let mut state = queue.state.lock().unwrap_or_else(|e| e.into_inner());
        state.stopped = true;
        (state.pending.take(), state.validation.take())
    };
    drop(pending);
    queue.available.notify_all();
}

fn worker_loop(queue: Arc<Queue>, root: PathBuf) {
    let _lifetime = WorkerLifetime(Arc::clone(&queue));
    let mut store = None;
    loop {
        let job = {
            let Ok(mut state) = queue.state.lock() else {
                return;
            };
            while state.pending.is_none() && state.validation.is_none() && !state.stopped {
                let Ok(next) = queue.available.wait(state) else {
                    return;
                };
                state = next;
            }
            if state.stopped {
                return;
            }
            state.pending.take().or_else(|| state.validation.take())
        };
        if let Some(job) = job {
            let started = job.diagnostic.start();
            let completion = execute(&job.request, &root, &mut store);
            job.diagnostic
                .finish(completion_outcome(&completion), started);
            if let StorageCompletion::Loaded { session, result } = &completion {
                let Ok(mut state) = queue.state.lock() else {
                    return;
                };
                state.phase = if result
                    .as_ref()
                    .is_ok_and(|loaded| loaded.recovery_issues.is_empty())
                {
                    Phase::Ready(*session)
                } else {
                    Phase::Recovery
                };
            }
            if let (Request::Manage(request), StorageCompletion::Managed { result, .. }) =
                (&job.request, &completion)
            {
                let Ok(mut state) = queue.state.lock() else {
                    return;
                };
                let selected = result
                    .as_ref()
                    .ok()
                    .and_then(|result| result.selected.as_ref());
                state.phase = Phase::Ready(if selected.is_some() {
                    request.next_session
                } else {
                    request.source.tag.session
                });
                if selected.is_some() {
                    state.latest = None;
                }
            }
            job.finish(completion);
        }
    }
}

fn execute(
    request: &Request,
    root: &std::path::Path,
    store: &mut Option<Store>,
) -> StorageCompletion {
    match request {
        Request::Manage(request) => {
            let result = store
                .as_mut()
                .ok_or(StorageFailure::Unavailable)
                .and_then(|store| store.manage(request));
            StorageCompletion::Managed {
                operation: request.receipt_tag(),
                result,
            }
        }
        Request::Check(request) => {
            let result = super::encode(
                &crate::application::contracts::configuration::ConfigurationDocument::Workspace(
                    request.document.as_ref().clone(),
                ),
            )
            .map(|_| ())
            .map_err(|error| StorageFailure::InvalidConfiguration(error.reason().into()));
            StorageCompletion::BudgetChecked {
                generation: request.generation,
                result,
            }
        }
        Request::Load(session) => {
            let result = if store.is_none() {
                Store::open(root).map(|opened| *store = Some(opened))
            } else {
                Ok(())
            }
            .and_then(|()| {
                let opened = store.as_mut().ok_or(StorageFailure::Unavailable)?;
                opened.load(*session)
            });
            StorageCompletion::Loaded {
                session: *session,
                result: result.map(Box::new),
            }
        }
        Request::Restore { session, token } => {
            let result = store
                .as_mut()
                .ok_or(StorageFailure::Unavailable)
                .and_then(|store| store.restore(*session, *token));
            StorageCompletion::Loaded {
                session: *session,
                result: result.map(Box::new),
            }
        }
        Request::Save(request) => {
            let result = store
                .as_mut()
                .ok_or(StorageFailure::Unavailable)
                .and_then(|store| store.save(request));
            StorageCompletion::Saved {
                tag: request.tag,
                result: result.map(|()| SaveOutcome::Saved),
            }
        }
    }
}

fn completion_outcome(completion: &StorageCompletion) -> Outcome {
    match completion {
        StorageCompletion::Loaded { result, .. } => {
            result
                .as_ref()
                .map_or_else(StorageFailure::diagnostic_outcome, |loaded| {
                    if loaded.recovery_issues.is_empty() {
                        Outcome::Complete
                    } else {
                        Outcome::RecoveryOnly
                    }
                })
        }
        StorageCompletion::Saved { result, .. } => {
            result
                .as_ref()
                .map_or_else(StorageFailure::diagnostic_outcome, |saved| match saved {
                    SaveOutcome::Saved => Outcome::Complete,
                    SaveOutcome::Superseded => Outcome::Superseded,
                })
        }
        StorageCompletion::BudgetChecked { result, .. } => result
            .as_ref()
            .map_or_else(StorageFailure::diagnostic_outcome, |_| Outcome::Complete),
        StorageCompletion::Managed { result, .. } => {
            result
                .as_ref()
                .map_or_else(StorageFailure::diagnostic_outcome, |managed| {
                    if let Some(error) = &managed.source_error {
                        error.diagnostic_outcome()
                    } else if managed.cancelled {
                        Outcome::Cancelled
                    } else {
                        Outcome::Complete
                    }
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::contracts::configuration::{WorkspaceDocument, WorkspaceId};
    use crate::domain::Workspace;
    use crate::persistence::tests::storage::wait;

    #[test]
    fn native_budget_validation_never_initializes_or_publishes_state()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::application::contracts::configuration::ConfigurationDocument;
        use crate::domain::WindowPlacement;
        use crate::persistence::tests::storage::{Fixture, request};
        use crate::persistence::{ConfigKey, encode};
        let fixture = Fixture::new()?;
        let mut worker = StorageWorker::new(fixture.path().to_path_buf())?;
        let id = WorkspaceId::random();
        let document = Arc::new(WorkspaceDocument {
            id,
            workspace: Workspace::default(),
        });
        assert!(matches!(
            wait(worker.check_workspace(BudgetRequest {
                generation: 1,
                document
            })),
            StorageCompletion::BudgetChecked {
                generation: 1,
                result: Ok(())
            }
        ));
        assert!(
            std::fs::read_dir(fixture.path())?.next().is_none(),
            "Validation must not create a Store or writer lock"
        );
        let session = SessionId(WorkspaceId::random());
        let loaded = match wait(worker.load(session)) {
            StorageCompletion::Loaded {
                session: completed,
                result: Ok(loaded),
            } if completed == session => loaded,
            other => return Err(format!("Expected successful load: {other:?}").into()),
        };
        let mut saved = request(&loaded, session, 1);
        assert_eq!(saved.document.workspace.boxes().len(), 1);
        saved.preferences.window = Some(WindowPlacement {
            x: 120,
            y: 90,
            width: 960,
            height: 640,
        });
        assert!(matches!(
            wait(worker.save(saved.clone())),
            StorageCompletion::Saved {
                tag,
                result: Ok(SaveOutcome::Saved),
            } if tag == saved.tag
        ));
        let config = fixture.path().join("config");
        // Include the complete file set, so a newly created backup or temporary
        // file is a failure even when every existing primary keeps its bytes.
        let before = fixture.config_bytes()?;
        let key = ConfigKey::Workspace(loaded.active.id);
        assert!(before.contains_key(std::ffi::OsStr::new(&format!("{}.bak", key.filename()))));
        assert!(before.contains_key(std::ffi::OsStr::new("preferences.json.bak")));
        let proposal = request(&loaded, session, 2).document;
        assert_ne!(proposal.workspace.boxes(), saved.document.workspace.boxes());
        assert!(matches!(
            wait(worker.check_workspace(BudgetRequest {
                generation: 3,
                document: proposal,
            })),
            StorageCompletion::BudgetChecked {
                generation: 3,
                result: Ok(()),
            }
        ));
        assert_eq!(
            fixture.config_bytes()?,
            before,
            "Checking an unaccepted proposal cannot publish it"
        );
        saved.tag.revision = 2;
        saved.tag.attempt = 2;
        saved.preferences.new_box_folder = Some("/fixture/accepted-default".into());
        assert!(matches!(
            wait(worker.save(saved.clone())),
            StorageCompletion::Saved {
                tag,
                result: Ok(SaveOutcome::Saved),
            } if tag == saved.tag
        ));
        assert_eq!(
            std::fs::read(config.join(key.filename()))?,
            encode(&ConfigurationDocument::Workspace(
                saved.document.as_ref().clone(),
            ))?
        );
        assert_eq!(
            std::fs::read(config.join("preferences.json"))?,
            encode(&ConfigurationDocument::Preferences(saved.preferences))?
        );
        worker.shutdown()?;
        Ok(())
    }
}
