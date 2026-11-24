//! Default-app requests have their own bounded worker, not the image-read queue.
use super::work::{WorkFuture, Workers};
use crate::application::contracts::directory::{Cancellation, EntryKind};
use crate::application::files::opening::{OpenCompletion, OpenRequest};
use crate::application::files::search::{SearchOpenCompletion, SearchOpenRequest};
use crate::diagnostics::Outcome;
use crate::domain::LauncherTarget;
use crate::platform::{OpenDiag, OpenError};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

pub struct OpenWorkers {
    workers: Workers<Result<(), String>>,
}

fn map_diag(diag: OpenDiag) -> Outcome {
    match diag {
        OpenDiag::Cancelled => Outcome::Cancelled,
        OpenDiag::TimedOut => Outcome::TimedOut,
        OpenDiag::Io(kind) => Outcome::Io(kind),
        OpenDiag::FileFailure(reason) => Outcome::FileFailure(reason),
    }
}

fn open_launcher_target(
    target: &LauncherTarget,
    cancellation: &Cancellation,
) -> Result<(), OpenError> {
    target
        .validate()
        .map_err(|msg| OpenError::safe(msg.into(), msg))?;
    match target {
        LauncherTarget::Path(path) => {
            crate::platform::launchers::open_path(path, || cancellation.is_cancelled())
        }
        LauncherTarget::Link(link) => {
            crate::platform::launchers::open_link(link, || cancellation.is_cancelled())
        }
    }
}

impl OpenWorkers {
    pub fn open_launcher(
        &self,
        request: crate::application::workspace::launchers::Request,
    ) -> LauncherFuture {
        let id = crate::diagnostics::session().next_id();
        let future = self.launcher_work(id, request.target.clone());
        LauncherFuture {
            request,
            future,
            id,
        }
    }

    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            workers: Workers::new("opening", 1, 1)?,
        })
    }

    pub fn open(&self, request: OpenRequest) -> OpenFuture {
        let future = self.path_work(
            request.path.clone(),
            false,
            crate::diagnostics::Activity::OpenFile,
            None,
        );
        OpenFuture { request, future }
    }

    pub fn open_search_result(&self, request: SearchOpenRequest) -> SearchOpenFuture {
        let is_directory = request.kind == EntryKind::Directory;
        let safety = request.safety.clone();
        let future = self.path_work(
            request.path.clone(),
            is_directory,
            crate::diagnostics::Activity::OpenFile,
            Some(safety),
        );
        SearchOpenFuture { request, future }
    }

    /// App-owned support folders do not belong to a workspace view.
    pub fn open_logs_folder(
        &self,
        path: std::path::PathBuf,
    ) -> impl Future<Output = Result<(), String>> + use<> {
        let future = self.path_work(path, true, crate::diagnostics::Activity::OpenLogs, None);
        async move { handoff_result(future.await) }
    }

    /// App-owned web links do not belong to a workspace view or shortcut tile.
    pub fn open_url(&self, url: &'static str) -> impl Future<Output = Result<(), String>> + use<> {
        let id = crate::diagnostics::session().next_id();
        let future = self.launcher_work(id, LauncherTarget::Link(url.into()));
        async move { handoff_result(future.await) }
    }

    fn launcher_work(&self, id: u64, target: LauncherTarget) -> WorkFuture<Result<(), String>> {
        self.workers.submit(move |cancellation| {
            let started = std::time::Instant::now();
            let result = open_launcher_target(&target, cancellation);
            crate::diagnostics::session().record(
                crate::diagnostics::Subsystem::Opening,
                crate::diagnostics::Activity::OpenLauncher,
                result
                    .as_ref()
                    .map_or_else(|error| map_diag(error.diagnostic), |()| Outcome::Complete),
                id,
                0,
                Some(started.elapsed()),
            );
            result.map_err(|error| error.message)
        })
    }

    fn path_work(
        &self,
        path: std::path::PathBuf,
        directory: bool,
        activity: crate::diagnostics::Activity,
        safety: Option<crate::domain::SafetyPolicy>,
    ) -> WorkFuture<Result<(), String>> {
        let id = crate::diagnostics::session().next_id();
        self.workers.submit(move |cancellation| {
            use crate::diagnostics::{Activity, Subsystem, session};
            let started = std::time::Instant::now();
            session().record(
                Subsystem::Opening,
                Activity::Work,
                Outcome::Requested,
                id,
                0,
                None,
            );
            let safety_check = safety
                .as_ref()
                .map_or(Ok(()), |policy| check_search_open_safety(&path, policy));
            let result = match safety_check {
                Ok(()) if directory => {
                    crate::platform::open_directory(&path, || cancellation.is_cancelled())
                }
                Ok(()) => crate::platform::open_file(&path, || cancellation.is_cancelled()),
                Err(reason) => Err(OpenError::safe(
                    reason,
                    "Blocked by Access and Safety settings.",
                )),
            };
            session().record(
                Subsystem::Opening,
                activity,
                result
                    .as_ref()
                    .map_or_else(|error| map_diag(error.diagnostic), |()| Outcome::Complete),
                id,
                0,
                Some(started.elapsed()),
            );
            result.map_err(|error| error.message)
        })
    }
}

fn check_search_open_safety(
    path: &std::path::Path,
    safety: &crate::domain::SafetyPolicy,
) -> Result<(), String> {
    let verify = |target: &std::path::Path| -> Result<(), &'static str> {
        safety.check_read_directory(target)?;
        if let Some(parent) = target.parent() {
            safety.check_read_directory(parent)?;
        }
        Ok(())
    };
    verify(path).map_err(str::to_owned)?;
    let meta = std::fs::symlink_metadata(path)
        .map_err(|error| format!("Cannot inspect item: {error}. Check the item and retry."))?;
    let resolved = if meta.file_type().is_symlink() {
        std::fs::canonicalize(path).map_err(|error| {
            format!("Cannot resolve target path: {error}. Check the item and retry.")
        })?
    } else if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
        std::fs::canonicalize(parent)
            .map_err(|error| {
                format!("Cannot resolve parent folder: {error}. Check the item and retry.")
            })?
            .join(name)
    } else {
        path.to_path_buf()
    };
    verify(&resolved).map_err(str::to_owned)
}

pub struct LauncherFuture {
    id: u64,
    request: crate::application::workspace::launchers::Request,
    future: WorkFuture<Result<(), String>>,
}

impl LauncherFuture {
    pub fn cancellation(&self) -> Cancellation {
        self.future.cancellation()
    }
}

impl Future for LauncherFuture {
    type Output = crate::application::workspace::launchers::Completion;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        Pin::new(&mut this.future).poll(cx).map(|result| {
            if let Err(error) = &result {
                use crate::diagnostics::{Activity, Subsystem, session};
                let outcome = match error {
                    super::work::WorkError::Busy => Outcome::Busy,
                    super::work::WorkError::Cancelled => Outcome::Cancelled,
                    super::work::WorkError::Unavailable => Outcome::Unavailable,
                };
                // Jobs rejected before native execution have no worker-owned failure record.
                session().record(
                    Subsystem::Opening,
                    Activity::OpenLauncher,
                    outcome,
                    this.id,
                    0,
                    None,
                );
            }
            crate::application::workspace::launchers::Completion {
                request: this.request.clone(),
                result: handoff_result(result),
            }
        })
    }
}

pub struct OpenFuture {
    request: OpenRequest,
    future: WorkFuture<Result<(), String>>,
}

impl OpenFuture {
    pub fn cancellation(&self) -> Cancellation {
        self.future.cancellation()
    }
}

impl Future for OpenFuture {
    type Output = OpenCompletion;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        Pin::new(&mut this.future)
            .poll(cx)
            .map(|result| OpenCompletion {
                request: this.request.clone(),
                result: handoff_result(result),
            })
    }
}

pub struct SearchOpenFuture {
    request: SearchOpenRequest,
    future: WorkFuture<Result<(), String>>,
}

impl SearchOpenFuture {
    pub fn cancellation(&self) -> Cancellation {
        self.future.cancellation()
    }
}

impl Future for SearchOpenFuture {
    type Output = SearchOpenCompletion;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        Pin::new(&mut this.future)
            .poll(cx)
            .map(|result| SearchOpenCompletion {
                request: this.request.clone(),
                result: handoff_result(result),
            })
    }
}

fn handoff_result(
    result: Result<Result<(), String>, super::work::WorkError>,
) -> Result<(), String> {
    result.map_err(|error| match error {
        super::work::WorkError::Busy => "Cannot open the target: another opening is still running. Wait for it to finish, then retry.".into(),
        super::work::WorkError::Cancelled => "Opening was cancelled. Retry if the target is still needed.".into(),
        super::work::WorkError::Unavailable => "Cannot open the target: the opening service is unavailable. Relaunch and retry.".into(),
    }).and_then(|result| result)
}
