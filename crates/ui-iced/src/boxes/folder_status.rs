//! Stable left metadata and independent right status, never extra layout rows.
use iced::widget::{Row, container, row, rule, text, tooltip};
use iced::{Color, Element, Fill};

use crate::application::files::folder_usage::{Estimate, UsageError};
use crate::application::{LoadState, contracts::watching::WatchState, files::sorting::PREPARING};
use crate::domain::{CapabilityItem, CapabilityPreferences, NavigationIntent};

use crate::{Message, runtime::metadata, window::appearance as a, window::typography};

#[cfg(target_os = "macos")]
const RETRY: &str = "⌘R to retry";
#[cfg(not(target_os = "macos"))]
const RETRY: &str = "Ctrl+R to retry";

pub(crate) struct Props<'a> {
    pub load: Option<&'a LoadState>,
    pub hint: Option<&'a str>,
    pub watch: Option<&'a WatchState>,
    pub show_hidden: bool,
    pub palette: a::Palette,
    pub sort_status: Option<&'a str>,
    pub usage: Option<&'a Result<Estimate, UsageError>>,
    pub capabilities: CapabilityPreferences,
}

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

pub(crate) fn view(props: Props<'_>) -> Element<'_, Message> {
    let palette = props.palette;
    let ty = palette.typography;
    let statuses = statuses(
        props.load,
        props.hint,
        props.watch,
        palette,
        props.sort_status,
    );
    let summary = summary(props.load, props.show_hidden, props.usage);
    let detail = statuses
        .iter()
        .map(|status| status.detail.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let (label, color) = statuses.first().map_or(("", palette.muted), |status| {
        (status.label.as_str(), status.color)
    });
    let left_summary = ty
        .small(summary)
        .color(palette.muted)
        .wrapping(text::Wrapping::None);
    let left: Element<'_, Message> =
        if let Some(caps) = capabilities_strip(props.load, props.capabilities, palette) {
            row![
                left_summary,
                container(rule::vertical(1).style(move |_| palette.rule())).height(12),
                caps,
            ]
            .spacing(10)
            .align_y(iced::Center)
            .into()
        } else {
            left_summary.into()
        };
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

fn capabilities_strip<'a>(
    load: Option<&'a LoadState>,
    preferences: CapabilityPreferences,
    palette: a::Palette,
) -> Option<Element<'a, Message>> {
    let listing = load.and_then(LoadState::listing)?;
    let active: Vec<&CapabilityItem> = listing.capabilities().active_items(preferences).collect();
    if active.is_empty() {
        return None;
    }
    let ty = palette.typography;
    let mut strip = Row::new().spacing(0).align_y(iced::Center);
    let mut details = Vec::with_capacity(active.len());
    for (idx, item) in active.into_iter().enumerate() {
        if idx > 0 {
            strip = strip.push(
                ty.small(" · ")
                    .color(palette.muted)
                    .wrapping(text::Wrapping::None),
            );
        }
        strip = strip.push(
            ty.small(item.prefix.clone())
                .color(palette.accent)
                .wrapping(text::Wrapping::None),
        );
        if let Some(value) = &item.value {
            strip = strip.push(
                ty.small(format!(":{value}"))
                    .color(palette.text)
                    .wrapping(text::Wrapping::None),
            );
        }
        details.push(item.detail.as_str());
    }
    let tooltip_text = details.join("\n");
    Some(
        tooltip(
            strip,
            container(ty.small(tooltip_text)).max_width(500),
            tooltip::Position::Top,
        )
        .delay(std::time::Duration::from_millis(450))
        .style(move |_| palette.tooltip())
        .into(),
    )
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
