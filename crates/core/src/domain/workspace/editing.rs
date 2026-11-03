//! Pure layout gestures. Rectangles commit together, never while previewing.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridDelta {
    pub columns: i32,
    pub rows: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeHandle {
    NorthWest,
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
}

impl ResizeHandle {
    pub const ALL: [Self; 8] = [
        Self::NorthWest,
        Self::North,
        Self::NorthEast,
        Self::East,
        Self::SouthEast,
        Self::South,
        Self::SouthWest,
        Self::West,
    ];
    pub fn west(self) -> bool {
        matches!(self, Self::NorthWest | Self::West | Self::SouthWest)
    }
    pub fn east(self) -> bool {
        matches!(self, Self::NorthEast | Self::East | Self::SouthEast)
    }
    pub fn north(self) -> bool {
        matches!(self, Self::NorthWest | Self::North | Self::NorthEast)
    }
    pub fn south(self) -> bool {
        matches!(self, Self::SouthWest | Self::South | Self::SouthEast)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutKind {
    Move,
    Resize(ResizeHandle),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutPreview {
    pub id: BoxId,
    pub kind: LayoutKind,
    pub rect: GridRect,
    pub swap: Option<(BoxId, GridRect)>,
    pub valid: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct LayoutGesture {
    original: GridRect,
    extent: GridExtent,
    preview: LayoutPreview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutOutcome {
    None,
    Unchanged,
    Invalid,
    Changed,
}

impl Workspace {
    pub fn select_box(&mut self, id: Option<BoxId>) {
        if self.mode != Mode::Layout {
            return;
        }
        self.selected = id.filter(|id| self.item_rect(*id).is_some());
        self.gesture = None;
    }

    pub fn begin_layout(&mut self, id: BoxId, kind: LayoutKind, extent: GridExtent) {
        if self.mode != Mode::Layout {
            return;
        }
        let Some(rect) = self.item_rect(id) else {
            return;
        };
        if !fits(rect, extent) {
            return;
        }
        self.selected = Some(id);
        self.gesture = Some(Gesture::Layout(Box::new(LayoutGesture {
            original: rect,
            extent,
            preview: LayoutPreview {
                id,
                kind,
                rect,
                swap: None,
                valid: true,
            },
        })));
    }

    pub fn layout_preview(&self) -> Option<LayoutPreview> {
        match self.gesture.as_ref() {
            Some(Gesture::Layout(gesture)) => Some(gesture.preview),
            _ => None,
        }
    }

    pub fn layout_to(&mut self, delta: GridDelta) {
        let Some(Gesture::Layout(current)) = self.gesture.as_ref() else {
            return;
        };
        let mut gesture = **current;
        match gesture.preview.kind {
            LayoutKind::Move => {
                let x = i64::from(gesture.original.origin.column) + i64::from(delta.columns);
                let y = i64::from(gesture.original.origin.row) + i64::from(delta.rows);
                let rect = GridRect {
                    origin: Cell::new(
                        x.clamp(0, i64::from(GridExtent::MAX_DIMENSION)) as u16,
                        y.clamp(0, i64::from(GridExtent::MAX_DIMENSION)) as u16,
                    ),
                    ..gesture.original
                };
                let inside = x >= 0 && y >= 0 && fits(rect, gesture.extent);
                let replacement = inside
                    .then(|| {
                        self.placement(gesture.preview.id, rect, gesture.original, gesture.extent)
                    })
                    .flatten();
                gesture.preview.rect = rect;
                gesture.preview.valid = replacement.is_some();
                gesture.preview.swap = replacement.flatten();
            }
            LayoutKind::Resize(handle) => {
                gesture.preview.rect = self.resized(
                    gesture.preview.id,
                    gesture.original,
                    handle,
                    delta,
                    gesture.extent,
                );
                gesture.preview.valid = true;
                gesture.preview.swap = None;
            }
        }
        if let Some(Gesture::Layout(current)) = self.gesture.as_mut() {
            **current = gesture;
        }
    }

    pub fn finish_layout(&mut self) -> LayoutOutcome {
        if !matches!(self.gesture.as_ref(), Some(Gesture::Layout(_))) {
            return LayoutOutcome::None;
        }
        let Some(Gesture::Layout(gesture)) = self.gesture.take() else {
            return LayoutOutcome::None;
        };
        if !gesture.preview.valid {
            return LayoutOutcome::Invalid;
        }
        if gesture.preview.rect == gesture.original && gesture.preview.swap.is_none() {
            return LayoutOutcome::Unchanged;
        }
        self.remember();
        self.set_item_rect(gesture.preview.id, gesture.preview.rect);
        if let Some((id, rect)) = gesture.preview.swap {
            self.set_item_rect(id, rect);
        }
        LayoutOutcome::Changed
    }

    pub fn remove_box(&mut self, id: BoxId) -> bool {
        if !(self.mode == Mode::Layout || self.configuring(id)) || self.item_rect(id).is_none() {
            return false;
        }
        self.remember();
        self.boxes.retain(|item| item.id != id);
        self.launchers.retain(|item| item.id != id);
        self.selected = None;
        self.gesture = None;
        true
    }

    fn placement(
        &self,
        id: BoxId,
        target: GridRect,
        original: GridRect,
        extent: GridExtent,
    ) -> Option<Option<(BoxId, GridRect)>> {
        let mut hits = self
            .placements()
            .filter(|item| item.id != id && item.rect.intersects(target));
        let Some(hit) = hits.next() else {
            return Some(None);
        };
        if hits.next().is_some() {
            return None;
        }
        let replacement = GridRect {
            origin: original.origin,
            ..hit.rect
        };
        if !fits(replacement, extent)
            || target.intersects(replacement)
            || self
                .placements()
                .any(|item| item.id != id && item.id != hit.id && item.rect.intersects(replacement))
        {
            return None;
        }
        Some(Some((hit.id, replacement)))
    }

    fn resized(
        &self,
        id: BoxId,
        original: GridRect,
        handle: ResizeHandle,
        delta: GridDelta,
        extent: GridExtent,
    ) -> GridRect {
        let Some(kind) = self.item_kind(id) else {
            return original;
        };
        let start = [
            original.origin.column,
            original.origin.row,
            original.right(),
            original.bottom(),
        ]
        .map(i64::from);
        let target = shape::resize_target(kind, original, handle, delta, extent);
        let steps = (0..4)
            .map(|index| (target[index] - start[index]).abs())
            .max()
            .unwrap_or(0);
        let mut result = original;
        // At most 200 steps. Walk each snapped boundary, not only the endpoint.
        for step in 1..=steps {
            let bounds: [i64; 4] = std::array::from_fn(|index| {
                start[index]
                    + (((target[index] - start[index]) * step) as f64 / steps as f64).round() as i64
            });
            let next = GridRect {
                origin: Cell::new(bounds[0] as u16, bounds[1] as u16),
                width: (bounds[2] - bounds[0]) as u16,
                height: (bounds[3] - bounds[1]) as u16,
            };
            if !shape::allows_resize(kind, result, next) {
                break;
            }
            if self
                .placements()
                .any(|item| item.id != id && item.rect.intersects(next))
            {
                break;
            }
            result = next;
        }
        result
    }
}

fn fits(rect: GridRect, extent: GridExtent) -> bool {
    rect.width > 0
        && rect.height > 0
        && u32::from(rect.origin.column) + u32::from(rect.width) <= u32::from(extent.columns())
        && u32::from(rect.origin.row) + u32::from(rect.height) <= u32::from(extent.rows())
}
