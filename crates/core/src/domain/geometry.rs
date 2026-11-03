/// An integer canvas coordinate, independent of pixels and display density.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub column: u16,
    pub row: u16,
}

impl Cell {
    pub const fn new(column: u16, row: u16) -> Self {
        Self { column, row }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridExtent {
    columns: u16,
    rows: u16,
}

impl GridExtent {
    pub(super) const MAX_DIMENSION: u16 = 200;

    pub fn new(columns: u16, rows: u16) -> Self {
        Self {
            columns: columns.clamp(1, Self::MAX_DIMENSION),
            rows: rows.clamp(1, Self::MAX_DIMENSION),
        }
    }

    pub fn columns(self) -> u16 {
        self.columns
    }
    pub fn rows(self) -> u16 {
        self.rows
    }

    pub fn clamp(self, cell: Cell) -> Cell {
        Cell::new(
            cell.column.min(self.columns - 1),
            cell.row.min(self.rows - 1),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridRect {
    pub origin: Cell,
    pub width: u16,
    pub height: u16,
}

impl GridRect {
    /// Include both endpoint cells, regardless of drag direction.
    pub(super) fn between(first: Cell, last: Cell, extent: GridExtent) -> Self {
        let first = extent.clamp(first);
        let last = extent.clamp(last);
        Self {
            origin: Cell::new(first.column.min(last.column), first.row.min(last.row)),
            width: first.column.abs_diff(last.column) + 1,
            height: first.row.abs_diff(last.row) + 1,
        }
    }

    pub fn right(self) -> u16 {
        self.origin.column.saturating_add(self.width)
    }
    pub fn bottom(self) -> u16 {
        self.origin.row.saturating_add(self.height)
    }

    pub fn contains(self, cell: Cell) -> bool {
        cell.column >= self.origin.column
            && cell.column < self.right()
            && cell.row >= self.origin.row
            && cell.row < self.bottom()
    }

    pub(super) fn intersects(self, other: Self) -> bool {
        self.origin.column < other.right()
            && other.origin.column < self.right()
            && self.origin.row < other.bottom()
            && other.origin.row < self.bottom()
    }
}
