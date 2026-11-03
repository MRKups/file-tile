//! Owned blocking reads with bounded admission and a nonblocking future boundary.

use crate::diagnostics::{Activity, Outcome, Subsystem, Work, session};
use std::collections::VecDeque;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll, Waker};
use std::thread::{self, JoinHandle};

use crate::application::contracts::directory::{
    Cancellation, DirectoryError, DirectoryErrorKind, DirectoryListing, DirectoryReader,
    ListingCompletion, ListingRequest,
};

type ReadResult = Result<DirectoryListing, DirectoryError>;

#[derive(Default)]
struct Reply {
    result: Option<ReadResult>,
    waker: Option<Waker>,
}

struct Job {
    diagnostic: Work,
    request: ListingRequest,
    cancellation: Cancellation,
    reply: Arc<Mutex<Reply>>,
    mode: ReadMode,
}

enum ReadMode {
    List(crate::domain::Scope),
    Resolve,
}

struct QueueState {
    jobs: VecDeque<Job>,
    stopped: bool,
}
struct Queue {
    state: Mutex<QueueState>,
    available: Condvar,
    capacity: usize,
}

pub struct DirectoryWorkers {
    queue: Arc<Queue>,
    shutdown: Cancellation,
    threads: Vec<JoinHandle<()>>,
}

impl DirectoryWorkers {
    pub fn new(
        reader: Arc<dyn DirectoryReader + Send + Sync>,
        workers: usize,
        capacity: usize,
    ) -> io::Result<Self> {
        if workers == 0 || capacity == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Directory workers and queue capacity must be nonzero",
            ));
        }
        let mut pool = Self {
            queue: Arc::new(Queue {
                state: Mutex::new(QueueState {
                    jobs: VecDeque::new(),
                    stopped: false,
                }),
                available: Condvar::new(),
                capacity,
            }),
            shutdown: Cancellation::default(),
            threads: Vec::new(),
        };
        for index in 0..workers {
            let queue = Arc::clone(&pool.queue);
            let reader = Arc::clone(&reader);
            pool.threads.push(
                thread::Builder::new()
                    .name(format!("file-tile-directory-{index}"))
                    .spawn(move || worker_loop(queue, reader))?,
            );
        }
        Ok(pool)
    }

    /// Admission never waits for directory I/O. Cancelled queued jobs make room first.
    pub fn submit(&self, request: ListingRequest) -> DirectoryFuture {
        self.submit_scoped(request, crate::domain::Scope::default())
    }
    pub fn submit_scoped(
        &self,
        request: ListingRequest,
        scope: crate::domain::Scope,
    ) -> DirectoryFuture {
        self.submit_job(request, ReadMode::List(scope))
    }
    pub fn resolve(&self, request: ListingRequest) -> DirectoryFuture {
        self.submit_job(request, ReadMode::Resolve)
    }
    fn submit_job(&self, request: ListingRequest, mode: ReadMode) -> DirectoryFuture {
        let cancellation = self.shutdown.child();
        let reply = Arc::new(Mutex::new(Reply::default()));
        let job = Job {
            diagnostic: Work::new(Subsystem::Listing),
            request: request.clone(),
            cancellation: cancellation.clone(),
            reply: Arc::clone(&reply),
            mode,
        };
        let diagnostic = job.diagnostic.clone();
        session().record(
            Subsystem::Listing,
            Activity::Work,
            Outcome::Requested,
            diagnostic.id,
            request.generation,
            None,
        );
        let mut removed = Vec::new();
        let mut rejected = None;
        match self.queue.state.lock() {
            Ok(mut state) => {
                let mut keep = VecDeque::new();
                while let Some(queued) = state.jobs.pop_front() {
                    if queued.cancellation.is_cancelled() {
                        removed.push(queued);
                    } else {
                        keep.push_back(queued);
                    }
                }
                state.jobs = keep;
                if state.stopped {
                    rejected = Some(DirectoryErrorKind::Unavailable);
                } else if state.jobs.len() >= self.queue.capacity {
                    rejected = Some(DirectoryErrorKind::Busy);
                } else {
                    diagnostic.event(Outcome::Queued);
                    state.jobs.push_back(job);
                    self.queue.available.notify_one();
                }
            }
            Err(_) => rejected = Some(DirectoryErrorKind::Unavailable),
        }
        for removed in removed {
            finish_cancelled(removed);
        }
        if let Some(kind) = rejected {
            diagnostic.event(kind.diagnostic_outcome());
            finish(
                &reply,
                Err(DirectoryError::new(request.directory.clone(), kind)),
            );
        }
        DirectoryFuture {
            diagnostic,
            completed: false,
            request,
            cancellation,
            reply,
        }
    }

    pub(crate) fn shutdown(&mut self) -> io::Result<()> {
        self.stop();
        let mut failed = false;
        for worker in self.threads.drain(..) {
            failed |= worker.join().is_err();
        }
        if failed {
            Err(io::Error::other(
                "A directory worker failed; collect the application error output",
            ))
        } else {
            Ok(())
        }
    }

    fn stop(&self) {
        self.shutdown.cancel();
        let jobs = {
            // Poisoned coordination state is not used for further reads. Recover
            // ownership only to close the queue, resolve replies, and join workers.
            let mut state = self.queue.state.lock().unwrap_or_else(|e| e.into_inner());
            state.stopped = true;
            state.jobs.drain(..).collect::<Vec<_>>()
        };
        self.queue.available.notify_all();
        for job in jobs {
            finish_cancelled(job);
        }
    }
}

impl Drop for DirectoryWorkers {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            eprintln!("Directory worker shutdown failed: {error}");
        }
    }
}

pub struct DirectoryFuture {
    diagnostic: Work,
    completed: bool,
    request: ListingRequest,
    cancellation: Cancellation,
    reply: Arc<Mutex<Reply>>,
}

impl DirectoryFuture {
    pub fn cancellation(&self) -> Cancellation {
        self.cancellation.clone()
    }
}

impl Future for DirectoryFuture {
    type Output = ListingCompletion;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let future = self.get_mut();
        if future.cancellation.is_cancelled() {
            future.completed = true;
            return Poll::Ready(ListingCompletion {
                request: future.request.clone(),
                result: Err(DirectoryError::new(
                    future.request.directory.clone(),
                    DirectoryErrorKind::Cancelled,
                )),
            });
        }
        let mut reply = match future.reply.lock() {
            Ok(reply) => reply,
            Err(_) => {
                future.completed = true;
                return Poll::Ready(ListingCompletion {
                    request: future.request.clone(),
                    result: Err(DirectoryError::new(
                        future.request.directory.clone(),
                        DirectoryErrorKind::Unavailable,
                    )),
                });
            }
        };
        match reply.result.take() {
            Some(result) => {
                future.completed = true;
                Poll::Ready(ListingCompletion {
                    request: future.request.clone(),
                    result,
                })
            }
            None => {
                reply.waker = Some(context.waker().clone());
                Poll::Pending
            }
        }
    }
}

impl Drop for DirectoryFuture {
    fn drop(&mut self) {
        if !self.completed {
            self.diagnostic.event(Outcome::CancelRequested);
        }
        self.cancellation.cancel();
    }
}

fn worker_loop(queue: Arc<Queue>, reader: Arc<dyn DirectoryReader + Send + Sync>) {
    loop {
        let job = {
            let Ok(mut state) = queue.state.lock() else {
                return;
            };
            while state.jobs.is_empty() && !state.stopped {
                let Ok(next) = queue.available.wait(state) else {
                    return;
                };
                state = next;
            }
            if state.stopped {
                return;
            }
            state.jobs.pop_front()
        };
        if let Some(job) = job {
            let started = job.diagnostic.start();
            let result = match job.mode {
                ReadMode::List(scope) => {
                    reader.list_scoped(&job.request.directory, &scope, &job.cancellation)
                }
                ReadMode::Resolve => reader
                    .resolve_directory(&job.request.directory, &job.cancellation)
                    .map(|directory| {
                        DirectoryListing::new(
                            directory,
                            crate::application::contracts::directory::ListingOrigin::Local,
                            vec![],
                            vec![],
                        )
                    }),
            };
            let outcome = result.as_ref().map_or_else(
                |error| error.kind.diagnostic_outcome(),
                |listing| {
                    if listing.issues.is_empty() {
                        Outcome::Complete
                    } else {
                        Outcome::Partial
                    }
                },
            );
            job.diagnostic.finish(outcome, started);
            finish(&job.reply, result);
        }
    }
}

fn finish_cancelled(job: Job) {
    job.diagnostic.event(Outcome::Cancelled);
    job.cancellation.cancel();
    finish(
        &job.reply,
        Err(DirectoryError::new(
            job.request.directory,
            DirectoryErrorKind::Cancelled,
        )),
    );
}

fn finish(reply: &Mutex<Reply>, result: ReadResult) {
    let waker = {
        let mut state = reply.lock().unwrap_or_else(|e| e.into_inner());
        state.result = Some(result);
        state.waker.take()
    };
    if let Some(waker) = waker {
        waker.wake();
    }
}
