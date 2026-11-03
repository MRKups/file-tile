//! File-detail task ownership and pure formatting. No stat calls in widgets.
#[cfg(test)]
use crate::DirectoryBackend;
use crate::application::Action;
use crate::application::contracts::directory::Cancellation;
#[cfg(test)]
use crate::application::contracts::directory::DirectoryReader;
use crate::application::files::metadata::{FileMetadata, MetadataError, MetadataRequest};
use crate::{Desktop, Message};
use iced::Task;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;
#[cfg(not(target_os = "macos"))]
use std::time::UNIX_EPOCH;
pub(crate) type Values = HashMap<PathBuf, Result<FileMetadata, MetadataError>>;
pub(crate) struct Pending {
    request: MetadataRequest,
    cancellation: Cancellation,
    handle: iced::task::Handle,
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.handle.abort();
    }
}
impl Desktop {
    pub(crate) fn read_metadata(&mut self, request: MetadataRequest) -> Task<Message> {
        #[cfg(test)]
        if let DirectoryBackend::Controlled(reader) = &self.directory {
            let result = Ok(request
                .paths
                .iter()
                .map(|path| {
                    (
                        path.clone(),
                        reader.metadata(path, &Cancellation::default()),
                    )
                })
                .collect());
            return Task::done(Message::Action(Action::MetadataCompleted(
                crate::application::files::metadata::MetadataCompletion { request, result },
            )));
        }
        let Some(workers) = &self.metadata_workers else {
            return Task::done(Message::Action(Action::MetadataCompleted(
                crate::application::files::metadata::MetadataCompletion {
                    request,
                    result: Err(MetadataError::Unavailable),
                },
            )));
        };
        let future = workers.submit(request.clone());
        let cancellation = future.cancellation();
        let (task, handle) = Task::perform(future, |completion| {
            Message::Action(Action::MetadataCompleted(completion))
        })
        .abortable();
        self.metadata_reads.insert(
            request.view,
            Pending {
                request,
                cancellation,
                handle,
            },
        );
        task
    }
    pub(crate) fn retain_metadata_reads(&mut self) {
        self.metadata_reads.retain(|_, pending| {
            self.application
                .metadata_requests()
                .any(|request| request == &pending.request)
        });
    }
}
pub(crate) fn size(value: Option<&Result<FileMetadata, MetadataError>>) -> String {
    match value {
        None => "Loading…".into(),
        Some(Err(_)) => "Unavailable".into(),
        Some(Ok(metadata)) => metadata.bytes.map_or_else(|| "–".into(), decimal_size),
    }
}
pub(crate) fn decimal_size(bytes: u64) -> String {
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let units = ["kB", "MB", "GB", "TB", "PB", "EB"];
    let mut amount = bytes as f64 / 1000.0;
    let mut index = 0;
    while amount >= 1000.0 && index + 1 < units.len() {
        amount /= 1000.0;
        index += 1;
    }
    format!("{amount:.1} {}", units[index])
}
pub(crate) fn date(value: Option<&Result<FileMetadata, MetadataError>>) -> String {
    match value {
        None => "Loading…".into(),
        Some(Ok(metadata)) => metadata
            .modified
            .as_ref()
            .ok()
            .and_then(|time| local_datetime(*time))
            .unwrap_or_else(|| "Unavailable".into()),
        Some(Err(_)) => "Unavailable".into(),
    }
}
#[cfg(target_os = "macos")]
fn local_datetime(time: SystemTime) -> Option<String> {
    crate::platform::format_local_datetime(time)
}
#[cfg(not(target_os = "macos"))]
fn local_datetime(time: SystemTime) -> Option<String> {
    let seconds = match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_secs()).ok()?,
        Err(error) => {
            -i64::try_from(error.duration().as_secs()).ok()?
                - i64::from(error.duration().subsec_nanos() > 0)
        }
    };
    let days = seconds.div_euclid(86400);
    let minute = seconds.rem_euclid(86400) / 60;
    // Gregorian civil date from days since 1970. Bound before era arithmetic.
    if !(-719528..=2932896).contains(&days) {
        return None;
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
    let year = year + i64::from(month <= 2);
    Some(format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        minute / 60,
        minute % 60
    ))
}
pub(crate) fn error(value: Option<&Result<FileMetadata, MetadataError>>) -> Option<String> {
    match value {
        Some(Err(error)) => Some(error.guidance()),
        Some(Ok(metadata)) => metadata
            .modified
            .as_ref()
            .err()
            .map(MetadataError::guidance),
        None => None,
    }
}
pub(crate) fn labels(
    props: crate::boxes::file_list::Props<'_>,
    entry: &crate::application::contracts::directory::DirectoryEntry,
) -> (String, String, Option<String>) {
    let value = props.metadata.and_then(|values| values.get(&entry.path));
    let deferred = value.is_none()
        && props
            .metadata_paths
            .is_some_and(|paths| !paths.contains(&entry.path));
    let bytes = if entry.kind == crate::application::contracts::directory::EntryKind::Directory {
        "–".into()
    } else if deferred {
        "Deferred".into()
    } else {
        size(value)
    };
    let date = if deferred {
        "Deferred".into()
    } else {
        date(value)
    };
    let error = if deferred {
        Some("Metadata deferred. Scroll to or select this entry to request it.".into())
    } else {
        error(value)
    };
    (bytes, date, error)
}
pub(crate) const HOVER_DELAY: std::time::Duration = std::time::Duration::from_millis(1500);

pub(crate) fn hover_tooltip<'a>(
    target: iced::Element<'a, Message>,
    props: crate::boxes::file_list::Props<'_>,
    listing: Option<&crate::application::contracts::directory::DirectoryListing>,
    entry: &crate::application::contracts::directory::DirectoryEntry,
) -> iced::Element<'a, Message> {
    let palette = props.palette;
    let ty = palette.typography;
    let hint = hover_tooltip_text(props, listing, entry);
    iced::widget::tooltip(
        target,
        ty.small(hint),
        iced::widget::tooltip::Position::FollowCursor,
    )
    .delay(HOVER_DELAY)
    .style(move |_| palette.tooltip())
    .into()
}

pub(crate) fn hover_tooltip_text(
    props: crate::boxes::file_list::Props<'_>,
    listing: Option<&crate::application::contracts::directory::DirectoryListing>,
    entry: &crate::application::contracts::directory::DirectoryEntry,
) -> String {
    let (file_size, date, _) = labels(props, entry);
    let size = if entry.kind == crate::application::contracts::directory::EntryKind::Directory {
        folder_size_label(props, listing, entry)
    } else {
        file_size
    };
    if size == date || size == "–" {
        date
    } else {
        format!("{size}\n{date}")
    }
}

fn folder_size_label(
    props: crate::boxes::file_list::Props<'_>,
    listing: Option<&crate::application::contracts::directory::DirectoryListing>,
    entry: &crate::application::contracts::directory::DirectoryEntry,
) -> String {
    let cached = props.folder_usages.and_then(|book| {
        book.get(&entry.path).or_else(|| {
            listing
                .and_then(|dir| dir.entry_target(entry))
                .and_then(|target| book.get(target))
        })
    });
    let parent_pending = listing.is_some_and(|dir| {
        props
            .folder_usages
            .is_some_and(|book| book.is_pending(&dir.directory))
    }) || props.usage.is_none();
    match cached {
        Some(Ok(estimate)) => format!(
            "{}{}",
            if estimate.partial { "≥" } else { "~" },
            decimal_size(estimate.bytes)
        ),
        Some(Err(_)) => "Size unavailable".into(),
        None if parent_pending && !entry.symlink => "Calculating size…".into(),
        None => "Size unavailable".into(),
    }
}
