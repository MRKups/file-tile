//! Pure projection construction from already collected metadata. No I/O or widgets.
use super::{SortRequest, SortedEntries};
use crate::{
    application::{contracts::directory::EntryKind, files::metadata::FileMetadata},
    domain::sorting::{SortDirection, SortField, compare_names},
};
use std::cmp::Ordering;

pub(crate) struct SortValue {
    pub index: usize,
    pub metadata: Option<FileMetadata>,
}
pub(crate) fn order(request: &SortRequest, values: Vec<SortValue>) -> SortedEntries {
    let settings = request.settings;
    let entries = request.listing.entries();
    let unavailable = values
        .iter()
        .filter(|value| match settings.field {
            SortField::Modified => value
                .metadata
                .as_ref()
                .is_none_or(|value| value.modified.is_err()),
            SortField::Size => value.metadata.as_ref().is_none_or(|metadata| {
                metadata.bytes.is_none() && entries[value.index].kind != EntryKind::Directory
            }),
            _ => false,
        })
        .count();
    let mut rows: Vec<_> = values
        .into_iter()
        .map(|value| {
            let extension = if settings.field == SortField::Type {
                entries[value.index]
                    .path
                    .extension()
                    .map(|ext| ext.as_encoded_bytes().to_ascii_lowercase())
                    .unwrap_or_default()
            } else {
                vec![]
            };
            (value, extension)
        })
        .collect();
    rows.sort_by(|(a, ax), (b, bx)| {
        let ae = &entries[a.index];
        let be = &entries[b.index];
        let folders = if settings.folders_first {
            (ae.kind != EntryKind::Directory).cmp(&(be.kind != EntryKind::Directory))
        } else {
            Ordering::Equal
        };
        folders
            .then_with(|| match settings.field {
                SortField::Name => settings.direction.apply(compare_names(&ae.name, &be.name)),
                SortField::Modified => optional(
                    a.metadata.as_ref().and_then(|m| m.modified.as_ref().ok()),
                    b.metadata.as_ref().and_then(|m| m.modified.as_ref().ok()),
                    settings.direction,
                ),
                SortField::Size => optional(
                    a.metadata.as_ref().and_then(|m| m.bytes),
                    b.metadata.as_ref().and_then(|m| m.bytes),
                    settings.direction,
                ),
                SortField::Type => settings.direction.apply(ax.cmp(bx)),
            })
            .then_with(|| compare_names(&ae.name, &be.name))
            .then_with(|| ae.path.cmp(&be.path))
    });
    SortedEntries {
        indices: rows.into_iter().map(|(value, _)| value.index).collect(),
        unavailable,
    }
}
fn optional<T: Ord>(a: Option<T>, b: Option<T>, direction: SortDirection) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => direction.apply(a.cmp(&b)),
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (None, None) => Ordering::Equal,
    }
}
