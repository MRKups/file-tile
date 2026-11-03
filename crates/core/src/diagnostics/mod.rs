//! Nonwaiting typed hooks; a single background owner formats and writes logs.
mod file;
mod format;
mod message;

use std::{
    io,
    path::Path,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU8, AtomicU64, Ordering},
        mpsc::{self, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

const CAPACITY: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subsystem {
    Session,
    Configuration,
    Listing,
    Metadata,
    Thumbnail,
    Sorting,
    Size,
    Watcher,
    Storage,
    Files,
    Opening,
}
/// Categories only. Never carry configuration values or user-supplied labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    AppSettings,
    BoxCreated,
    LauncherCreated,
    LauncherSettings,
    LauncherRemoved,
    LauncherLayout,
    BoxRemoved,
    BoxSettings,
    BoxFolder,
    BoxScope,
    BoxSort,
    BoxLayout,
    BoxTabs,
    WorkspaceCreated,
    WorkspaceSelected,
    WorkspaceRemoved,
    WorkspaceRenamed,
    WorkspaceUndo,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    ConfigurationChanged(Change),
    Load,
    Restore,
    Save,
    Manage,
    Validate,
    Work,
    Measure,
    SizeIssue(SizeIssue),
    OpenFile,
    OpenLauncher,
    OpenLogs,
    CheckWorkspaceReference,
    CacheMiss,
    CacheReuse,
    CacheExpired,
    Evict,
    Register,
    Overflow,
    ReloadManual,
    ReloadWatch,
    ReloadWrite,
    Listing,
    Receipt,
    Close,
    CaptureWindow,
    Flush,
    Copy,
    Move,
    Create,
    Rename,
    Trash,
    ConflictChoice,
    Stop,
    Navigate,
    Activate,
    Configure,
    TemporarySynced,
    BackupPublished,
    PrimaryPublished,
    WorkspaceInitialized,
    IndexInitialized,
    ManagementWorkspace,
    ManagementIndex,
    ManagementPreferences,
    Panic,
    Shutdown,
}
/// Safe causes only. Utility output and paths never enter a diagnostic event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeIssue {
    TimedOut,
    OutputLimit,
    UtilityExit {
        code: Option<i32>,
        errors: SizeErrors,
    },
    InvalidOutput,
    Unavailable,
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SizeErrors {
    pub(crate) access_denied: u64,
    pub(crate) not_found: u64,
    pub(crate) io: u64,
    pub(crate) other: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Requested,
    Queued,
    Started,
    Complete,
    Partial,
    Failed,
    CancelRequested,
    Cancelled,
    Busy,
    Unavailable,
    TimedOut,
    InvalidOutput,
    Stale,
    RecoveryOnly,
    Superseded,
    UnsafePath,
    Io(io::ErrorKind),
    /// Fixed application-written explanation, never a filename or raw error.
    FileFailure(&'static str),
    InvalidConfiguration,
    ExternalChange,
    ReadOnly,
    BackupAvailable,
    Conflict,
    KeepBoth,
    Skip,
    WaitingFiles,
    WaitingTransition,
    WaitingSave,
    OutputLimit,
    NativeFailure,
    NotFound,
    NotDirectory,
    PermissionDenied,
    OutsideScope,
    MissingOwner,
    GenerationMismatch,
    WorkspaceMismatch,
    Backoff,
    Finished,
}

#[derive(Debug, Clone)]
struct Event {
    pub(super) timestamp: SystemTime,
    pub(super) elapsed: Duration,
    pub(super) subsystem: Subsystem,
    pub(super) activity: Activity,
    pub(super) outcome: Outcome,
    pub(super) request: u64,
    pub(super) detail: u64,
    pub(super) duration: Option<Duration>,
}
enum Message {
    Event(Event),
    Flush(SyncSender<bool>),
    HealthyShutdown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Disabled,
    Starting,
    Ready,
    Failed,
    Stopped,
}
pub struct Recorder {
    started: Instant,
    sender: Mutex<Option<SyncSender<Message>>>,
    status: AtomicU8,
    dropped: AtomicU64,
    next_id: AtomicU64,
}
impl Default for Recorder {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            sender: Mutex::new(None),
            status: AtomicU8::new(0),
            dropped: AtomicU64::new(0),
            next_id: AtomicU64::new(1),
        }
    }
}
impl Recorder {
    /// File creation and all disk work happen on the returned owner's thread.
    pub fn start(self: &Arc<Self>, root: &Path) -> io::Result<Writer> {
        let mut sender = self
            .sender
            .lock()
            .map_err(|_| io::Error::other("Diagnostic recorder unavailable"))?;
        if self.status.load(Ordering::Relaxed) != 0 {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Diagnostic writer already started",
            ));
        }
        let (tx, rx) = mpsc::sync_channel(CAPACITY);
        let recorder = Arc::clone(self);
        let root = root.to_path_buf();
        let started = SystemTime::now();
        self.status.store(1, Ordering::Relaxed);
        let thread = thread::Builder::new()
            .name("diagnostic-log".into())
            .spawn(move || {
                 let result = file::run(&root, rx, &recorder, started);
            if let Err(error) = result {
                recorder.status.store(3, Ordering::Relaxed);
                eprintln!("Cannot write diagnostic logs ({:?}). Check the Diagnostic Logs path and available disk space, then relaunch. Application outcomes are unchanged.", error.kind());
            } else {
                recorder.status.store(4, Ordering::Relaxed);
            }
        });
        match thread {
            Ok(thread) => {
                *sender = Some(tx);
                Ok(Writer {
                    recorder: Arc::clone(self),
                    thread: Some(thread),
                    healthy_shutdown: false,
                })
            }
            Err(error) => {
                self.status.store(3, Ordering::Relaxed);
                Err(error)
            }
        }
    }
    pub fn status(&self) -> Status {
        match self.status.load(Ordering::Relaxed) {
            0 => Status::Disabled,
            1 => Status::Starting,
            2 => Status::Ready,
            3 => Status::Failed,
            _ => Status::Stopped,
        }
    }
    pub(crate) fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }
    pub fn record(
        &self,
        subsystem: Subsystem,
        activity: Activity,
        outcome: Outcome,
        request: u64,
        detail: u64,
        duration: Option<Duration>,
    ) {
        if format::severity(subsystem, activity, outcome).is_none() {
            return;
        }
        let Ok(sender) = self.sender.try_lock() else {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        };
        let Some(sender) = sender.as_ref() else {
            return;
        };
        let event = Event {
            timestamp: SystemTime::now(),
            elapsed: self.started.elapsed(),
            subsystem,
            activity,
            outcome,
            request,
            detail,
            duration,
        };
        if sender.try_send(Message::Event(event)).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
    /// Used only by the panic hook, never by ordinary UI or application work.
    fn flush_timeout(&self, timeout: Duration) -> bool {
        let (tx, rx) = mpsc::sync_channel(1);
        let sent = self.sender.try_lock().ok().is_some_and(|sender| {
            sender
                .as_ref()
                .is_some_and(|s| s.try_send(Message::Flush(tx)).is_ok())
        });
        sent && rx.recv_timeout(timeout).is_ok_and(|ok| ok)
    }
}

pub struct Writer {
    recorder: Arc<Recorder>,
    thread: Option<JoinHandle<()>>,
    healthy_shutdown: bool,
}
impl Writer {
    /// Call after successful runtime return and worker teardown. Ordinary drop,
    /// including unwinding, drains records without claiming a healthy exit.
    pub fn finish_healthy(mut self) {
        self.healthy_shutdown = true;
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        // Runtime and worker owners have already exited. Disconnect and drain.
        {
            let mut sender = match self.recorder.sender.lock() {
                Ok(sender) => sender,
                Err(poisoned) => poisoned.into_inner(),
            };
            if let Some(sender) = sender.take()
                && self.healthy_shutdown
                && !thread::panicking()
                && sender.send(Message::HealthyShutdown).is_err()
            {
                eprintln!(
                    "Cannot record healthy shutdown: diagnostic writer is unavailable. Inspect the log file and application error output."
                );
            }
        }
        if self
            .thread
            .take()
            .is_some_and(|thread| thread.join().is_err())
        {
            self.recorder.status.store(3, Ordering::Relaxed);
            eprintln!(
                "Diagnostic log writer stopped unexpectedly. Inspect the log file and application error output."
            );
        }
    }
}

/// One recorder for the process, available before any storage owner is opened.
pub fn session() -> &'static Arc<Recorder> {
    static SESSION: OnceLock<Arc<Recorder>> = OnceLock::new();
    SESSION.get_or_init(|| Arc::new(Recorder::default()))
}

pub(crate) fn configuration_changed(change: Change) {
    session().record(
        Subsystem::Configuration,
        Activity::ConfigurationChanged(change),
        Outcome::Complete,
        0,
        0,
        None,
    );
}

pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        session().record(
            Subsystem::Session,
            Activity::Panic,
            Outcome::Failed,
            0,
            0,
            None,
        );
        if !session().flush_timeout(Duration::from_millis(200)) {
            eprintln!(
                "Cannot flush diagnostic panic marker. Collect application error output as well as the log file."
            );
        }
        previous(info);
    }));
}

/// Worker identity and timing for issue records. Routine lifecycle calls are
/// filtered before admission. IDs are unique across workspace switches.
#[derive(Clone)]
pub(crate) struct Work {
    pub id: u64,
    subsystem: Subsystem,
    submitted: Instant,
}
impl Work {
    pub fn new(subsystem: Subsystem) -> Self {
        Self {
            id: session().next_id(),
            subsystem,
            submitted: Instant::now(),
        }
    }
    pub fn event(&self, outcome: Outcome) {
        session().record(self.subsystem, Activity::Work, outcome, self.id, 0, None);
    }
    pub fn start(&self) -> Instant {
        session().record(
            self.subsystem,
            Activity::Work,
            Outcome::Started,
            self.id,
            0,
            Some(self.submitted.elapsed()),
        );
        Instant::now()
    }
    pub fn finish(&self, outcome: Outcome, started: Instant) {
        session().record(
            self.subsystem,
            Activity::Work,
            outcome,
            self.id,
            0,
            Some(started.elapsed()),
        );
    }
}
