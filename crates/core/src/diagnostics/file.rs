//! One bounded rotating append sink, independent of the configuration writer.
use super::{Message, Recorder, format};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{atomic::Ordering, mpsc::Receiver},
    time::SystemTime,
};

const LIMIT: u64 = 1024 * 1024;
const FILES: usize = 5;

pub(super) fn run(
    root: &Path,
    receiver: Receiver<Message>,
    recorder: &Recorder,
    started: SystemTime,
) -> io::Result<()> {
    let mut sink = Sink::open(root)?;
    let session = uuid::Uuid::new_v4();
    sink.write(&format!(
        "{} INFO  session - Application started. pid={} session={session} version={}\n",
        format::timestamp(started),
        std::process::id(),
        env!("CARGO_PKG_VERSION")
    ))?;
    recorder.status.store(2, Ordering::Relaxed);
    let mut dropped = 0;
    let mut healthy_shutdown = false;
    while let Ok(message) = receiver.recv() {
        report_dropped(&mut sink, recorder, &mut dropped, session)?;
        match message {
            Message::Event(event) => {
                if let Some(line) = event.line() {
                    sink.write(&format!(
                        "{line} session={session} version={}\n",
                        env!("CARGO_PKG_VERSION")
                    ))?;
                }
            }
            Message::Flush(reply) => {
                let result = sink.file.sync_data();
                let _ = reply.try_send(result.is_ok());
                result?;
            }
            Message::HealthyShutdown => {
                healthy_shutdown = true;
                break;
            }
        }
    }
    report_dropped(&mut sink, recorder, &mut dropped, session)?;
    if healthy_shutdown {
        sink.file.sync_all()?;
        sink.write(&format!("{} INFO  session - Application shut down cleanly. pid={} session={session} version={}\n",
            format::timestamp(SystemTime::now()), std::process::id(), env!("CARGO_PKG_VERSION")))?;
    }
    sink.file.sync_all()
}

fn report_dropped(
    sink: &mut Sink,
    recorder: &Recorder,
    previous: &mut u64,
    session: uuid::Uuid,
) -> io::Result<()> {
    let total = recorder.dropped.load(Ordering::Relaxed);
    if total != *previous {
        sink.write(&format!("{} WARN  diagnostics - Diagnostic records were lost because logging could not keep up. This session's log is incomplete. dropped={} total_dropped={} pid={} session={session} version={}\n",
            format::timestamp(SystemTime::now()), total.saturating_sub(*previous), total, std::process::id(), env!("CARGO_PKG_VERSION")))?;
        *previous = total;
    }
    Ok(())
}

struct Sink {
    root: PathBuf,
    directory: PathBuf,
    file: File,
    _lock: File,
    bytes: u64,
    needs_separator: bool,
}
impl Sink {
    fn open(root: &Path) -> io::Result<Self> {
        if !root.is_absolute() {
            return Err(unsafe_path());
        }
        directory(root)?;
        let logs = root.join("logs");
        directory(&logs)?;
        let lock = append(&logs.join(".file-tile-writer.lock"), false)?;
        lock.try_lock().map_err(|e| match e {
            fs::TryLockError::WouldBlock => io::Error::new(
                io::ErrorKind::WouldBlock,
                "Another diagnostic writer owns this root",
            ),
            fs::TryLockError::Error(e) => e,
        })?;
        let mut file = append(&logs.join("file-tile.log"), false)?;
        let bytes = file.metadata()?.len();
        if bytes > LIMIT {
            return Err(unsafe_path());
        }
        let needs_separator = if bytes > 0 {
            file.seek(SeekFrom::End(-1))?;
            let mut last = [0];
            file.read_exact(&mut last)?;
            last[0] != b'\n'
        } else {
            false
        };
        Ok(Self {
            root: root.into(),
            directory: logs,
            file,
            _lock: lock,
            bytes,
            needs_separator,
        })
    }
    fn path(&self, index: usize) -> PathBuf {
        self.directory.join(if index == 0 {
            "file-tile.log".into()
        } else {
            format!("file-tile.{index}.log")
        })
    }
    fn write(&mut self, line: &str) -> io::Result<()> {
        if line.len() as u64 > LIMIT {
            return Err(unsafe_path());
        }
        self.check()?;
        if self.bytes + line.len() as u64 + u64::from(self.needs_separator) > LIMIT {
            self.rotate()?;
        }
        // Keep a new record separate from a crash-interrupted tail.
        if self.needs_separator {
            self.file.write_all(b"\n")?;
            self.bytes += 1;
            self.needs_separator = false;
        }
        self.file.write_all(line.as_bytes())?;
        self.bytes += line.len() as u64;
        Ok(())
    }
    fn check(&self) -> io::Result<()> {
        for directory in [&self.root, &self.directory] {
            if !fs::symlink_metadata(directory)?.is_dir() {
                return Err(unsafe_path());
            }
        }
        same_file(&self.path(0), &self.file)?;
        same_file(&self.directory.join(".file-tile-writer.lock"), &self._lock)
    }
    fn rotate(&mut self) -> io::Result<()> {
        // Validate every managed destination before removing or renaming any.
        for index in 0..FILES {
            match fs::symlink_metadata(self.path(index)) {
                Ok(meta) => {
                    regular(&meta)?;
                    if meta.len() > LIMIT {
                        return Err(unsafe_path());
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        self.file.sync_data()?;
        for index in (1..FILES).rev() {
            let from = self.path(index - 1);
            match fs::rename(&from, self.path(index)) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        self.file = append(&self.path(0), true)?;
        self.bytes = 0;
        self.needs_separator = false;
        File::open(&self.directory)?.sync_all()
    }
}

fn directory(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    builder.mode(0o700);
    builder.create(path)?;
    if !fs::symlink_metadata(path)?.is_dir() {
        return Err(unsafe_path());
    }
    Ok(())
}
fn append(path: &Path, exclusive: bool) -> io::Result<File> {
    match fs::symlink_metadata(path) {
        Ok(meta) => regular(&meta)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let mut options = OpenOptions::new();
    options.append(true).read(true);
    #[cfg(unix)]
    options.mode(0o600);
    #[cfg(target_os = "macos")]
    options.custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
    let file = if exclusive {
        options.create_new(true).open(path)?
    } else {
        options.create(true).open(path)?
    };
    same_file(path, &file)?;
    Ok(file)
}
fn same_file(path: &Path, file: &File) -> io::Result<()> {
    let path = fs::symlink_metadata(path)?;
    let opened = file.metadata()?;
    regular(&path)?;
    regular(&opened)?;
    #[cfg(unix)]
    if path.dev() != opened.dev() || path.ino() != opened.ino() {
        return Err(unsafe_path());
    }
    Ok(())
}
fn regular(meta: &fs::Metadata) -> io::Result<()> {
    if !meta.is_file() {
        return Err(unsafe_path());
    }
    #[cfg(unix)]
    if meta.nlink() != 1 {
        return Err(unsafe_path());
    }
    Ok(())
}
fn unsafe_path() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Unsafe or changed diagnostic log target",
    )
}
