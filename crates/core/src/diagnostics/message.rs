//! Product operations, causes, effects, and safe next actions, not queue narration.
use super::{Activity, Change, Event, Outcome, SizeIssue, Subsystem};

pub(super) fn operation(subsystem: Subsystem, activity: Activity) -> Option<&'static str> {
    match activity {
        Activity::ConfigurationChanged(_) => Some("change configuration"),
        Activity::SizeIssue(_) => Some("estimate folder size"),
        Activity::OpenFile => Some("open the file in its default application"),
        Activity::OpenLauncher => Some("open the shortcut target"),
        Activity::OpenLogs => Some("open the diagnostic logs folder"),
        Activity::CheckWorkspaceReference => Some("load the active workspace"),
        Activity::Load => Some("load saved configuration"),
        Activity::Restore => Some("restore the configuration backup"),
        Activity::Save => Some("save settings and workspace changes"),
        Activity::Manage => Some("complete the workspace change"),
        Activity::Validate => Some("validate the proposed workspace change"),
        Activity::Work => match subsystem {
            Subsystem::Listing => Some("read folder contents"),
            Subsystem::Metadata => Some("read file details"),
            Subsystem::Thumbnail => Some("load an image preview"),
            Subsystem::Sorting => Some("sort folder contents"),
            Subsystem::Size => Some("estimate folder size"),
            Subsystem::Files => Some("run the file operation"),
            Subsystem::Opening => Some("open the file in its default application"),
            _ => None,
        },
        Activity::Register => Some("watch folders for changes"),
        Activity::Overflow => Some("track every folder change"),
        Activity::Close => Some("save before closing"),
        Activity::Copy => Some("copy the item"),
        Activity::Move => Some("move the item"),
        Activity::Create => Some("create the item"),
        Activity::Rename => Some("rename the item"),
        Activity::Trash => Some("move the item to the Trash"),
        Activity::Panic => Some("continue after an application panic"),
        Activity::Shutdown => Some("run the application"),
        // These hooks describe bookkeeping, or repeat a result already logged
        // by the operation owner. They must never become standalone records.
        _ => None,
    }
}

pub(super) fn text(event: &Event) -> Option<String> {
    let operation = operation(event.subsystem, event.activity)?;
    if let Outcome::FileFailure(explanation) = event.outcome {
        return Some(format!("Could not {operation}. {explanation}"));
    }
    if let Activity::ConfigurationChanged(change) = event.activity {
        return Some(format!("{}.", change_message(change)));
    }
    if let Activity::SizeIssue(cause) = event.activity {
        return Some(size_message(cause, event.outcome == Outcome::Partial));
    }
    match event.activity {
        Activity::CheckWorkspaceReference => return Some("Cannot load the active workspace: the saved workspace reference does not match the workspace list. Automatic saving is disabled. Inspect configuration and backups in recovery details.".into()),
        Activity::Close => return Some("Application stayed open because the final workspace save failed. Retry saving, or explicitly choose Close without saving.".into()),
        Activity::Overflow => return Some("Folder-change notifications exceeded the pending-update limit. Reloading affected folders to catch up.".into()),
        Activity::Panic => return Some("Application code panicked. Collect application error output for the source location and cause; private panic details are not written to this log.".into()),
        Activity::Shutdown => return Some("The application stopped because its desktop runtime returned an error. Collect application error output along with this log.".into()),
        _ => {}
    }
    let reason = match event.outcome {
        Outcome::Partial => return Some("Folder contents could not be read completely. Some entries or file details may be missing. Reload the folder to retry.".into()),
        Outcome::RecoveryOnly => return Some("Some saved workspace records could not be loaded. Opened a read-only viewing copy; automatic saving is disabled. Inspect recovery details and backups.".into()),
        Outcome::Busy if event.subsystem == Subsystem::Storage => "another file-tile session is using this data folder".into(),
        Outcome::Busy => "too many requests are pending".into(),
        Outcome::Unavailable => "the required background service is unavailable".into(),
        Outcome::TimedOut => "the operation exceeded its time limit".into(),
        Outcome::InvalidOutput => "the system utility returned an unreadable result".into(),
        Outcome::UnsafePath => "a managed configuration path is a symbolic link or has the wrong file type".into(),
        // Constructed solely from ErrorKind, never from a raw OS error or path.
        Outcome::Io(kind) => match kind {
            std::io::ErrorKind::StorageFull => "the disk is full".into(),
            std::io::ErrorKind::PermissionDenied => "access was denied".into(),
            std::io::ErrorKind::ReadOnlyFilesystem => "the disk is read-only".into(),
            std::io::ErrorKind::NotFound => "a required file or folder is missing".into(),
            _ => std::io::Error::from(kind).to_string(),
        },
        Outcome::InvalidConfiguration => "the configuration is invalid or unsupported".into(),
        Outcome::ExternalChange => "configuration files changed outside this session".into(),
        Outcome::ReadOnly => "configuration recovery is read-only".into(),
        Outcome::BackupAvailable => return Some(format!("Could not {operation}. A validated configuration backup is available. Review Restore backup in recovery details before replacing any files.")),
        Outcome::NativeFailure => "the macOS service reported a failure".into(),
        Outcome::NotFound => "the folder no longer exists".into(),
        Outcome::NotDirectory => "the location is not a folder".into(),
        Outcome::PermissionDenied => "access was denied".into(),
        Outcome::OutputLimit => "the system utility exceeded its output limit".into(),
        Outcome::Failed => "the operation failed; its cause was not recorded".into(),
        _ => return None,
    };
    Some(format!(
        "Could not {operation}: {reason}. {}",
        guidance(event)
    ))
}

fn guidance(event: &Event) -> &'static str {
    match event.subsystem {
        Subsystem::Storage => match event.outcome {
            Outcome::Busy => "Close the other session, then retry.",
            Outcome::ExternalChange
            | Outcome::UnsafePath
            | Outcome::InvalidConfiguration
            | Outcome::ReadOnly => {
                "Saving is blocked. Preserve the configuration files and inspect recovery details before retrying."
            }
            _ if event.activity == Activity::Save => {
                "Changes are not confirmed saved. Keep the application open, check data-folder access and disk space, then retry."
            }
            _ => {
                "Inspect recovery details before retrying; existing configuration files were not reset."
            }
        },
        Subsystem::Listing => {
            "Folder contents are unavailable or incomplete. Reload the folder to retry."
        }
        Subsystem::Watcher => {
            "Automatic folder updates may be delayed. Reload folders manually while observation reconnects."
        }
        Subsystem::Files => {
            "Inspect the source and destination before retrying; partial output may remain."
        }
        Subsystem::Opening if event.outcome == Outcome::TimedOut => {
            "An application may already have opened. Check before retrying Open."
        }
        Subsystem::Opening => "Check the file and its default application, then retry Open.",
        Subsystem::Size => {
            "The size estimate was not refreshed. Any earlier estimate remains available; retry on navigation or reload after five minutes."
        }
        Subsystem::Metadata => "Some file details may be missing. Reload the folder to retry.",
        Subsystem::Thumbnail => "The image preview is unavailable. The file was not changed.",
        Subsystem::Sorting => {
            "The requested ordering may not be available. Reload the folder to retry."
        }
        _ => "Collect application error output along with this log.",
    }
}

fn size_message(cause: SizeIssue, partial: bool) -> String {
    let reason = match cause {
        SizeIssue::TimedOut => "the scan reached its 30-second time limit".into(),
        SizeIssue::OutputLimit => "the scan exceeded its 256-KiB output limit".into(),
        SizeIssue::UtilityExit { code, errors } => {
            let mut causes = vec![];
            for (count, label) in [
                (errors.access_denied, "access denied"),
                (errors.not_found, "items not found"),
                (errors.io, "read errors"),
                (errors.other, "unclassified errors"),
            ] {
                if count != 0 {
                    causes.push(format!("{label}: {count}"));
                }
            }
            let status =
                code.map_or_else(|| "terminated".into(), |code| format!("exit status {code}"));
            if causes.is_empty() {
                format!("the macOS size utility failed ({status}); no cause was reported")
            } else {
                format!(
                    "the macOS size utility reported {} ({status})",
                    causes.join(", ")
                )
            }
        }
        SizeIssue::InvalidOutput => {
            "the macOS size utility did not return a complete readable total".into()
        }
        SizeIssue::Unavailable => {
            "the macOS size utility could not be started or its result could not be read".into()
        }
    };
    let effect = if partial {
        "An incomplete estimate is available, shown with ≥."
    } else {
        "No new estimate is available; any earlier estimate is kept."
    };
    format!(
        "Folder-size scan incomplete: {reason}. {effect} Navigate or reload after five minutes to retry."
    )
}

fn change_message(change: Change) -> &'static str {
    match change {
        Change::AppSettings => "Application settings changed",
        Change::BoxCreated => "Box created",
        Change::LauncherCreated => "Launcher created",
        Change::LauncherSettings => "Launcher settings changed",
        Change::LauncherRemoved => "Launcher removed",
        Change::LauncherLayout => "Launcher layout changed",
        Change::BoxRemoved => "Box removed",
        Change::BoxSettings => "Box settings changed",
        Change::BoxFolder => "Box folder configured",
        Change::BoxScope => "Box navigation scope configured",
        Change::BoxSort => "Box sorting configured",
        Change::BoxLayout => "Box layout changed",
        Change::BoxTabs => "Box tab structure changed",
        Change::WorkspaceCreated => "Workspace created and selected",
        Change::WorkspaceSelected => "Workspace selected",
        Change::WorkspaceRemoved => "Workspace removed from the workspace list",
        Change::WorkspaceRenamed => "Workspace renamed",
        Change::WorkspaceUndo => "Workspace edit undone",
    }
}
