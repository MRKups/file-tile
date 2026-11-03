//! Small bounded file-detail worker queue. No I/O while holding coordination locks.
use crate::application::contracts::directory::Cancellation;
use crate::diagnostics::{Outcome, Subsystem, Work};
use std::collections::VecDeque;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll, Waker};
use std::thread::{self, JoinHandle};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkError {
    Busy,
    Cancelled,
    Unavailable,
}
struct Reply<T> {
    result: Option<Result<T, WorkError>>,
    waker: Option<Waker>,
}
struct Job<T> {
    diagnostic: Work,
    cancellation: Cancellation,
    reply: Arc<Mutex<Reply<T>>>,
    run: Box<dyn FnOnce(&Cancellation) -> T + Send>,
}
struct State<T> {
    jobs: VecDeque<Job<T>>,
    stopped: bool,
}
struct Queue<T> {
    state: Mutex<State<T>>,
    wake: Condvar,
    capacity: usize,
}
pub(crate) struct Workers<T> {
    subsystem: Subsystem,
    queue: Arc<Queue<T>>,
    shutdown: Cancellation,
    threads: Vec<JoinHandle<()>>,
}
impl<T: Send + 'static> Workers<T> {
    pub fn new(name: &str, workers: usize, capacity: usize) -> io::Result<Self> {
        if workers == 0 || capacity == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "File worker count and capacity must be nonzero",
            ));
        }
        let mut owner = Self {
            subsystem: match name {
                "folder-usage" => Subsystem::Size,
                "file-operations" => Subsystem::Files,
                "opening" => Subsystem::Opening,
                "sorting" => Subsystem::Sorting,
                "preview" => Subsystem::Thumbnail,
                _ => Subsystem::Metadata,
            },
            queue: Arc::new(Queue {
                state: Mutex::new(State {
                    jobs: VecDeque::new(),
                    stopped: false,
                }),
                wake: Condvar::new(),
                capacity,
            }),
            shutdown: Cancellation::default(),
            threads: vec![],
        };
        for index in 0..workers {
            let queue = Arc::clone(&owner.queue);
            owner.threads.push(
                thread::Builder::new()
                    .name(format!("file-tile-{name}-{index}"))
                    .spawn(move || run(queue))?,
            );
        }
        Ok(owner)
    }
    pub fn submit(&self, run: impl FnOnce(&Cancellation) -> T + Send + 'static) -> WorkFuture<T> {
        let cancellation = self.shutdown.child();
        let reply = Arc::new(Mutex::new(Reply {
            result: None,
            waker: None,
        }));
        let job = Job {
            diagnostic: Work::new(self.subsystem),
            cancellation: cancellation.clone(),
            reply: Arc::clone(&reply),
            run: Box::new(run),
        };
        let diagnostic = job.diagnostic.clone();
        diagnostic.event(Outcome::Requested);
        let mut obsolete = vec![];
        let failure = match self.queue.state.lock() {
            Ok(mut state) => {
                let mut keep = VecDeque::new();
                while let Some(job) = state.jobs.pop_front() {
                    if job.cancellation.is_cancelled() {
                        obsolete.push(job);
                    } else {
                        keep.push_back(job);
                    }
                }
                state.jobs = keep;
                if state.stopped {
                    Some(WorkError::Unavailable)
                } else if state.jobs.len() >= self.queue.capacity {
                    Some(WorkError::Busy)
                } else {
                    diagnostic.event(Outcome::Queued);
                    state.jobs.push_back(job);
                    self.queue.wake.notify_one();
                    None
                }
            }
            Err(_) => Some(WorkError::Unavailable),
        };
        for job in obsolete {
            job.diagnostic.event(Outcome::Cancelled);
            finish(&job.reply, Err(WorkError::Cancelled));
        }
        if let Some(error) = failure {
            diagnostic.event(match error {
                WorkError::Busy => Outcome::Busy,
                WorkError::Cancelled => Outcome::Cancelled,
                WorkError::Unavailable => Outcome::Unavailable,
            });
            finish(&reply, Err(error));
        }
        WorkFuture {
            diagnostic,
            completed: false,
            cancellation,
            reply,
        }
    }
}
impl<T> Drop for Workers<T> {
    fn drop(&mut self) {
        self.shutdown.cancel();
        let jobs = {
            let mut state = self
                .queue
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state.stopped = true;
            state.jobs.drain(..).collect::<Vec<_>>()
        };
        self.queue.wake.notify_all();
        for job in jobs {
            job.diagnostic.event(Outcome::Cancelled);
            finish(&job.reply, Err(WorkError::Cancelled));
        }
        for thread in self.threads.drain(..) {
            if thread.join().is_err() {
                eprintln!("File-detail worker shutdown failed. Collect application error output.");
            }
        }
    }
}
pub(crate) struct WorkFuture<T> {
    diagnostic: Work,
    completed: bool,
    cancellation: Cancellation,
    reply: Arc<Mutex<Reply<T>>>,
}
impl<T> WorkFuture<T> {
    pub fn cancellation(&self) -> Cancellation {
        self.cancellation.clone()
    }
}
impl<T> Future for WorkFuture<T> {
    type Output = Result<T, WorkError>;
    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if this.cancellation.is_cancelled() {
            this.completed = true;
            return Poll::Ready(Err(WorkError::Cancelled));
        }
        let Ok(mut state) = this.reply.lock() else {
            this.completed = true;
            return Poll::Ready(Err(WorkError::Unavailable));
        };
        if let Some(result) = state.result.take() {
            this.completed = true;
            Poll::Ready(result)
        } else {
            state.waker = Some(context.waker().clone());
            Poll::Pending
        }
    }
}
impl<T> Drop for WorkFuture<T> {
    fn drop(&mut self) {
        if !self.completed {
            self.diagnostic.event(Outcome::CancelRequested);
        }
        self.cancellation.cancel();
    }
}
fn finish<T>(reply: &Mutex<Reply<T>>, result: Result<T, WorkError>) {
    let waker = {
        let mut reply = reply.lock().unwrap_or_else(|error| error.into_inner());
        reply.result = Some(result);
        reply.waker.take()
    };
    if let Some(waker) = waker {
        waker.wake();
    }
}
fn run<T>(queue: Arc<Queue<T>>) {
    loop {
        let job = {
            let Ok(mut state) = queue.state.lock() else {
                return;
            };
            while state.jobs.is_empty() && !state.stopped {
                let Ok(next) = queue.wake.wait(state) else {
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
            let result = if job.cancellation.is_cancelled() {
                Err(WorkError::Cancelled)
            } else {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    (job.run)(&job.cancellation)
                }))
                .map_err(|_| WorkError::Unavailable)
            };
            job.diagnostic.finish(
                match &result {
                    Err(WorkError::Cancelled) => Outcome::Cancelled,
                    Err(_) => Outcome::Unavailable,
                    Ok(_) => Outcome::Finished,
                },
                started,
            );
            finish(&job.reply, result);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filesystem::tests::wait;
    #[test]
    fn bounded_admission_cancels_queued_work_and_shutdown_resolves_unstarted_futures()
    -> Result<(), Box<dyn std::error::Error>> {
        let workers = Workers::new("fixture", 1, 1)?;
        let (entered, observed) = std::sync::mpsc::channel();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let blocked = Arc::clone(&gate);
        let first = workers.submit(move |_| {
            let _ = entered.send(());
            let (state, wake) = &*blocked;
            let mut released = state.lock().unwrap_or_else(|error| error.into_inner());
            while !*released {
                released = wake
                    .wait(released)
                    .unwrap_or_else(|error| error.into_inner());
            }
            1
        });
        struct Release(Arc<(Mutex<bool>, Condvar)>);
        impl Drop for Release {
            fn drop(&mut self) {
                *self.0.0.lock().unwrap_or_else(|error| error.into_inner()) = true;
                self.0.1.notify_all();
            }
        }
        let release = Release(Arc::clone(&gate));
        observed.recv_timeout(std::time::Duration::from_secs(2))?;
        let obsolete = workers.submit(|_| 2);
        let rejected = workers.submit(|_| 3);
        assert_eq!(wait(rejected), Err(WorkError::Busy));
        drop(obsolete);
        let queued = workers.submit(|_| 4);
        drop(release);
        assert_eq!(wait(first), Ok(1));
        assert_eq!(wait(queued), Ok(4));
        let (entered, observed) = std::sync::mpsc::channel();
        let (exited, completed) = std::sync::mpsc::channel();
        let running = workers.submit(move |cancellation| {
            let _ = entered.send(());
            while !cancellation.is_cancelled() {
                std::thread::yield_now();
            }
            let _ = exited.send(());
            5
        });
        observed.recv_timeout(std::time::Duration::from_secs(2))?;
        let invoked = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let queued_invoked = Arc::clone(&invoked);
        let unstarted = workers.submit(move |_| {
            queued_invoked.store(true, std::sync::atomic::Ordering::Release);
            6
        });
        assert_eq!(
            workers
                .queue
                .state
                .lock()
                .map_err(|_| "Worker queue lock failed")?
                .jobs
                .len(),
            1,
            "The acknowledged running job must leave one admitted job queued"
        );
        drop(workers);
        completed.try_recv()?;
        assert_eq!(wait(running), Err(WorkError::Cancelled));
        assert_eq!(wait(unstarted), Err(WorkError::Cancelled));
        assert!(!invoked.load(std::sync::atomic::Ordering::Acquire));
        Ok(())
    }
}
