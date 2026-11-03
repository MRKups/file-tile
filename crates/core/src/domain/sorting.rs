//! Saved ordering preferences and pure native-filename comparison.
use std::{cmp::Ordering, ffi::OsStr};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SortField {
    #[default]
    Name,
    Modified,
    Size,
    Type,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SortDirection {
    #[default]
    Ascending,
    Descending,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortSettings {
    pub field: SortField,
    pub direction: SortDirection,
    pub folders_first: bool,
}
impl Default for SortSettings {
    fn default() -> Self {
        Self {
            field: SortField::Name,
            direction: SortDirection::Ascending,
            folders_first: true,
        }
    }
}
impl SortField {
    pub const ALL: [Self; 4] = [Self::Name, Self::Modified, Self::Size, Self::Type];
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Modified => "Date modified",
            Self::Size => "Size",
            Self::Type => "File type",
        }
    }
    pub fn order_label(self, direction: SortDirection) -> &'static str {
        match (self, direction) {
            (Self::Modified, SortDirection::Ascending) => "Oldest first",
            (Self::Modified, SortDirection::Descending) => "Newest first",
            (Self::Size, SortDirection::Ascending) => "Smallest first",
            (Self::Size, SortDirection::Descending) => "Largest first",
            (_, SortDirection::Ascending) => "A–Z",
            (_, SortDirection::Descending) => "Z–A",
        }
    }
}
impl SortDirection {
    pub fn apply(self, order: Ordering) -> Ordering {
        if self == Self::Descending {
            order.reverse()
        } else {
            order
        }
    }
}

pub fn compare_names(a: &OsStr, b: &OsStr) -> Ordering {
    natural(a.as_encoded_bytes(), b.as_encoded_bytes()).then_with(|| a.cmp(b))
}
fn natural(mut a: &[u8], mut b: &[u8]) -> Ordering {
    while let (Some(x), Some(y)) = (a.first(), b.first()) {
        let order = if x.is_ascii_digit() && y.is_ascii_digit() {
            let an = a.iter().take_while(|c| c.is_ascii_digit()).count();
            let bn = b.iter().take_while(|c| c.is_ascii_digit()).count();
            let av = &a[..an];
            let bv = &b[..bn];
            let av = &av[av.iter().take_while(|c| **c == b'0').count()..];
            let bv = &bv[bv.iter().take_while(|c| **c == b'0').count()..];
            a = &a[an..];
            b = &b[bn..];
            av.len().cmp(&bv.len()).then_with(|| av.cmp(bv))
        } else {
            let order = x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase());
            a = &a[1..];
            b = &b[1..];
            order
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a.len().cmp(&b.len())
}
