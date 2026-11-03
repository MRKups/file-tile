//! Move preflight and confirmation, separate from native execution and copy effects.
use super::*;

pub(super) fn perform(
    job: &Job,
    source: &Path,
    destination: &Path,
    parent: &std::fs::Metadata,
    original: &mut Option<Identity>,
) -> Result<Outcome, Failure> {
    let before = inspect_copy_source(source, &job.directory)?;
    *original = Some(identity(&before));
    if job
        .rename_identity
        .is_some_and(|expected| Some(expected) != *original)
    {
        return Err(
            "Move source was replaced while choosing a name. Inspect both folders and start again."
                .into(),
        );
    }
    let source_parent = source
        .parent()
        .ok_or("Cannot move a filesystem root. Choose an entry inside it.")?;
    let canonical_parent = std::fs::canonicalize(source_parent).map_err(|error| {
        Failure::io(
            &error,
            format!("Cannot resolve move source folder: {error}. Copy the source again."),
        )
    })?;
    if canonical_parent != source_parent {
        return Err("Move source folder is not its resolved path. Copy the source again from its actual folder.".into());
    }
    job.safety
        .check_entry_modification(source)
        .map_err(Failure::from)?;
    if let Some(name) = source.file_name() {
        job.safety
            .check_entry_modification(&canonical_parent.join(name))
            .map_err(Failure::from)?;
    }
    if !before.file_type().is_symlink() && before.is_dir() {
        let canonical_source = std::fs::canonicalize(source).map_err(|error| {
            Failure::io(
                &error,
                format!("Cannot resolve move source: {error}. Copy the source again."),
            )
        })?;
        job.safety
            .check_entry_modification(&canonical_source)
            .map_err(Failure::from)?;
    }
    let from_info = std::fs::symlink_metadata(source_parent).map_err(|error| {
        Failure::io(
            &error,
            format!("Cannot inspect move source folder: {error}. Reopen it and retry."),
        )
    })?;
    if !from_info.is_dir() || from_info.file_type().is_symlink() {
        return Err("Move source folder changed. Reopen it before retrying.".into());
    }
    if source == destination {
        return Ok(Outcome::Complete);
    }
    let from_id = identity(&from_info);
    let parent_id = identity(parent);
    let before_id = identity(&before);
    match crate::platform::moving::move_entry(
        source,
        destination,
        (from_id.device, from_id.inode),
        (parent_id.device, parent_id.inode),
        (before_id.device, before_id.inode),
    ) {
        Ok(()) => {}
        Err(crate::platform::moving::MoveError::Conflict) => {
            return Ok(Outcome::Conflict);
        }
        Err(crate::platform::moving::MoveError::CrossVolume) => {
            return cross_volume(job, source, destination, parent, &from_info, &before);
        }
        Err(crate::platform::moving::MoveError::Failed(error)) => {
            return Err(Failure::safe(
                error,
                "The native move did not complete or could not be confirmed. Inspect the source and destination before retrying.",
            ));
        }
    }
    let after = std::fs::symlink_metadata(destination).map_err(|error| {
        Failure::io(
            &error,
            format!("Cannot confirm moved entry: {error}. Inspect both folders before retrying."),
        )
    })?;
    if identity(&after) != identity(&before)
        || identity(
            &std::fs::metadata(source_parent)
                .map_err(|error| Failure::io(&error, error.to_string()))?,
        ) != identity(&from_info)
    {
        return Err("Move source folder or destination identity changed. Inspect both folders before retrying.".into());
    }
    confirm_output(job, destination, parent)
}

fn cross_volume(
    job: &Job,
    source: &Path,
    destination: &Path,
    parent: &std::fs::Metadata,
    from_info: &std::fs::Metadata,
    before: &std::fs::Metadata,
) -> Result<Outcome, Failure> {
    match std::fs::symlink_metadata(destination) {
        Ok(_) => return Ok(Outcome::Conflict),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(Failure::io(
                &error,
                format!(
                    "Cannot inspect the proposed filename: {error}. Check folder access and retry."
                ),
            ));
        }
    }
    if identity(
        &std::fs::metadata(&job.directory)
            .map_err(|error| Failure::io(&error, error.to_string()))?,
    ) != identity(parent)
    {
        return Err("Destination folder changed before writing. Reopen it before retrying.".into());
    }
    crate::platform::file_copy::copy(source, destination).map_err(|message| {
        Failure::safe(
            message,
            "The cross-volume copy did not complete. The original source was retained; inspect the destination before retrying.",
        )
    })?;
    let outcome = confirm_output(job, destination, parent)?;
    let after_source = std::fs::symlink_metadata(source).map_err(|error| {
        Failure::io(
            &error,
            format!("Source changed during cross-volume move: {error}. Both copies were retained."),
        )
    })?;
    if identity(&after_source) != identity(before)
        || before.len() != after_source.len()
        || before.modified().ok() != after_source.modified().ok()
    {
        return Err("Move source changed during cross-volume copy. Both the original source and destination copy were retained.".into());
    }
    let from_id = identity(from_info);
    let before_id = identity(before);
    crate::platform::moving::remove_verified_source(
        source,
        (from_id.device, from_id.inode),
        (before_id.device, before_id.inode),
        before.is_dir() && !before.file_type().is_symlink(),
    )
    .map_err(|message| {
        Failure::safe(
            message,
            "The item was copied to the destination volume, but the original source could not be removed. Both copies were retained.",
        )
    })?;
    Ok(outcome)
}
