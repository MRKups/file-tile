//! Conventional text records: RFC 3339 UTC, severity, component, message, context.
use super::{Activity, Event, Outcome, Subsystem, message};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn timestamp(time: SystemTime) -> String {
    let millis = match time.duration_since(UNIX_EPOCH) {
        Ok(d) => i128::try_from(d.as_millis()).unwrap_or(i128::MAX),
        Err(e) => -i128::try_from(e.duration().as_millis()).unwrap_or(i128::MAX),
    };
    let seconds = millis.div_euclid(1000);
    let days = seconds.div_euclid(86400);
    if !(-719528..=2932896).contains(&days) {
        return "0000-01-01T00:00:00.000Z".into();
    }
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = year + i128::from(month <= 2);
    let clock = seconds.rem_euclid(86400);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        clock / 3600,
        clock / 60 % 60,
        clock % 60,
        millis.rem_euclid(1000)
    )
}

impl Event {
    pub(super) fn line(&self) -> Option<String> {
        let severity = severity(self.subsystem, self.activity, self.outcome)?;
        let text = message::text(self)?;
        let component = match self.subsystem {
            Subsystem::Session => "session",
            Subsystem::Configuration => "configuration",
            Subsystem::Listing => "folder-reading",
            Subsystem::Metadata => "file-details",
            Subsystem::Thumbnail => "image-preview",
            Subsystem::Sorting => "sorting",
            Subsystem::Size => "folder-size",
            Subsystem::Watcher => "folder-updates",
            Subsystem::Storage => "configuration-storage",
            Subsystem::Files => "file-operation",
            Subsystem::Opening => "file-opening",
        };
        let mut line = format!(
            "{} {severity:<5} {component} - {text} pid={} elapsed_ms={}",
            timestamp(self.timestamp),
            std::process::id(),
            self.elapsed.as_millis()
        );
        if self.request != 0 {
            line.push_str(&format!(" request_id={}", self.request));
        }
        let detail = match (self.subsystem, self.activity) {
            (Subsystem::Storage, Activity::Save) => Some("revision"),
            (Subsystem::Storage, Activity::Manage) => Some("attempt"),
            (Subsystem::Watcher, Activity::Register) => Some("affected_folders"),
            (
                Subsystem::Files,
                Activity::Copy
                | Activity::Move
                | Activity::Create
                | Activity::Rename
                | Activity::Trash,
            ) => Some("item_number"),
            _ => None,
        };
        if let Some(name) = detail
            && self.detail != 0
        {
            line.push_str(&format!(" {name}={}", self.detail));
        }
        if let Some(duration) = self.duration {
            line.push_str(&format!(" duration_ms={}", duration.as_millis()));
        }
        Some(line)
    }
}

/// Only product operations with a useful explanation can enter the queue.
pub(super) fn severity(
    subsystem: Subsystem,
    activity: Activity,
    value: Outcome,
) -> Option<&'static str> {
    message::operation(subsystem, activity)?;
    if matches!(activity, Activity::ConfigurationChanged(_)) {
        return (value == Outcome::Complete).then_some("INFO");
    }
    match value {
        Outcome::Failed
        | Outcome::UnsafePath
        | Outcome::Io(_)
        | Outcome::FileFailure(_)
        | Outcome::InvalidConfiguration
        | Outcome::NativeFailure => Some("ERROR"),
        Outcome::Partial
        | Outcome::Busy
        | Outcome::Unavailable
        | Outcome::TimedOut
        | Outcome::InvalidOutput
        | Outcome::RecoveryOnly
        | Outcome::ExternalChange
        | Outcome::ReadOnly
        | Outcome::BackupAvailable
        | Outcome::OutputLimit
        | Outcome::NotFound
        | Outcome::NotDirectory
        | Outcome::PermissionDenied => Some("WARN"),
        _ => None,
    }
}
