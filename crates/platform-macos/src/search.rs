//! Bounded, cancelable macOS Spotlight filename search via `/usr/bin/mdfind`.

use std::ffi::OsStr;
use std::io::{self, Read};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const OUTPUT_LIMIT: usize = 512 * 1024;
const MAX_RAW_CANDIDATES: usize = 512;
const MAX_QUERY_BYTES: usize = 256;
const TIME_LIMIT: Duration = Duration::from_secs(4);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchFailure {
    Cancelled,
    TimedOut,
    InvalidQuery,
    Unavailable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSearchOutput {
    pub paths: Vec<PathBuf>,
    pub truncated: bool,
}

enum WaitOutcome {
    Exited(ExitStatus),
    Capped,
    Cancelled,
    TimedOut,
    Failed,
}

struct MdfindChild(Option<Child>);

impl MdfindChild {
    fn stop_now(&mut self) -> io::Result<()> {
        match self.0.as_mut() {
            Some(child) => stop(child),
            None => Ok(()),
        }
    }
}

impl Drop for MdfindChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take()
            && let Err(error) = stop(&mut child)
        {
            eprintln!("Cannot wait for cancelled mdfind child: {error}");
        }
    }
}

/// Builds a strictly escaped Spotlight filename predicate that never begins with `-`.
pub fn build_filename_predicate(query: &str) -> Result<String, SearchFailure> {
    let trimmed = query.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_QUERY_BYTES || trimmed.contains(['\0', '\r', '\n'])
    {
        return Err(SearchFailure::InvalidQuery);
    }
    let mut escaped = String::with_capacity(trimmed.len() + 8);
    for ch in trimmed.chars() {
        if matches!(ch, '\\' | '\'' | '*' | '?') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    Ok(format!("kMDItemFSName == '*{escaped}*'cd"))
}

/// Searches macOS Spotlight filenames via `/usr/bin/mdfind -0`, respecting `is_cancelled` and caps.
pub fn search_filenames(
    query: &str,
    is_cancelled: impl Fn() -> bool,
) -> Result<RawSearchOutput, SearchFailure> {
    if is_cancelled() {
        return Err(SearchFailure::Cancelled);
    }
    let predicate = build_filename_predicate(query)?;
    let mut child = Command::new("/usr/bin/mdfind")
        .arg("-0")
        .arg(&predicate)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| unavailable_error())?;

    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        let _ = stop(&mut child);
        return Err(unavailable_error());
    };
    let mut guard = MdfindChild(Some(child));
    let cap_reached = Arc::new(AtomicBool::new(false));
    let output_reader = spawn_output_reader(stdout, Arc::clone(&cap_reached));
    let error_drainer = spawn_error_drainer(stderr);
    let wait_outcome = wait(&mut guard, &is_cancelled, &cap_reached, TIME_LIMIT);
    drop(guard);

    let output = output_reader.join().ok().and_then(Result::ok);
    let _ = error_drainer.join();
    if is_cancelled() {
        return Err(SearchFailure::Cancelled);
    }
    finish_search(wait_outcome, output)
}

fn finish_search(
    wait_outcome: WaitOutcome,
    output: Option<(Vec<PathBuf>, bool)>,
) -> Result<RawSearchOutput, SearchFailure> {
    match wait_outcome {
        WaitOutcome::Cancelled => Err(SearchFailure::Cancelled),
        WaitOutcome::TimedOut => Err(SearchFailure::TimedOut),
        WaitOutcome::Failed => Err(unavailable_error()),
        WaitOutcome::Capped => {
            let (paths, truncated) = output.ok_or_else(unavailable_error)?;
            let capped = truncated || paths.len() >= MAX_RAW_CANDIDATES;
            Ok(RawSearchOutput {
                paths,
                truncated: capped,
            })
        }
        WaitOutcome::Exited(status) => {
            if !status.success() {
                return Err(unavailable_error());
            }
            let (paths, truncated) = output.ok_or_else(unavailable_error)?;
            Ok(RawSearchOutput { paths, truncated })
        }
    }
}

fn wait(
    guard: &mut MdfindChild,
    is_cancelled: &dyn Fn() -> bool,
    cap_reached: &AtomicBool,
    limit: Duration,
) -> WaitOutcome {
    let started = Instant::now();
    loop {
        if is_cancelled() {
            return stop_or_fail(guard, WaitOutcome::Cancelled);
        }
        if cap_reached.load(Ordering::Acquire) {
            return stop_or_fail(guard, WaitOutcome::Capped);
        }
        let Some(child) = guard.0.as_mut() else {
            return WaitOutcome::Failed;
        };
        match child.try_wait() {
            Ok(Some(status)) => {
                guard.0 = None;
                if cap_reached.load(Ordering::Acquire) {
                    return WaitOutcome::Capped;
                }
                return WaitOutcome::Exited(status);
            }
            Ok(None) if started.elapsed() >= limit => {
                return stop_or_fail(guard, WaitOutcome::TimedOut);
            }
            Ok(None) => thread::sleep(POLL_INTERVAL),
            Err(_) => {
                let _ = guard.stop_now();
                guard.0 = None;
                return WaitOutcome::Failed;
            }
        }
    }
}

fn stop_or_fail(guard: &mut MdfindChild, outcome: WaitOutcome) -> WaitOutcome {
    let result = guard.stop_now();
    guard.0 = None;
    match result {
        Ok(()) => outcome,
        Err(_) => WaitOutcome::Failed,
    }
}

fn stop(child: &mut Child) -> io::Result<()> {
    let _ = child.kill();
    child.wait().map(|_| ())
}

fn spawn_output_reader(
    stdout: ChildStdout,
    cap_reached: Arc<AtomicBool>,
) -> JoinHandle<io::Result<(Vec<PathBuf>, bool)>> {
    thread::Builder::new()
        .name("file-tile-search-output".into())
        .spawn(move || capture_paths(stdout, &cap_reached))
        .unwrap_or_else(|error| thread::spawn(move || Err(error)))
}

fn capture_paths(
    mut stdout: ChildStdout,
    cap_reached: &AtomicBool,
) -> io::Result<(Vec<PathBuf>, bool)> {
    let mut paths = Vec::new();
    let mut pending = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut total_bytes = 0usize;
    let mut truncated = false;

    loop {
        let count = stdout.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        let remaining = OUTPUT_LIMIT.saturating_sub(total_bytes);
        let take = count.min(remaining);
        total_bytes = total_bytes.saturating_add(take);
        let hit_byte_limit = take < count;
        let hit_candidate_limit = consume_nul_paths(&chunk[..take], &mut pending, &mut paths);
        if hit_byte_limit || hit_candidate_limit {
            truncated = true;
            cap_reached.store(true, Ordering::Release);
            drop(stdout);
            return Ok((paths, truncated));
        }
    }
    if !pending.is_empty() && !truncated && paths.len() < MAX_RAW_CANDIDATES {
        paths.push(PathBuf::from(OsStr::from_bytes(&pending)));
    }
    Ok((paths, truncated))
}

fn consume_nul_paths(slice: &[u8], pending: &mut Vec<u8>, paths: &mut Vec<PathBuf>) -> bool {
    let mut start = 0usize;
    for (index, &byte) in slice.iter().enumerate() {
        if byte == 0 {
            if start < index || !pending.is_empty() {
                pending.extend_from_slice(&slice[start..index]);
                if !pending.is_empty() {
                    if paths.len() >= MAX_RAW_CANDIDATES {
                        pending.clear();
                        return true;
                    }
                    paths.push(PathBuf::from(OsStr::from_bytes(pending)));
                    pending.clear();
                }
            }
            start = index + 1;
        }
    }
    if start < slice.len() {
        if pending.len().saturating_add(slice.len() - start) > 16 * 1024 {
            pending.clear();
            return true;
        }
        pending.extend_from_slice(&slice[start..]);
    }
    false
}

fn spawn_error_drainer(stderr: ChildStderr) -> JoinHandle<()> {
    thread::Builder::new()
        .name("file-tile-search-errors".into())
        .spawn(move || drain_stderr(stderr))
        .unwrap_or_else(|_| thread::spawn(|| ()))
}

fn drain_stderr(mut stderr: ChildStderr) {
    let mut chunk = [0u8; 4096];
    let mut drained = 0usize;
    while drained < OUTPUT_LIMIT {
        let Ok(count) = stderr.read(&mut chunk) else {
            break;
        };
        if count == 0 {
            break;
        }
        drained = drained.saturating_add(count);
    }
}

fn unavailable_error() -> SearchFailure {
    SearchFailure::Unavailable("Spotlight search is unavailable right now.".into())
}
