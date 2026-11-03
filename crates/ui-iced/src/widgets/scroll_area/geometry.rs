//! One geometry for painting, hit testing, and mapping bounded thumbs to offsets.
use crate::window::appearance::SCROLLBAR_WIDTH;
use iced::{Point, Rectangle, Vector, widget::scrollable::Direction};

#[derive(Clone, Copy, Default)]
pub(super) struct Sample {
    pub bounds: Rectangle,
    pub content: Rectangle,
    pub offset: Vector,
}

#[derive(Clone, Copy)]
pub(super) struct Rail {
    pub target: Rectangle,
    pub thumb: Rectangle,
    pub horizontal: bool,
    pub extent: f32,
}

impl Rail {
    pub fn coordinate(self, point: Point) -> f32 {
        if self.horizontal { point.x } else { point.y }
    }

    pub fn start(self) -> f32 {
        if self.horizontal {
            self.target.x
        } else {
            self.target.y
        }
    }

    pub fn thumb_start(self) -> f32 {
        if self.horizontal {
            self.thumb.x
        } else {
            self.thumb.y
        }
    }

    pub fn length(self) -> f32 {
        if self.horizontal {
            self.thumb.width
        } else {
            self.thumb.height
        }
    }

    pub fn travel(self) -> f32 {
        (if self.horizontal {
            self.target.width
        } else {
            self.target.height
        } - self.length())
        .max(0.0)
    }

    pub fn offset(self, point: Point, grabbed: f32) -> f32 {
        let fraction = if self.travel() > 0.0 {
            ((self.coordinate(point) - self.start() - grabbed * self.length()) / self.travel())
                .clamp(0.0, 1.0)
        } else {
            0.0
        };
        fraction * self.extent
    }
}

impl Sample {
    pub fn rails(self, direction: Direction) -> Vec<Rail> {
        let x = direction.horizontal().is_some() && self.content.width > self.bounds.width;
        let y = direction.vertical().is_some() && self.content.height > self.bounds.height;
        let mut rails = Vec::with_capacity(2);
        for horizontal in [false, true] {
            if !(if horizontal { x } else { y }) {
                continue;
            }
            let width = SCROLLBAR_WIDTH;
            let (length, visible, content, offset) = if horizontal {
                (
                    (self.bounds.width - if y { width } else { 0.0 }).max(0.0),
                    self.bounds.width,
                    self.content.width,
                    self.offset.x,
                )
            } else {
                (
                    (self.bounds.height - if x { width } else { 0.0 }).max(0.0),
                    self.bounds.height,
                    self.content.height,
                    self.offset.y,
                )
            };
            if length <= 0.0 {
                continue;
            }
            // Leave travel on tiny viewports too. The same bounded geometry owns
            // drawing and dragging, not a decorative enlargement of Iced's thumb.
            let maximum = 160.0_f32.min(length * 0.8);
            let thumb = (length * visible / content).clamp(28.0_f32.min(maximum), maximum);
            let extent = (content - visible).max(0.0);
            let position = (length - thumb) * (offset / extent).clamp(0.0, 1.0);
            let target = if horizontal {
                Rectangle {
                    x: self.bounds.x,
                    y: self.bounds.y + self.bounds.height - width,
                    width: length,
                    height: width,
                }
            } else {
                Rectangle {
                    x: self.bounds.x + self.bounds.width - width,
                    y: self.bounds.y,
                    width,
                    height: length,
                }
            };
            let thumb = if horizontal {
                Rectangle {
                    x: target.x + position,
                    y: target.y + 2.5,
                    width: thumb,
                    height: 5.0,
                }
            } else {
                Rectangle {
                    x: target.x + 2.5,
                    y: target.y + position,
                    width: 5.0,
                    height: thumb,
                }
            };
            rails.push(Rail {
                target,
                thumb,
                horizontal,
                extent,
            });
        }
        rails
    }
}
