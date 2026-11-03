use std::ops::Range;

use iced::Element;
use iced::widget::{Column, Row, container, pin, space};

use crate::domain::{Cell, GridRect};

use crate::{
    Message,
    canvas::geometry::{CanvasGeometry, GAP, PITCH},
    window::appearance as a,
};

// Keep one neighboring cell on each side while scroll messages catch up.
const BUFFER: u16 = 1;

fn visible_range(offset: f32, length: f32, cells: u16) -> Range<u16> {
    let first = ((offset / PITCH).floor() as u16)
        .saturating_sub(BUFFER)
        .min(cells);
    let end = (((offset + length) / PITCH).ceil() as u16)
        .saturating_add(BUFFER)
        .min(cells);
    first..end.max(first)
}

pub fn view(
    geometry: CanvasGeometry,
    visible: bool,
    preview: Option<(GridRect, bool)>,
    palette: a::Palette,
) -> Element<'static, Message> {
    let size = geometry.size();
    if !visible {
        return container(space())
            .width(size.width)
            .height(size.height)
            .into();
    }
    let columns = visible_range(
        geometry.offset.x,
        geometry.viewport.width,
        geometry.extent.columns(),
    );
    let rows = visible_range(
        geometry.offset.y,
        geometry.viewport.height,
        geometry.extent.rows(),
    );
    let origin = (
        f32::from(columns.start) * PITCH,
        f32::from(rows.start) * PITCH,
    );
    let mut grid = Column::new().spacing(GAP);
    for row in rows {
        let mut line = Row::new().spacing(GAP);
        for column in columns.clone() {
            let selected = preview
                .filter(|(rect, _)| rect.contains(Cell::new(column, row)))
                .map(|(_, valid)| valid);
            line = line.push(
                container(space())
                    .width(PITCH - GAP)
                    .height(PITCH - GAP)
                    .style(move |_| {
                        let mut style = palette.grid_cell(selected == Some(true));
                        if selected == Some(false) {
                            style.background = Some(palette.grid_invalid.into());
                            style.border.color = palette.preview_invalid;
                        }
                        style
                    }),
            );
        }
        grid = grid.push(line);
    }
    // The full surface still owns scroll extent. Only its visible decorations
    // have widgets, pinned at their original absolute grid coordinates.
    container(pin(grid).x(origin.0).y(origin.1))
        .width(size.width)
        .height(size.height)
        .into()
}
