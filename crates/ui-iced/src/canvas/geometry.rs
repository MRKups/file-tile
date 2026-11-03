use iced::{Point, Rectangle, Size, Vector};

use crate::domain::{Cell, GridExtent, GridRect, Workspace};

use crate::canvas::workspace_header;

// Density-independent logical units. Iced applies display scaling, not this module.
// Window/layout extents must never enter the pitch calculation.
pub const PITCH: f32 = 76.0;
pub const GAP: f32 = 8.0;
pub const SIDE: f32 = 24.0;
pub const GUTTER: f32 = 18.0;

/// Matches the responsive header and canvas padding in workspace composition.
/// Both rendering and pointer routing use these logical coordinates.
#[derive(Debug, Clone, Copy)]
pub struct CanvasGeometry {
    pub typography: crate::window::typography::Style,
    pub viewport: Rectangle,
    pub extent: GridExtent,
    pub offset: Vector,
    pub header_height: f32,
    pub side: f32,
}

impl CanvasGeometry {
    pub fn new(size: Size, offset: Vector, workspace: &Workspace) -> Self {
        let side = if size.width <= 600.0 { 12.0 } else { SIDE };
        let header_height = workspace_header::height(size.width, workspace.mode());
        let viewport = Rectangle {
            x: side + GUTTER,
            y: header_height + GUTTER,
            width: (size.width - (side + GUTTER) * 2.0).max(1.0),
            height: (size.height - header_height - GUTTER * 2.0).max(1.0),
        };
        let columns = workspace
            .placements()
            .map(|item| item.rect.right())
            .max()
            .unwrap_or(0)
            .max(((viewport.width + GAP) / PITCH).floor() as u16);
        let rows = workspace
            .placements()
            .map(|item| item.rect.bottom())
            .max()
            .unwrap_or(0)
            .max(((viewport.height + GAP) / PITCH).floor() as u16);
        let mut geometry = Self {
            typography: Default::default(),
            viewport,
            extent: GridExtent::new(columns, rows),
            offset: Vector::ZERO,
            header_height,
            side,
        };
        let surface = geometry.size();
        // Iced 0.14 Scrollable clamps the offset and rounds its logical translation.
        // Match that transform so fractional wheel/scrollbar offsets cannot move a
        // pointer hit into the cell next to the one being painted.
        geometry.offset = Vector::new(
            offset
                .x
                .clamp(0.0, (surface.width - viewport.width).max(0.0))
                .round(),
            offset
                .y
                .clamp(0.0, (surface.height - viewport.height).max(0.0))
                .round(),
        );
        geometry
    }

    pub fn with_typography(
        mut self,
        preferences: crate::application::preferences::typography::TypographyPreferences,
    ) -> Self {
        self.typography = crate::window::typography::Style(preferences);
        self
    }

    pub fn cell(self, point: Point) -> Cell {
        let column = ((point.x - self.viewport.x + self.offset.x) / PITCH)
            .floor()
            .max(0.0) as u16;
        let row = ((point.y - self.viewport.y + self.offset.y) / PITCH)
            .floor()
            .max(0.0) as u16;
        self.extent.clamp(Cell::new(column, row))
    }

    pub fn can_start(self, point: Point) -> bool {
        let content_point = Point::new(
            point.x - self.viewport.x + self.offset.x,
            point.y - self.viewport.y + self.offset.y,
        );
        self.viewport.contains(point)
            && iced::Rectangle::with_size(self.size()).contains(content_point)
    }

    pub fn size(self) -> Size {
        Size::new(
            f32::from(self.extent.columns()) * PITCH - GAP,
            f32::from(self.extent.rows()) * PITCH - GAP,
        )
    }

    pub fn rect(self, rect: GridRect) -> Rectangle {
        Rectangle {
            x: f32::from(rect.origin.column) * PITCH,
            y: f32::from(rect.origin.row) * PITCH,
            width: f32::from(rect.width) * PITCH - GAP,
            height: f32::from(rect.height) * PITCH - GAP,
        }
    }
}
