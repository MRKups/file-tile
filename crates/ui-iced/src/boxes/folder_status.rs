//! Stable left metadata and independent right status, never extra layout rows.
use iced::widget::{container, row, text, tooltip};
use iced::{Color, Element, Fill};

use crate::application::files::folder_usage::{Estimate, UsageError};
use crate::application::{LoadState, contracts::watching::WatchState, files::sorting::PREPARING};
use crate::domain::NavigationIntent;

use crate::{Message, runtime::metadata, window::appearance as a, window::typography};

#[cfg(target_os = "macos")]
const RETRY: &str = "⌘R to retry";
#[cfg(not(target_os = "macos"))]
const RETRY: &str = "Ctrl+R to retry";

pub(crate) fn height(ty: typography::Style) -> f32 {
    ty.row(34.0)
}

struct Status {
    label: String,
    detail: String,
    color: Color,
}

fn statuses(
    load: Option<&LoadState>,
    hint: Option<&str>,
    watch: Option<&WatchState>,
    palette: a::Palette,
    sort_status: Option<&str>,
) -> Vec<Status> {
    let mut statuses = vec![];
    if let Some(LoadState::Failed { message, .. }) = load {
        statuses.push(failure("Folder read failed", message.clone(), palette));
    } else if let Some(listing) = load.and_then(LoadState::listing)
        && let Some(issue) = listing.issues.first()
    {
        statuses.push(failure(
            "Incomplete listing",
            format!(
                "{} read issues, {} entries omitted. {}",
                listing.issues.len(),
                listing.omitted_count(),
                issue.error
            ),
            palette,
        ));
    }
    if let Some(hint) = hint {
        statuses.push(Status {
            label: hint.replace(['\r', '\n'], " "),
            detail: hint.into(),
            color: palette.text,
        });
    }
    if let Some(WatchState::Unavailable(reason)) = watch {
        statuses.push(failure(
            "Automatic updates unavailable",
            reason.clone(),
            palette,
        ));
    }
    if let Some(LoadState::Loading {
        request, intent, ..
    }) = load
    {
        if *intent != NavigationIntent::Reload || load.and_then(LoadState::listing).is_none() {
            statuses.push(Status {
                label: "Loading folder…".into(),
                detail: format!("Loading {}", request.directory.display()),
                color: palette.muted,
            });
        }
    } else if load.is_none() {
        statuses.push(Status {
            label: "Not loaded".into(),
            detail: "Not loaded".into(),
            color: palette.muted,
        });
    }
    if let Some(message) = sort_status {
        statuses.push(if message == PREPARING {
            Status {
                label: message.into(),
                detail: message.into(),
                color: palette.muted,
            }
        } else {
            failure("Sorting incomplete", message.into(), palette)
        });
    }
    statuses
}

pub fn view<'a>(
    load: Option<&LoadState>,
    hint: Option<&'a str>,
    watch: Option<&WatchState>,
    show_hidden: bool,
    palette: a::Palette,
    sort_status: Option<&'a str>,
    usage: Option<&Result<Estimate, UsageError>>,
) -> Element<'a, Message> {
    let ty = palette.typography;
    let statuses = statuses(load, hint, watch, palette, sort_status);
    let summary = summary(load, show_hidden, usage);
    let detail = statuses
        .iter()
        .map(|status| status.detail.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let (label, color) = statuses.first().map_or(("", palette.muted), |status| {
        (status.label.as_str(), status.color)
    });
    let left = ty
        .small(summary)
        .color(palette.muted)
        .wrapping(text::Wrapping::None);
    // The left child is measured first. Status gets only the remaining width and
    // cannot push, overwrite, or wrap the metadata, even for a long failure.
    let right = container(
        ty.small(label.to_owned())
            .color(color)
            .wrapping(text::Wrapping::None),
    )
    .align_right(Fill)
    .clip(true);
    let right: Element<'_, Message> = if detail.is_empty() {
        right.into()
    } else {
        tooltip(
            right,
            container(ty.small(detail)).max_width(500),
            tooltip::Position::Top,
        )
        .delay(std::time::Duration::from_millis(650))
        .style(move |_| palette.tooltip())
        .into()
    };
    container(row![left, right].spacing(12).align_y(iced::Center))
        .padding([10, 17])
        .width(Fill)
        .height(height(ty))
        .align_y(iced::Center)
        .clip(true)
        .into()
}

fn failure(label: &str, detail: String, palette: a::Palette) -> Status {
    Status {
        label: format!("{label} · {RETRY}"),
        detail: format!("{detail}\n{RETRY}."),
        color: palette.error,
    }
}

fn summary(
    load: Option<&LoadState>,
    show_hidden: bool,
    usage: Option<&Result<Estimate, UsageError>>,
) -> String {
    let count = load.and_then(LoadState::listing).map_or_else(
        || "– items".into(),
        |listing| {
            let count = listing.display_len(show_hidden);
            let qualifier = if listing.issues.is_empty() { "" } else { "≥" };
            format!(
                "{qualifier}{count} {}",
                if count == 1 { "item" } else { "items" }
            )
        },
    );
    match usage {
        Some(Ok(value)) => format!(
            "{count} · {}{}",
            if value.partial { "≥" } else { "~" },
            metadata::decimal_size(value.bytes)
        ),
        Some(Err(_)) => format!("{count} · Size unavailable"),
        None => count,
    }
}
