//! Versioned sort values, independent of presentation and worker state.
use crate::domain::sorting::{SortDirection, SortField, SortSettings};
use serde::{Deserialize, Serialize};
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SortRecord {
    field: Field,
    direction: Direction,
    folders_first: bool,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Field {
    Name,
    Modified,
    Size,
    Type,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Direction {
    Ascending,
    Descending,
}
impl From<SortSettings> for SortRecord {
    fn from(value: SortSettings) -> Self {
        Self {
            field: match value.field {
                SortField::Name => Field::Name,
                SortField::Modified => Field::Modified,
                SortField::Size => Field::Size,
                SortField::Type => Field::Type,
            },
            direction: match value.direction {
                SortDirection::Ascending => Direction::Ascending,
                SortDirection::Descending => Direction::Descending,
            },
            folders_first: value.folders_first,
        }
    }
}
impl From<SortRecord> for SortSettings {
    fn from(value: SortRecord) -> Self {
        Self {
            field: match value.field {
                Field::Name => SortField::Name,
                Field::Modified => SortField::Modified,
                Field::Size => SortField::Size,
                Field::Type => SortField::Type,
            },
            direction: match value.direction {
                Direction::Ascending => SortDirection::Ascending,
                Direction::Descending => SortDirection::Descending,
            },
            folders_first: value.folders_first,
        }
    }
}
