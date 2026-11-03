//! Explicit default-handler handoff through the OS utility, without a shell.
//! Cancellation before spawn prevents handoff; it cannot recall an already launched handler.
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenDiag {
    Cancelled,
    TimedOut,
    Io(std::io::ErrorKind),
    FileFailure(&'static str),
}

pub struct OpenError {
    pub message: String,
    pub diagnostic: OpenDiag,
}

impl OpenError {
    pub fn safe(message: String, explanation: &'static str) -> Self {
        Self {
            message,
            diagnostic: OpenDiag::FileFailure(explanation),
        }
    }

    pub(super) fn io(error: &std::io::Error, message: String) -> Self {
        Self {
            message,
            diagnostic: OpenDiag::Io(error.kind()),
        }
    }
}

impl From<&'static str> for OpenError {
    fn from(message: &'static str) -> Self {
        Self::safe(message.into(), message)
    }
}

pub fn open_file(path: &Path, is_cancelled: impl Fn() -> bool) -> Result<(), OpenError> {
    if is_cancelled() {
        return Err(OpenError {
            message: "External opening cancelled before handoff.".into(),
            diagnostic: OpenDiag::Cancelled,
        });
    }
    if !path.is_absolute()
        || !std::fs::metadata(path)
            .map_err(|error| {
                OpenError::io(
                    &error,
                    format!("Cannot open file externally: {error}. Check the target and retry."),
                )
            })?
            .is_file()
    {
        return Err(
            "Cannot open this entry externally: only regular files are supported. Choose a file."
                .into(),
        );
    }
    handoff(path.as_os_str(), &is_cancelled)
}

pub fn open_directory(path: &Path, is_cancelled: impl Fn() -> bool) -> Result<(), OpenError> {
    if is_cancelled() {
        return Err(OpenError {
            message: "External opening cancelled before handoff.".into(),
            diagnostic: OpenDiag::Cancelled,
        });
    }
    if !path.is_absolute()
        || !std::fs::symlink_metadata(path)
            .map_err(|error| {
                OpenError::io(
                    &error,
                    format!(
                        "Cannot open folder externally: {error}. Check the displayed path and retry."
                    ),
                )
            })?
            .is_dir()
    {
        return Err(OpenError::safe(
            "Cannot open diagnostic logs: the logs folder is missing or is not a directory. Check the displayed path before retrying.".into(),
            "The logs folder is missing or is not a directory. Check the Diagnostic Logs path, then retry.",
        ));
    }
    handoff(path.as_os_str(), &is_cancelled)
}

pub(super) fn handoff(
    target: &std::ffi::OsStr,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(), OpenError> {
    if is_cancelled() {
        return Err(OpenError {
            message: "External opening cancelled before handoff.".into(),
            diagnostic: OpenDiag::Cancelled,
        });
    }
    let child = std::process::Command::new("/usr/bin/open")
        .arg("--")
        .arg(target)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| {
            OpenError::io(&error, format!(
                "Cannot open the target through macOS: {error}. Check the target and its default handler, then retry."
            ))
        })?;
    let status = wait_for_handoff(child, is_cancelled, std::time::Duration::from_secs(30))?;
    if status.success() {
        Ok(())
    } else {
        Err(OpenError::safe(
            format!(
                "macOS rejected opening the target (status {status}). Check the target and its default handler, then retry."
            ),
            "macOS rejected the opening request. Check the target and its default application before retrying.",
        ))
    }
}

fn wait_for_handoff(
    mut child: std::process::Child,
    is_cancelled: &dyn Fn() -> bool,
    timeout: std::time::Duration,
) -> Result<std::process::ExitStatus, OpenError> {
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(error) => {
                let cleanup = stop_utility(&mut child)
                    .err()
                    .map(|error| error.message)
                    .unwrap_or_default();
                return Err(OpenError::io(
                    &error,
                    format!(
                        "Cannot observe macOS file handoff: {error}. {cleanup} Check the default handler and retry."
                    ),
                ));
            }
        }
        if is_cancelled() || started.elapsed() >= timeout {
            stop_utility(&mut child)?;
            return Err(OpenError {
                message: "Opening timed out or was cancelled. Check the default handler before retrying; an already launched app cannot be recalled.".into(),
                diagnostic: if is_cancelled() {
                    OpenDiag::Cancelled
                } else {
                    OpenDiag::TimedOut
                },
            });
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn stop_utility(child: &mut std::process::Child) -> Result<(), OpenError> {
    let killed = child.kill();
    match child.wait() {
        Ok(_) => Ok(()), // Reaped, including an exit racing with cancellation.
        Err(error) => Err(OpenError::io(
            &error,
            format!(
                "Cannot reap the file-opening utility: {error}; stop result: {killed:?}. Collect application error output; an already launched app cannot be recalled."
            ),
        )),
    }
}
