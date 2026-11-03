//! Normal window bounds and startup fitting, without platform calls.

pub const MIN_WINDOW_WIDTH: u32 = 480;
pub const MIN_WINDOW_HEIGHT: u32 = 360;
pub const DEFAULT_WINDOW_WIDTH: u32 = 1120;
pub const DEFAULT_WINDOW_HEIGHT: u32 = 800;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowPlacement {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl WindowPlacement {
    pub fn valid(self) -> bool {
        self.width > 0 && self.height > 0
    }

    pub fn observed(x: f32, y: f32, width: f32, height: f32) -> Option<Self> {
        let [x, y, width, height] = [x, y, width, height].map(|value| f64::from(value).round());
        if ![x, y, width, height].iter().all(|value| value.is_finite())
            || x < f64::from(i32::MIN)
            || x > f64::from(i32::MAX)
            || y < f64::from(i32::MIN)
            || y > f64::from(i32::MAX)
            || width < 1.0
            || width > f64::from(u32::MAX)
            || height < 1.0
            || height > f64::from(u32::MAX)
        {
            return None;
        }
        Some(Self {
            x: x as i32,
            y: y as i32,
            width: width as u32,
            height: height as u32,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WindowRestoration {
    pub placement: WindowPlacement,
    pub adjusted: bool,
}

/// Connected display rectangles in the same logical desktop coordinates as windows.
#[derive(Debug, Clone, Copy)]
pub struct DisplayArea {
    pub frame: WindowPlacement,
    pub work: WindowPlacement,
}

pub fn restore_window(
    saved: Option<WindowPlacement>,
    displays: &[DisplayArea],
) -> Option<WindowRestoration> {
    let saved = saved.filter(|placement| placement.valid());
    if let Some(placement) = saved
        && placement.width >= MIN_WINDOW_WIDTH
        && placement.height >= MIN_WINDOW_HEIGHT
        && displays
            .iter()
            .any(|display| contains(display.frame, placement))
    {
        return Some(WindowRestoration {
            placement,
            adjusted: false,
        });
    }
    let desired = saved.unwrap_or(WindowPlacement {
        x: 0,
        y: 0,
        width: DEFAULT_WINDOW_WIDTH,
        height: DEFAULT_WINDOW_HEIGHT,
    });
    let work = displays
        .iter()
        .filter(|display| display.frame.valid() && display.work.valid())
        .max_by_key(|display| {
            (
                overlap(display.frame, desired),
                -distance(display.frame, desired),
            )
        })?
        .work;
    let width = desired
        .width
        .clamp(MIN_WINDOW_WIDTH, work.width.max(MIN_WINDOW_WIDTH));
    let height = desired
        .height
        .clamp(MIN_WINDOW_HEIGHT, work.height.max(MIN_WINDOW_HEIGHT));
    let placement = WindowPlacement {
        x: centered(work.x, work.width, width),
        y: centered(work.y, work.height, height),
        width,
        height,
    };
    Some(WindowRestoration {
        placement,
        adjusted: saved.is_some_and(|saved| saved != placement),
    })
}

fn contains(outer: WindowPlacement, inner: WindowPlacement) -> bool {
    outer.valid()
        && inner.x >= outer.x
        && inner.y >= outer.y
        && i64::from(inner.x) + i64::from(inner.width)
            <= i64::from(outer.x) + i64::from(outer.width)
        && i64::from(inner.y) + i64::from(inner.height)
            <= i64::from(outer.y) + i64::from(outer.height)
}

fn overlap(a: WindowPlacement, b: WindowPlacement) -> i128 {
    let axis = |a: i32, width: u32, b: i32, other: u32| {
        (i64::from(a) + i64::from(width)).min(i64::from(b) + i64::from(other)) - i64::from(a.max(b))
    };
    i128::from(axis(a.x, a.width, b.x, b.width).max(0))
        * i128::from(axis(a.y, a.height, b.y, b.height).max(0))
}

fn distance(a: WindowPlacement, b: WindowPlacement) -> i128 {
    let axis = |a: i32, width: u32, b: i32, other: u32| {
        (i128::from(a) - i128::from(b) - i128::from(other))
            .max(i128::from(b) - i128::from(a) - i128::from(width))
            .max(0)
    };
    let x = axis(a.x, a.width, b.x, b.width);
    let y = axis(a.y, a.height, b.y, b.height);
    x * x + y * y
}

fn centered(origin: i32, available: u32, desired: u32) -> i32 {
    (i64::from(origin) + i64::from(available.saturating_sub(desired)) / 2)
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}
