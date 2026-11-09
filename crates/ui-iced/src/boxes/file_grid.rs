//! Row-windowed wrapping tiles. Width changes never resize the workspace boxes.
use crate::{
    Message,
    boxes::{
        entry_motion,
        file_list::{Props, RowKey},
    },
    window::icons,
};
use iced::widget::{column, container, mouse_area, row, space, text};
use iced::{Element, Fill};
pub(crate) const TILE_HEIGHT: f32 = 96.0;
const LABEL_LINE_HEIGHT: f32 = 1.25;
pub(crate) fn columns(width: f32, ty: crate::window::typography::Style) -> usize {
    ((width - 18.0).max(84.0) / ty.row(92.0)).floor().max(1.0) as usize
}

pub(crate) fn entry_vertical_bounds(
    display_index: usize,
    content_width: f32,
    listing: &crate::application::contracts::directory::DirectoryListing,
    order: Option<&[usize]>,
    show_hidden: bool,
    motion: Option<&entry_motion::ViewMotion>,
    ty: crate::window::typography::Style,
) -> (f32, f32, f32) {
    let count = listing.display_len(show_hidden);
    let cols = columns(content_width, ty);
    let tile_h = ty.row(TILE_HEIGHT);
    if let Some(motion) = motion.filter(|item| item.has_deletions()) {
        let slots = motion.project_slots(listing, order, show_hidden);
        let slot_idx = slots
            .iter()
            .position(|slot| {
                matches!(slot, entry_motion::Slot::Live { index, .. } if *index == display_index)
            })
            .unwrap_or(display_index);
        let row_idx = slot_idx / cols;
        let total_rows = slots.len().div_ceil(cols);
        return (
            row_idx as f32 * tile_h,
            (row_idx + 1) as f32 * tile_h,
            total_rows as f32 * tile_h,
        );
    }
    let row_idx = display_index / cols;
    let total_rows = count.div_ceil(cols);
    (
        row_idx as f32 * tile_h,
        (row_idx + 1) as f32 * tile_h,
        total_rows as f32 * tile_h,
    )
}
pub(crate) fn range(count: usize, props: Props<'_>) -> std::ops::Range<usize> {
    let columns = columns(props.width, props.palette.typography);
    let rows = count.div_ceil(columns);
    let visible = crate::boxes::file_list::range_with_height(
        rows,
        props.window,
        props.palette.typography.row(TILE_HEIGHT),
    );
    visible.start.saturating_mul(columns).min(count)..visible.end.saturating_mul(columns).min(count)
}
const OFF_SCREEN_BUFFER_ROWS: usize = 2;

fn demand_rows(
    total_rows: usize,
    window: crate::boxes::file_list::ScrollWindow,
    tile_height: f32,
) -> impl Iterator<Item = usize> {
    let on_screen = crate::boxes::file_list::unbuffered_range(total_rows, window, tile_height);
    let below_end = on_screen
        .end
        .saturating_add(OFF_SCREEN_BUFFER_ROWS)
        .min(total_rows);
    let above_start = on_screen.start.saturating_sub(OFF_SCREEN_BUFFER_ROWS);
    on_screen
        .clone()
        .chain(on_screen.end..below_end)
        .chain(above_start..on_screen.start)
}

pub(crate) fn content(props: Props<'_>) -> Element<'_, Message> {
    let Some(listing) = props.load.and_then(crate::application::LoadState::listing) else {
        return crate::boxes::file_list::empty(
            "No contents loaded.",
            props.view,
            props.interactive,
            props.palette,
        );
    };
    let count = listing.display_len(props.show_hidden);
    let active_deletions = props.motion.filter(|motion| motion.has_deletions());
    if count == 0 && active_deletions.is_none() {
        return crate::boxes::file_list::empty_listing(listing, props);
    }
    let ty = props.palette.typography;
    let tile_height = ty.row(TILE_HEIGHT);
    let columns = columns(props.width, ty);
    let tile_width = ((props.width - 18.0 - 6.0 * (columns - 1) as f32) / columns as f32).max(1.0);
    let visible_columns = column_range(props, tile_width + 6.0, columns);
    if let Some(motion) = active_deletions {
        return content_with_deletions(
            props,
            listing,
            motion,
            columns,
            tile_width,
            tile_height,
            visible_columns,
        );
    }
    let range = range(count, props);
    let visible_rows = demand_rows(count.div_ceil(columns), props.window, tile_height);
    let indices = visible_rows
        .flat_map(|index| {
            visible_columns
                .clone()
                .map(move |column| index * columns + column)
        })
        .filter(|index| *index < count);
    let demand = crate::boxes::file_list::metadata_demand(props, listing, indices);
    let mut children: Vec<(RowKey, Element<'_, Message>)> = vec![(
        RowKey::Before,
        space()
            .height((range.start / columns) as f32 * tile_height)
            .into(),
    )];
    for start in (range.start..range.end).step_by(columns) {
        children.push((
            RowKey::Grid(
                props.revision,
                props.order_revision,
                props.show_hidden,
                columns,
                start,
                visible_columns.start,
            ),
            grid_row(
                props,
                listing,
                start,
                columns,
                visible_columns.clone(),
                tile_width,
            ),
        ));
    }
    children.push((
        RowKey::After,
        space()
            .height(
                count
                    .div_ceil(columns)
                    .saturating_sub(range.end.div_ceil(columns)) as f32
                    * tile_height,
            )
            .into(),
    ));
    crate::boxes::file_list::keyed_rows(props, children, demand)
}

fn content_with_deletions<'a>(
    props: Props<'a>,
    listing: &'a crate::application::contracts::directory::DirectoryListing,
    motion: &'a entry_motion::ViewMotion,
    columns: usize,
    tile_width: f32,
    tile_height: f32,
    visible_columns: std::ops::Range<usize>,
) -> Element<'a, Message> {
    let slots = motion.project_slots(listing, props.order, props.show_hidden);
    if slots.is_empty() {
        return crate::boxes::file_list::empty_listing(listing, props);
    }
    let total = slots.len();
    let range = range(total, props);
    let visible_rows = demand_rows(total.div_ceil(columns), props.window, tile_height);
    let live_indices = visible_rows
        .flat_map(|row_idx| {
            visible_columns
                .clone()
                .map(move |column| row_idx * columns + column)
        })
        .filter_map(|slot_idx| match slots.get(slot_idx) {
            Some(entry_motion::Slot::Live { index, .. }) => Some(*index),
            _ => None,
        });
    let demand = crate::boxes::file_list::metadata_demand(props, listing, live_indices);
    let mut children: Vec<(RowKey, Element<'_, Message>)> = vec![(
        RowKey::Before,
        space()
            .height((range.start / columns) as f32 * tile_height)
            .into(),
    )];
    for start in (range.start..range.end).step_by(columns) {
        children.push((
            RowKey::GridMotion(
                props.revision,
                props.order_revision,
                props.show_hidden,
                columns,
                start,
                visible_columns.start,
            ),
            grid_row_slots(
                props,
                listing,
                &slots,
                start,
                columns,
                visible_columns.clone(),
                tile_width,
            ),
        ));
    }
    children.push((
        RowKey::After,
        space()
            .height(
                total
                    .div_ceil(columns)
                    .saturating_sub(range.end.div_ceil(columns)) as f32
                    * tile_height,
            )
            .into(),
    ));
    crate::boxes::file_list::keyed_rows(props, children, demand)
}

fn column_range(props: Props<'_>, pitch: f32, columns: usize) -> std::ops::Range<usize> {
    if props.clipped_width <= 0.0 {
        return 0..0;
    }
    let first = ((props.clipped_left - 9.0).max(0.0) / pitch).floor() as usize;
    let last = ((props.clipped_left + props.clipped_width - 9.0).max(0.0) / pitch).ceil() as usize;
    first.min(columns)..last.min(columns)
}

fn grid_row<'a>(
    props: Props<'a>,
    listing: &'a crate::application::contracts::directory::DirectoryListing,
    start: usize,
    columns: usize,
    visible: std::ops::Range<usize>,
    tile_width: f32,
) -> Element<'a, Message> {
    let count = listing.display_len(props.show_hidden);
    let mut tiles = row![space().width(visible.start as f32 * (tile_width + 6.0))];
    for index in (start + visible.start).min(count)..(start + visible.end).min(count) {
        if let Some(entry) = props.entry(listing, index) {
            tiles = tiles.push(container(tile(props, listing, entry, tile_width)).padding(
                iced::Padding {
                    right: if index % columns + 1 == columns {
                        0.0
                    } else {
                        6.0
                    },
                    ..iced::Padding::ZERO
                },
            ));
        }
    }
    tiles
        .push(
            space().width(
                ((columns - visible.end) as f32 * (tile_width + 6.0)
                    - if visible.end < columns { 6.0 } else { 0.0 })
                .max(0.0),
            ),
        )
        .height(props.palette.typography.row(TILE_HEIGHT))
        .into()
}
fn grid_row_slots<'a>(
    props: Props<'a>,
    listing: &'a crate::application::contracts::directory::DirectoryListing,
    slots: &[entry_motion::Slot<'a>],
    start: usize,
    columns: usize,
    visible: std::ops::Range<usize>,
    tile_width: f32,
) -> Element<'a, Message> {
    let total = slots.len();
    let tile_height = props.palette.typography.row(TILE_HEIGHT);
    let mut tiles = row![space().width(visible.start as f32 * (tile_width + 6.0))];
    for slot_idx in (start + visible.start).min(total)..(start + visible.end).min(total) {
        if let Some(slot) = slots.get(slot_idx) {
            let cell = match *slot {
                entry_motion::Slot::Live { entry, .. } => tile(props, listing, entry, tile_width),
                entry_motion::Slot::Ghost {
                    entry,
                    tone,
                    presence,
                    tint,
                    ..
                } => entry_motion::ghost_tile(
                    entry,
                    tone,
                    presence,
                    tint,
                    tile_width,
                    tile_height,
                    props.palette,
                ),
            };
            tiles = tiles.push(container(cell).padding(iced::Padding {
                right: if slot_idx % columns + 1 == columns {
                    0.0
                } else {
                    6.0
                },
                ..iced::Padding::ZERO
            }));
        }
    }
    tiles
        .push(
            space().width(
                ((columns - visible.end) as f32 * (tile_width + 6.0)
                    - if visible.end < columns { 6.0 } else { 0.0 })
                .max(0.0),
            ),
        )
        .height(tile_height)
        .into()
}

fn tile<'a>(
    props: Props<'a>,
    listing: &'a crate::application::contracts::directory::DirectoryListing,
    entry: &'a crate::application::contracts::directory::DirectoryEntry,
    width: f32,
) -> Element<'a, Message> {
    let permitted = crate::boxes::file_list::entry_allowed(props, listing, entry);
    let palette = props.palette;
    let ty = palette.typography;
    let name = entry.name.to_string_lossy();
    let arrival = props.motion.and_then(|motion| motion.arrival(&entry.path));
    let drag_dim = if props
        .dragging
        .is_some_and(|paths| paths.contains(&entry.path))
    {
        0.45
    } else {
        1.0
    };
    let icon_alpha = (arrival.map_or(1.0, |v| v.icon_alpha) * drag_dim).clamp(0.0, 1.0);
    let label_alpha = (arrival.map_or(1.0, |v| v.label_alpha) * drag_dim).clamp(0.0, 1.0);
    let rise_y = arrival.map_or(0.0, |v| v.row_offset).max(0.0) * 16.0;
    let label_slide_x = arrival.map_or(0.0, |v| v.label_offset).max(0.0) * 16.0;
    let muted = palette.muted.scale_alpha(label_alpha);
    let color = if permitted {
        palette.text.scale_alpha(label_alpha)
    } else {
        muted
    };
    let contents = column![
        tile_artwork(props, entry, icon_alpha),
        tile_label(ty, name.as_ref(), width, color, label_slide_x)
    ]
    .spacing(8)
    .align_x(iced::Center);
    let selected = props
        .selection
        .is_some_and(|selection| selection.contains(&entry.path))
        || props.selected == Some(entry.path.as_path());
    let hovered = props.hovered == Some(entry.path.as_path());
    let panel = container(contents)
        .padding(iced::Padding {
            top: rise_y,
            right: 5.0,
            bottom: 0.0,
            left: 5.0,
        })
        .width(width)
        .height(ty.row(TILE_HEIGHT) - 6.0)
        .align_y(iced::Center)
        .clip(true)
        .style(move |_| {
            dim_tile_style(
                entry_motion::tile_style(
                    palette.entry_panel(selected, hovered, props.interactive),
                    arrival,
                    palette,
                ),
                drag_dim,
            )
        });
    let mut target = mouse_area(panel);
    if props.interactive && permitted {
        target = target
            .on_press(Message::EntrySelected(props.view, entry.path.clone()))
            .on_double_click(Message::EntryOpened(props.view, entry.path.clone()))
            .on_enter(Message::EntryHovered(props.view, entry.path.clone(), true))
            .on_exit(Message::EntryHovered(props.view, entry.path.clone(), false))
            .interaction(iced::mouse::Interaction::Pointer);
    }
    let target: Element<'_, Message> = if props.interactive {
        crate::menus::entry_menu::target(props.view, entry.path.clone(), target.into(), palette)
    } else {
        target.into()
    };
    crate::runtime::metadata::hover_tooltip(target, props, Some(listing), entry)
}

fn dim_tile_style(mut style: container::Style, drag_dim: f32) -> container::Style {
    if drag_dim >= 0.999 {
        return style;
    }
    if let Some(iced::Background::Color(color)) = style.background {
        style.background = Some(color.scale_alpha(drag_dim).into());
    }
    style.border.color = style.border.color.scale_alpha(drag_dim);
    style
}

pub(crate) fn tile_label(
    ty: crate::window::typography::Style,
    name: &str,
    tile_width: f32,
    color: iced::Color,
    slide_x: f32,
) -> Element<'static, Message> {
    let max_width = (tile_width - 12.0).max(1.0);
    let formatted = format_tile_name(name, ty, max_width);
    let two_line_height =
        (ty.size(crate::window::typography::SMALL_SIZE) * LABEL_LINE_HEIGHT * 2.0).ceil() + 2.0;
    container(
        ty.small(formatted)
            .color(color)
            .line_height(LABEL_LINE_HEIGHT)
            .wrapping(text::Wrapping::None)
            .width(Fill)
            .align_x(iced::Center),
    )
    .width(Fill)
    .height(two_line_height)
    .align_y(iced::alignment::Vertical::Top)
    .clip(true)
    .padding(iced::Padding {
        left: slide_x,
        ..iced::Padding::ZERO
    })
    .into()
}

fn format_tile_name(name: &str, ty: crate::window::typography::Style, max_width: f32) -> String {
    if name.is_empty() {
        return String::new();
    }
    let clean: std::borrow::Cow<'_, str> = if name.contains(['\n', '\r', '\t']) {
        std::borrow::Cow::Owned(
            name.chars()
                .map(|ch| {
                    if matches!(ch, '\n' | '\r' | '\t') {
                        ' '
                    } else {
                        ch
                    }
                })
                .collect(),
        )
    } else {
        std::borrow::Cow::Borrowed(name)
    };
    use iced::advanced::graphics::text as gfx_text;
    let Ok(mut font_system) = gfx_text::font_system().write() else {
        return clean.into_owned();
    };
    let font_size = ty.size(crate::window::typography::SMALL_SIZE);
    let attrs = gfx_text::to_attributes(ty.font());
    let shaping = iced::widget::text::Shaping::default();
    let mut buffer = gfx_text::cosmic_text::Buffer::new(
        font_system.raw(),
        gfx_text::cosmic_text::Metrics::new(font_size, font_size * LABEL_LINE_HEIGHT),
    );
    buffer.set_wrap(font_system.raw(), gfx_text::cosmic_text::Wrap::None);
    buffer.set_size(font_system.raw(), None, None);
    let mut fits = |candidate: &str| -> bool {
        if candidate.is_empty() {
            return true;
        }
        buffer.set_text(
            font_system.raw(),
            candidate,
            &attrs,
            gfx_text::to_shaping(shaping, candidate),
            None,
        );
        gfx_text::measure(&buffer).0.width <= max_width
    };
    wrap_two_lines(clean.as_ref(), &mut fits)
}

fn wrap_two_lines(clean: &str, fits: &mut impl FnMut(&str) -> bool) -> String {
    if fits(clean) {
        return clean.to_owned();
    }
    let ends: Vec<usize> = clean
        .char_indices()
        .skip(1)
        .map(|(idx, _)| idx)
        .chain(std::iter::once(clean.len()))
        .collect();
    let mut lo = 0usize;
    let mut hi = ends.len();
    while lo + 1 < hi {
        let mid = lo + (hi - lo) / 2;
        if fits(clean[..ends[mid]].trim_end_matches(' ')) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let max_end = ends[lo];
    let prefix = &clean[..max_end];
    let (line1, rest) = if let Some(space) = prefix
        .rfind(' ')
        .filter(|&idx| clean[..idx].chars().any(|ch| ch != ' '))
    {
        (
            clean[..space].trim_end_matches(' '),
            clean[space..].trim_start_matches(' '),
        )
    } else if let Some(sep) = prefix
        .rmatch_indices(['-', '_', '.'])
        .find(|&(idx, matched)| {
            idx + matched.len() < clean.len() && clean[..idx].chars().count() >= 2
        })
        .map(|(idx, matched)| idx + matched.len())
    {
        (&clean[..sep], clean[sep..].trim_start_matches(' '))
    } else {
        (&clean[..max_end], clean[max_end..].trim_start_matches(' '))
    };
    if rest.is_empty() {
        return line1.to_owned();
    }
    let line2 = ellipsize_line(rest, fits);
    format!("{line1}\n{line2}")
}

fn ellipsize_line(rest: &str, fits: &mut impl FnMut(&str) -> bool) -> String {
    if fits(rest) {
        return rest.to_owned();
    }
    if let Some((stem, ext)) = split_short_extension(rest)
        && let Some(min_prefix) = char_prefix(stem, 3)
        && !min_prefix.trim_end_matches(' ').is_empty()
        && fits(&format!("{}…{ext}", min_prefix.trim_end_matches(' ')))
    {
        let head = longest_fitting_prefix(stem, |prefix| {
            let trimmed = prefix.trim_end_matches(' ');
            !trimmed.is_empty() && fits(&format!("{trimmed}…{ext}"))
        });
        let trimmed = head.trim_end_matches(' ');
        if !trimmed.is_empty() {
            return format!("{trimmed}…{ext}");
        }
    }
    let head = longest_fitting_prefix(rest, |prefix| {
        let trimmed = prefix.trim_end_matches(' ');
        !trimmed.is_empty() && fits(&format!("{trimmed}…"))
    });
    let trimmed = head.trim_end_matches(' ');
    if trimmed.is_empty() {
        "…".to_owned()
    } else {
        format!("{trimmed}…")
    }
}

fn split_short_extension(text: &str) -> Option<(&str, &str)> {
    let dot = text.rfind('.')?;
    if dot == 0 {
        return None;
    }
    let ext = &text[dot + 1..];
    if (1..=5).contains(&ext.len()) && ext.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        Some((&text[..dot], &text[dot..]))
    } else {
        None
    }
}

fn char_prefix(text: &str, count: usize) -> Option<&str> {
    if count == 0 {
        return Some("");
    }
    let end = text.char_indices().nth(count).map_or_else(
        || (text.chars().count() == count).then_some(text.len()),
        |(idx, _)| Some(idx),
    )?;
    Some(&text[..end])
}

fn longest_fitting_prefix(text: &str, mut ok: impl FnMut(&str) -> bool) -> &str {
    let ends: Vec<usize> = text
        .char_indices()
        .skip(1)
        .map(|(idx, _)| idx)
        .chain(std::iter::once(text.len()))
        .collect();
    if ends.is_empty() || !ok(&text[..ends[0]]) {
        return "";
    }
    let mut lo = 0usize;
    let mut hi = ends.len();
    while lo + 1 < hi {
        let mid = lo + (hi - lo) / 2;
        if ok(&text[..ends[mid]]) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    &text[..ends[lo]]
}

fn tile_artwork<'a>(
    props: Props<'a>,
    entry: &'a crate::application::contracts::directory::DirectoryEntry,
    alpha: f32,
) -> Element<'a, Message> {
    let icon = icons::Icon::for_entry(entry);
    crate::runtime::thumbnails::image_with_opacity(
        props.thumbnails,
        props.view,
        props.revision,
        &entry.path,
        32.0,
        alpha,
    )
    .unwrap_or_else(|| icons::view(icon, 32.0, Some(props.palette.icon(icon)), alpha).into())
}
