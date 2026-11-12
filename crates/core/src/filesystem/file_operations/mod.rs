//! Synchronous native writes behind a dedicated bounded worker and terminal receipt.
use super::work::{WorkFuture, Workers};
use crate::application::files::file_operations::{Completion, Identity, Item, Job, Outcome};
use crate::diagnostics::{Activity, Outcome as DiagnosticOutcome, Subsystem, session};
use crate::domain::files::validate_name;
use std::{
    future::Future,
    io::Write,
    path::Path,
    pin::Pin,
    task::{Context, Poll},
};

// Preserve the existing UI explanation separately from safe diagnostic evidence.
// Only compile-time messages and ErrorKind can enter the log, never raw errors.
struct Failure {
    message: String,
    diagnostic: DiagnosticOutcome,
}
impl Failure {
    fn io(error: &std::io::Error, message: String) -> Self {
        Self {
            message,
            diagnostic: DiagnosticOutcome::Io(error.kind()),
        }
    }
    fn safe(message: String, explanation: &'static str) -> Self {
        Self {
            message,
            diagnostic: DiagnosticOutcome::FileFailure(explanation),
        }
    }
}
impl From<&'static str> for Failure {
    fn from(message: &'static str) -> Self {
        Self::safe(message.into(), message)
    }
}

pub struct FileOperationWorker {
    workers: Workers<Completion>,
}
impl FileOperationWorker {
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            workers: Workers::new("file-operations", 1, 1)?,
        })
    }
    pub fn submit(&self, job: Job) -> FileOperationFuture {
        let work = job.clone();
        let future = self.workers.submit(move |_| execute(work));
        FileOperationFuture { job, future }
    }
}

// Unlike read tasks, the runtime never aborts this waiter or cancels its future.
// Stop and Quit wait for its actual terminal receipt before dropping the owner.
pub struct FileOperationFuture {
    job: Job,
    future: WorkFuture<Completion>,
}
impl Future for FileOperationFuture {
    type Output = Completion;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Completion> {
        let this = self.get_mut();
        Pin::new(&mut this.future).poll(cx).map(|result| result.unwrap_or_else(|error| Completion {
            job: this.job.clone(), parent: None, rename_identity: None,
            outcome: Outcome::Failed(format!("File-operation worker failed ({error:?}). Inspect the destination before retrying; partial output may remain.")),
        }))
    }
}

fn execute(job: Job) -> Completion {
    let started = std::time::Instant::now();
    let activity = match job.item {
        Item::Copy(_) => Activity::Copy,
        Item::Move(_) => Activity::Move,
        Item::Create(_) => Activity::Create,
        Item::Rename(_) => Activity::Rename,
        Item::Trash(_) => Activity::Trash,
    };
    session().record(
        Subsystem::Files,
        activity,
        DiagnosticOutcome::Started,
        job.operation,
        job.step,
        None,
    );
    let mut parent = None;
    let mut rename_identity = None;
    let (outcome, diagnostic) = match perform(&job, &mut parent, &mut rename_identity) {
        Ok(outcome) => {
            let diagnostic = match outcome {
                Outcome::Complete => DiagnosticOutcome::Complete,
                Outcome::Conflict => DiagnosticOutcome::Conflict,
                Outcome::Failed(_) => DiagnosticOutcome::Failed,
            };
            (outcome, diagnostic)
        }
        Err(failure) => (Outcome::Failed(failure.message), failure.diagnostic),
    };
    session().record(
        Subsystem::Files,
        activity,
        diagnostic,
        job.operation,
        job.step,
        Some(started.elapsed()),
    );
    Completion {
        job,
        parent,
        rename_identity,
        outcome,
    }
}

fn inspect_destination(
    job: &Job,
    parent: &mut Option<Identity>,
) -> Result<(std::fs::Metadata, std::path::PathBuf), Failure> {
    if !job.directory.is_absolute() {
        return Err("Cannot write to a relative destination. Reopen the folder.".into());
    }
    if !matches!(job.item, Item::Trash(_)) {
        job.safety.check_write_directory(&job.directory)?;
    }
    let info = std::fs::symlink_metadata(&job.directory).map_err(|e| {
        Failure::io(
            &e,
            format!("Cannot inspect the destination: {e}. Reopen the folder."),
        )
    })?;
    if !info.is_dir() || info.file_type().is_symlink() {
        return Err(
            "Destination is no longer the displayed folder. Reopen it before retrying.".into(),
        );
    }
    let canonical = std::fs::canonicalize(&job.directory).map_err(|e| {
        Failure::io(
            &e,
            format!("Cannot resolve the destination: {e}. Reopen the folder."),
        )
    })?;
    if canonical != job.directory {
        return Err("Destination path changed. Reopen the folder before retrying.".into());
    }
    *parent = Some(identity(&info));
    if job.parent.is_some_and(|expected| Some(expected) != *parent) {
        return Err(
            "Destination folder was replaced during this batch. Inspect it before retrying.".into(),
        );
    }
    Ok((info, canonical))
}

pub(super) fn check_mount_access(
    safety: &crate::domain::SafetyPreferences,
    path: &Path,
) -> Result<(), &'static str> {
    match crate::platform::volumes::classify_mount(path) {
        crate::platform::volumes::VolumeMount::Local => Ok(()),
        crate::platform::volumes::VolumeMount::External { .. } => {
            if safety.external_drives_access {
                Ok(())
            } else {
                Err("External drive access is turned off in Access and Safety settings.")
            }
        }
        crate::platform::volumes::VolumeMount::Network { .. } => {
            if safety.network_shares_access {
                Ok(())
            } else {
                Err("Network share access is turned off in Access and Safety settings.")
            }
        }
    }
}

fn perform(
    job: &Job,
    parent: &mut Option<Identity>,
    rename_identity: &mut Option<Identity>,
) -> Result<Outcome, Failure> {
    validate_name(&job.name).map_err(|message| {
        Failure::safe(
            message,
            "The proposed filename is invalid. Choose another name.",
        )
    })?;
    let (info, canonical) = inspect_destination(job, parent)?;
    check_mount_access(&job.safety.preferences, &canonical)?;
    if let Item::Trash(source) = &job.item {
        return trash(job, source, &info, &canonical);
    }
    job.safety.check_write_directory(&canonical)?;
    let destination = job.directory.join(&job.name);
    if let Item::Rename(source) = &job.item {
        return rename(job, source, &destination, &info, rename_identity);
    }
    if let Item::Move(source) = &job.item {
        return moving::perform(job, source, &destination, &info, rename_identity);
    }
    match std::fs::symlink_metadata(&destination) {
        Ok(_) => return Ok(Outcome::Conflict),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(Failure::io(
                &e,
                format!(
                    "Cannot inspect the proposed filename: {e}. Check folder access and retry."
                ),
            ));
        }
    }
    let source_info = match &job.item {
        Item::Copy(source) => Some(inspect_copy_source(job, source, &canonical)?),
        Item::Create(_) | Item::Rename(_) | Item::Move(_) | Item::Trash(_) => None,
    };
    if identity(&std::fs::metadata(&job.directory).map_err(|e| Failure::io(&e, e.to_string()))?)
        != identity(&info)
    {
        return Err("Destination folder changed before writing. Reopen it before retrying.".into());
    }
    match &job.item {
        Item::Copy(source) => {
            crate::platform::file_copy::copy(source, &destination).map_err(|message| {
                Failure::safe(
                    message,
                    "The native copy did not complete. Inspect the destination before retrying; partial output may remain.",
                )
            })?;
            if let Some(before) = source_info {
                let after = std::fs::symlink_metadata(source).map_err(|e| Failure::io(&e, format!("Source changed during copy: {e}. Inspect the retained destination before retrying.")))?;
                if identity(&before) != identity(&after)
                    || before.len() != after.len()
                    || before.modified().ok() != after.modified().ok()
                {
                    return Err("Source changed during copy. The destination was retained; inspect it before retrying.".into());
                }
            }
        }
        Item::Create(kind) => {
            if matches!(create(*kind, &destination)?, Outcome::Conflict) {
                return Ok(Outcome::Conflict);
            }
        }
        Item::Rename(_) | Item::Move(_) | Item::Trash(_) => {
            return Err(Failure::safe(
                 "Move, Rename, or Trash reached the copy executor. Inspect the folder and report this error."
                    .into(),
                "The move, rename, or trash request could not be executed safely. No copy was attempted. Inspect the folder and report this error.",
            ));
        }
    }
    confirm_output(job, &destination, &info)
}

mod moving;

fn confirm_output(
    job: &Job,
    destination: &Path,
    parent: &std::fs::Metadata,
) -> Result<Outcome, Failure> {
    if identity(&std::fs::metadata(&job.directory).map_err(|e| {
        Failure::io(
            &e,
            format!("Cannot recheck destination: {e}. Inspect output before retrying."),
        )
    })?) != identity(parent)
    {
        return Err(
            "Destination folder changed during the operation. Inspect output before retrying."
                .into(),
        );
    }
    if std::fs::symlink_metadata(destination).is_err() {
        return Err("The created destination disappeared before confirmation. Inspect the folder before retrying.".into());
    }
    Ok(Outcome::Complete)
}

fn inspect_copy_source(
    job: &Job,
    source: &Path,
    destination: &Path,
) -> Result<std::fs::Metadata, Failure> {
    if !source.is_absolute() || source.to_str().is_none() {
        return Err("Cannot use this native path exactly. Use Finder for this entry.".into());
    }
    job.safety
        .check_read_directory(source)
        .map_err(Failure::from)?;
    if let Some(parent) = source.parent() {
        let canonical_parent = std::fs::canonicalize(parent).map_err(|e| {
            Failure::io(
                &e,
                format!("Cannot resolve the source folder: {e}. Copy the source again."),
            )
        })?;
        job.safety
            .check_read_directory(&canonical_parent)
            .map_err(Failure::from)?;
        check_mount_access(&job.safety.preferences, &canonical_parent)?;
    }
    let info = std::fs::symlink_metadata(source).map_err(|e| {
        Failure::io(
            &e,
            format!("Cannot inspect the source: {e}. Copy the source again."),
        )
    })?;
    if !(info.is_file() || info.is_dir() || info.file_type().is_symlink()) {
        return Err(
            "Cannot transfer this entry type. Only files, folders, and symbolic links are supported."
                .into(),
        );
    }
    if info.is_dir() {
        let source = std::fs::canonicalize(source).map_err(|e| {
            Failure::io(
                &e,
                format!("Cannot resolve the source folder: {e}. Copy it again."),
            )
        })?;
        job.safety
            .check_read_directory(&source)
            .map_err(Failure::from)?;
        check_mount_access(&job.safety.preferences, &source)?;
        if destination.starts_with(&source) {
            return Err("Cannot put a folder inside itself. Choose another destination.".into());
        }
    }
    Ok(info)
}

fn rename(
    job: &Job,
    source: &Path,
    destination: &Path,
    parent: &std::fs::Metadata,
    original: &mut Option<Identity>,
) -> Result<Outcome, Failure> {
    if source.parent() != Some(job.directory.as_path()) || source.to_str().is_none() {
        return Err("Cannot rename outside the displayed parent folder. Reload and retry.".into());
    }
    job.safety
        .check_entry_modification(source)
        .map_err(Failure::from)?;
    let before = std::fs::symlink_metadata(source).map_err(|error| {
        Failure::io(
            &error,
            format!("Cannot inspect rename source: {error}. Reload the folder."),
        )
    })?;
    *original = Some(identity(&before));
    if job
        .rename_identity
        .is_some_and(|expected| Some(expected) != *original)
    {
        return Err("Rename source was replaced while choosing a name. Inspect the folder and start Rename again.".into());
    }
    if !(before.is_file() || before.is_dir() || before.file_type().is_symlink()) {
        return Err("Cannot rename this entry type. Use Finder for this entry.".into());
    }
    if !before.file_type().is_symlink() && before.is_dir() {
        let canonical_source = std::fs::canonicalize(source).map_err(|error| {
            Failure::io(
                &error,
                format!("Cannot resolve rename source: {error}. Reload the folder."),
            )
        })?;
        job.safety
            .check_entry_modification(&canonical_source)
            .map_err(Failure::from)?;
    }
    if source == destination {
        return Ok(Outcome::Complete);
    }
    match std::fs::symlink_metadata(destination) {
        Ok(_) => return Ok(Outcome::Conflict),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(Failure::io(
                &error,
                format!(
                    "Cannot inspect rename destination: {error}. Check folder access and retry."
                ),
            ));
        }
    }
    if identity(
        &std::fs::symlink_metadata(source)
            .map_err(|error| Failure::io(&error, error.to_string()))?,
    ) != identity(&before)
        || identity(
            &std::fs::symlink_metadata(&job.directory)
                .map_err(|error| Failure::io(&error, error.to_string()))?,
        ) != identity(parent)
    {
        return Err("Rename source or parent changed. Reload the folder before retrying.".into());
    }
    let parent_id = identity(parent);
    let before_id = identity(&before);
    match crate::platform::renaming::rename(
        source,
        destination,
        (parent_id.device, parent_id.inode),
        (before_id.device, before_id.inode),
    ) {
        Ok(()) => {}
        Err(crate::platform::renaming::RenameError::Conflict) => {
            return Ok(Outcome::Conflict);
        }
        Err(crate::platform::renaming::RenameError::Failed(error)) => {
            return Err(Failure::safe(
                error,
                "The native rename did not complete. Inspect both names before retrying.",
            ));
        }
    }
    let after = std::fs::symlink_metadata(destination).map_err(|error| {
        Failure::io(
            &error,
            format!("Cannot confirm rename: {error}. Inspect both names before retrying."),
        )
    })?;
    if identity(&after) != identity(&before)
        || identity(
            &std::fs::metadata(&job.directory)
                .map_err(|error| Failure::io(&error, error.to_string()))?,
        ) != identity(parent)
    {
        return Err(Failure::safe(
            "Rename identity changed during execution. Inspect both names before retrying.".into(),
            "The source or parent folder changed while renaming. Inspect both names before retrying.",
        ));
    }
    Ok(Outcome::Complete)
}

fn trash(
    job: &Job,
    source: &Path,
    parent: &std::fs::Metadata,
    canonical_parent: &Path,
) -> Result<Outcome, Failure> {
    if source.parent() != Some(job.directory.as_path()) || source.to_str().is_none() {
        return Err(
            "Cannot move an item outside the displayed folder to Trash. Reload and retry.".into(),
        );
    }
    job.safety.check_trash(source).map_err(Failure::from)?;
    if let Some(name) = source.file_name() {
        job.safety
            .check_trash(&canonical_parent.join(name))
            .map_err(Failure::from)?;
    }
    let before = std::fs::symlink_metadata(source).map_err(|error| {
        Failure::io(
            &error,
            format!("Cannot inspect item before moving to Trash: {error}. Reload the folder."),
        )
    })?;
    if !(before.is_file() || before.is_dir() || before.file_type().is_symlink()) {
        return Err("Cannot move this entry type to Trash. Use Finder for this entry.".into());
    }
    if !before.file_type().is_symlink() && before.is_dir() {
        let canonical_source = std::fs::canonicalize(source).map_err(|error| {
            Failure::io(
                &error,
                format!(
                    "Cannot resolve folder before moving to Trash: {error}. Reload the folder."
                ),
            )
        })?;
        job.safety
            .check_trash(&canonical_source)
            .map_err(Failure::from)?;
    }
    if identity(
        &std::fs::symlink_metadata(source)
            .map_err(|error| Failure::io(&error, error.to_string()))?,
    ) != identity(&before)
        || identity(
            &std::fs::symlink_metadata(&job.directory)
                .map_err(|error| Failure::io(&error, error.to_string()))?,
        ) != identity(parent)
    {
        return Err(
            "Item or parent folder changed before moving to Trash. Reload and retry.".into(),
        );
    }
    crate::platform::trash::trash(source).map_err(|error| {
        Failure::safe(
            error,
            "The native Trash service could not move the item to Trash. Inspect the folder and Trash before retrying.",
        )
    })?;
    if identity(
        &std::fs::metadata(&job.directory)
            .map_err(|error| Failure::io(&error, error.to_string()))?,
    ) != identity(parent)
    {
        return Err(
            "Parent folder changed while moving the item to Trash. Inspect the folder and Trash."
                .into(),
        );
    }
    match std::fs::symlink_metadata(source) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Outcome::Complete),
        Ok(_) => Err(
            "The item remained in the folder after Trash placement. Inspect the folder and Trash before retrying."
                .into(),
        ),
        Err(error) => Err(Failure::io(
            &error,
            format!(
                "Cannot confirm Trash placement: {error}. Inspect the folder and Trash before retrying."
            ),
        )),
    }
}

fn identity(info: &std::fs::Metadata) -> Identity {
    use std::os::unix::fs::MetadataExt;
    Identity {
        device: info.dev(),
        inode: info.ino(),
    }
}

fn create(kind: crate::domain::files::CreateKind, destination: &Path) -> Result<Outcome, Failure> {
    if kind == crate::domain::files::CreateKind::Folder {
        return match std::fs::create_dir(destination) {
            Ok(()) => Ok(Outcome::Complete),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(Outcome::Conflict),
            Err(e) => Err(Failure::io(
                &e,
                format!("Cannot create folder: {e}. Check access and retry."),
            )),
        };
    }
    let mut file = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(Outcome::Conflict),
        Err(e) => {
            return Err(Failure::io(
                &e,
                format!("Cannot create file: {e}. Check access and retry."),
            ));
        }
    };
    file.write_all(kind.contents()).and_then(|()| file.sync_all())
        .map_err(|e| Failure::io(&e, format!("Cannot finish creating file: {e}. The new entry was retained; inspect it before retrying.")))?;
    Ok(Outcome::Complete)
}
