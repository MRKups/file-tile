//! Read-only native sizing. Bounded JSON output avoids parsing filenames as lines.
use serde::Deserialize;
use std::{
    io::Read,
    path::{Component, Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

const CACHE_LIMIT: usize = 512;
const OUTPUT_LIMIT: usize = 256 * 1024;
const TIME_LIMIT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawUsage {
    pub total_bytes: u64,
    pub total_partial: bool,
    pub children: Vec<(PathBuf, u64, bool)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageFailure {
    Cancelled,
    TimedOut,
    Unavailable,
    InvalidOutput,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct UsageErrors {
    pub access_denied: u32,
    pub not_found: u32,
    pub io: u32,
    pub other: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageIssue {
    TimedOut,
    Unavailable,
    OutputLimit,
    InvalidOutput,
    UtilityExit {
        code: Option<i32>,
        errors: UsageErrors,
    },
}

struct Du(Child);
impl Drop for Du {
    fn drop(&mut self) {
        if stop(&mut self.0).is_err() {
            eprintln!("Cannot reap folder-size utility. Collect application error output.");
        }
    }
}

fn stop(child: &mut Child) -> Result<(), UsageFailure> {
    let _ = child.kill();
    child
        .wait()
        .map(|_| ())
        .map_err(|_| UsageFailure::Unavailable)
}

fn wait(
    child: &mut Child,
    is_cancelled: &dyn Fn() -> bool,
    limit: Duration,
) -> Result<ExitStatus, UsageFailure> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(_) => {
                stop(child)?;
                return Err(UsageFailure::Unavailable);
            }
        }
        if is_cancelled() || started.elapsed() >= limit {
            stop(child)?;
            return Err(if is_cancelled() {
                UsageFailure::Cancelled
            } else {
                UsageFailure::TimedOut
            });
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

struct Captured {
    bytes: Vec<u8>,
    truncated: bool,
}

fn capture(mut input: impl Read) -> Result<Captured, UsageFailure> {
    let mut bytes = vec![];
    let mut truncated = false;
    let mut buffer = [0; 8192];
    loop {
        let count = input
            .read(&mut buffer)
            .map_err(|_| UsageFailure::Unavailable)?;
        if count == 0 {
            break;
        }
        let keep = count.min(OUTPUT_LIMIT.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&buffer[..keep]);
        truncated |= keep != count;
    }
    Ok(Captured { bytes, truncated })
}

// Classify only fixed English error suffixes. Keep at most 64 bytes per line,
// discard every path and raw message, and drain the pipe without an output buffer.
fn capture_errors(mut input: impl Read) -> Result<UsageErrors, UsageFailure> {
    let mut errors = UsageErrors::default();
    let mut tail = std::collections::VecDeque::with_capacity(64);
    let mut buffer = [0; 8192];
    loop {
        let count = input
            .read(&mut buffer)
            .map_err(|_| UsageFailure::Unavailable)?;
        for byte in &buffer[..count] {
            if *byte == b'\n' {
                classify_error(tail.make_contiguous(), &mut errors);
                tail.clear();
            } else {
                if tail.len() == 64 {
                    tail.pop_front();
                }
                tail.push_back(*byte);
            }
        }
        if count == 0 {
            if !tail.is_empty() {
                classify_error(tail.make_contiguous(), &mut errors);
            }
            return Ok(errors);
        }
    }
}

fn classify_error(tail: &[u8], errors: &mut UsageErrors) {
    let count =
        if tail.ends_with(b": Permission denied") || tail.ends_with(b": Operation not permitted") {
            &mut errors.access_denied
        } else if tail.ends_with(b": No such file or directory") {
            &mut errors.not_found
        } else if tail.ends_with(b": Input/output error") {
            &mut errors.io
        } else {
            &mut errors.other
        };
    *count = count.saturating_add(1);
}

pub fn measure_with_exclusions(
    directory: &Path,
    excluded: &[&str],
    is_cancelled: impl Fn() -> bool,
) -> (Result<RawUsage, UsageFailure>, Option<UsageIssue>, Duration) {
    let started = Instant::now();
    let mut issue = None;
    let result = measure_with_limit(directory, excluded, &is_cancelled, TIME_LIMIT, &mut issue);
    let fallback = match &result {
        Ok(report) if report.total_partial => Some(UsageIssue::InvalidOutput),
        Ok(_) | Err(UsageFailure::Cancelled) => None,
        Err(UsageFailure::TimedOut) => Some(UsageIssue::TimedOut),
        Err(UsageFailure::Unavailable) => Some(UsageIssue::Unavailable),
        Err(UsageFailure::InvalidOutput) => Some(UsageIssue::InvalidOutput),
    };
    (result, issue.or(fallback), started.elapsed())
}

fn measure_with_limit(
    directory: &Path,
    excluded: &[&str],
    is_cancelled: &dyn Fn() -> bool,
    limit: Duration,
    issue: &mut Option<UsageIssue>,
) -> Result<RawUsage, UsageFailure> {
    if is_cancelled() {
        return Err(UsageFailure::Cancelled);
    }
    if !directory.is_absolute() {
        return Err(UsageFailure::Unavailable);
    }
    let mut command = Command::new("/usr/bin/du");
    command.args(["--libxo", "json,flush", "-A", "-k", "-P", "-x", "-d", "1"]);
    for mask in excluded {
        command.args(["-I", mask]);
    }
    let mut child = Du(command
        .arg(".")
        .env("LC_ALL", "C")
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| UsageFailure::Unavailable)?);
    let stdout = child.0.stdout.take().ok_or(UsageFailure::Unavailable)?;
    let stderr = child.0.stderr.take().ok_or(UsageFailure::Unavailable)?;
    let reader = std::thread::Builder::new()
        .name("file-tile-usage-output".into())
        .spawn(move || capture(stdout))
        .map_err(|_| UsageFailure::Unavailable)?;
    let error_reader = std::thread::Builder::new()
        .name("file-tile-usage-errors".into())
        .spawn(move || capture_errors(stderr))
        .map_err(|_| UsageFailure::Unavailable)?;
    let status = wait(&mut child.0, is_cancelled, limit);
    drop(child);
    let captured = reader.join().map_err(|_| UsageFailure::Unavailable)??;
    let errors = error_reader
        .join()
        .map_err(|_| UsageFailure::Unavailable)??;
    if is_cancelled() {
        return Err(UsageFailure::Cancelled);
    }
    let complete = status.as_ref().is_ok_and(|status| status.success()) && !captured.truncated;
    if !complete {
        *issue = Some(match &status {
            Err(UsageFailure::TimedOut) => UsageIssue::TimedOut,
            Err(_) => UsageIssue::Unavailable,
            Ok(_) if captured.truncated => UsageIssue::OutputLimit,
            Ok(status) => UsageIssue::UtilityExit {
                code: status.code(),
                errors,
            },
        });
    }
    parse_output(&captured.bytes, directory, complete).map_err(|error| {
        if status == Err(UsageFailure::TimedOut) {
            UsageFailure::TimedOut
        } else {
            error
        }
    })
}

#[derive(Deserialize)]
struct Record {
    blocks: u64,
    path: PathBuf,
}

fn array_start(bytes: &[u8]) -> Option<&[u8]> {
    let key = b"\"paths\"";
    let index = bytes.windows(key.len()).position(|part| part == key)?;
    let mut bytes = &bytes[index + key.len()..];
    for expected in *b":[" {
        bytes = bytes.trim_ascii_start();
        if bytes.first() != Some(&expected) {
            return None;
        }
        bytes = &bytes[1..];
    }
    Some(bytes)
}

fn parse_output(bytes: &[u8], directory: &Path, complete: bool) -> Result<RawUsage, UsageFailure> {
    let mut bytes = array_start(bytes).ok_or(UsageFailure::InvalidOutput)?;
    let mut root = None;
    let mut subtotal = 0u64;
    let mut seen = std::collections::HashSet::new();
    let mut children = vec![];
    loop {
        bytes = bytes.trim_ascii_start();
        let mut stream = serde_json::Deserializer::from_slice(bytes).into_iter::<Record>();
        let Some(Ok(record)) = stream.next() else {
            break;
        };
        let value = record
            .blocks
            .checked_mul(1024)
            .ok_or(UsageFailure::InvalidOutput)?;
        if record.path == Path::new(".") {
            root = Some(value);
        } else {
            let mut parts = record.path.components();
            if let (Some(Component::CurDir), Some(Component::Normal(name)), None) =
                (parts.next(), parts.next(), parts.next())
                && seen.insert(record.path.clone())
            {
                subtotal = subtotal
                    .checked_add(value)
                    .ok_or(UsageFailure::InvalidOutput)?;
                if children.len() < CACHE_LIMIT {
                    children.push((directory.join(name), value, !complete));
                }
            }
        }
        bytes = bytes[stream.byte_offset()..].trim_ascii_start();
        if bytes.first() != Some(&b',') {
            break;
        }
        bytes = &bytes[1..];
    }
    let (total_bytes, total_partial) = match root {
        Some(bytes) => (bytes, !complete),
        None if !seen.is_empty() => (subtotal, true),
        None => return Err(UsageFailure::InvalidOutput),
    };
    Ok(RawUsage {
        total_bytes,
        total_partial,
        children,
    })
}
