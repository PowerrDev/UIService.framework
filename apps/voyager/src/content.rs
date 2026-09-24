//! The listing itself: list view, icon (grid) view, row/tile geometry and hit
//! testing.
//!
//! List view follows Finder's: a header row with column titles over a hairline,
//! alternating white and pale rows that carry on down the empty part of the
//! window, a file icon and name in the first column, kind and right-aligned
//! size after it, and a disclosure chevron on folders. A selected row is a
//! rounded accent-blue bar with white text in every column. Icon view puts a 48 pt icon over
//! its name; the selected one gets a soft plate behind the icon and an accent
//! pill behind the name.
//!
//! Rows and tiles do not change under a plain hover (Finder's do not), so only
//! selection and the press that precedes it repaint anything here.

use ui::prelude::*;

use crate::draw2d;
use crate::icons;
use crate::model::{format_size, Entry};
use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    List,
    Grid,
}

// ---- list geometry ---------------------------------------------------------

fn header_height() -> u32 {
    scale::pt(24)
}
fn list_icon_size() -> u32 {
    scale::pt(16)
}
fn kind_column_width() -> u32 {
    scale::pt(96)
}
fn size_column_width() -> u32 {
    scale::pt(60)
}
/// The Kind column drops out below this content width so Name keeps room.
fn kind_column_min_content_width() -> u32 {
    scale::pt(520)
}
fn disclosure_size() -> u32 {
    scale::pt(10)
}

/// Where each list column sits for a content area of a given width.
#[derive(Clone, Copy)]
struct Columns {
    name_x: i32,
    name_right: i32,
    /// `None` when the window is too narrow for a Kind column.
    kind_x: Option<i32>,
    /// The right edge the size text is aligned to.
    size_right: i32,
    disclosure_x: i32,
}

impl Columns {
    fn compute(content: Rect) -> Self {
        let margin = theme::margin();
        let right_edge = content.origin.x + content.size.width as i32 - margin;
        let disclosure_x = right_edge - disclosure_size() as i32;
        let size_right = disclosure_x - scale::pt_i32(10);
        let size_left = size_right - size_column_width() as i32;
        let gap = scale::pt_i32(14);

        let (kind_x, name_right) = if content.size.width >= kind_column_min_content_width() {
            let kind_x = size_left - gap - kind_column_width() as i32;
            (Some(kind_x), kind_x - gap)
        } else {
            (None, size_left - gap)
        };

        Self {
            name_x: content.origin.x + margin,
            name_right,
            kind_x,
            size_right,
            disclosure_x,
        }
    }
}

/// The full-width band of list row `index`: what a click lands on.
pub fn row_rect(content: Rect, index: usize) -> Rect {
    let row_height = theme::row_height();
    Rect::new(
        content.origin.x,
        content.origin.y + header_height() as i32 + (index as i32) * row_height as i32,
        content.size.width,
        row_height,
    )
}

/// The rounded bar a selected row shows, inset from its band.
fn selection_rect(row: Rect) -> Rect {
    let side = scale::pt_i32(10);
    let vertical = scale::pt_i32(1);
    Rect::new(
        row.origin.x + side,
        row.origin.y + vertical,
        row.size.width.saturating_sub(side as u32 * 2),
        row.size.height.saturating_sub(vertical as u32 * 2),
    )
}

// ---- grid geometry ---------------------------------------------------------

// Sized so three tiles fit the default window's content area next to the
// sidebar (about 300 pt).
fn tile_width() -> u32 {
    scale::pt(88)
}
fn tile_height() -> u32 {
    scale::pt(88)
}
fn tile_gap() -> i32 {
    scale::pt_i32(4)
}
fn grid_padding() -> i32 {
    scale::pt_i32(12)
}
fn plate_size() -> u32 {
    scale::pt(54)
}
fn grid_icon_size() -> u32 {
    scale::pt(44)
}

/// Number of grid columns that fit `content_width`; always at least one so a
/// window narrower than a tile still lays out (just overlapping the right
/// edge) instead of dividing by zero.
pub fn grid_columns(content_width: u32) -> usize {
    let gap = tile_gap();
    let usable = content_width as i32 - grid_padding() * 2 + gap;
    let column = tile_width() as i32 + gap;
    ((usable / column).max(1)) as usize
}

pub fn tile_rect(content: Rect, index: usize) -> Rect {
    let columns = grid_columns(content.size.width);
    let row = index / columns;
    let column = index % columns;
    let (tile_width, tile_height, tile_gap) = (tile_width(), tile_height(), tile_gap());
    Rect::new(
        content.origin.x + grid_padding() + (column as i32) * (tile_width as i32 + tile_gap),
        content.origin.y + scale::pt_i32(10) + (row as i32) * (tile_height as i32 + tile_gap),
        tile_width,
        tile_height,
    )
}

// ---- scrolling geometry ----------------------------------------------------

/// `content` moved up by `scroll` pixels: the rectangle row and tile geometry
/// is computed against, so a scrolled listing is the same layout further up.
/// Everything that is about the *screen* (clipping, hit-testing, the sticky
/// header) keeps using the real `content`.
pub fn scrolled(content: Rect, scroll: i32) -> Rect {
    Rect::new(content.origin.x, content.origin.y - scroll, content.size.width, content.size.height)
}

/// The part of the content area rows and tiles are visible in: all of it for
/// the icon view, everything under the column header for the list.
pub fn viewport(content: Rect, mode: ViewMode) -> Rect {
    match mode {
        ViewMode::Grid => content,
        ViewMode::List => {
            let header = header_height();
            Rect::new(content.origin.x, content.origin.y + header as i32, content.size.width, content.size.height.saturating_sub(header))
        }
    }
}

/// How tall the whole listing is, from the top of the content area.
pub fn content_height(content_width: u32, entry_count: usize, mode: ViewMode) -> i32 {
    match mode {
        ViewMode::List => header_height() as i32 + entry_count as i32 * theme::row_height() as i32 + scale::pt_i32(8),
        ViewMode::Grid => {
            let columns = grid_columns(content_width);
            let rows = entry_count.div_ceil(columns) as i32;
            scale::pt_i32(10) + rows * (tile_height() as i32 + tile_gap()) + scale::pt_i32(8)
        }
    }
}

/// The furthest the listing can scroll: how far it overflows the content area.
pub fn max_scroll(content: Rect, entry_count: usize, mode: ViewMode) -> i32 {
    (content_height(content.size.width, entry_count, mode) - content.size.height as i32).max(0)
}

/// Which entry index (if any) a point over the content area lands on, with the
/// listing scrolled by `scroll` pixels.
pub fn hit_test(content: Rect, entry_count: usize, mode: ViewMode, scroll: i32, point: Point) -> Option<usize> {
    // Rows and tiles that are scrolled out of view exist in the geometry but
    // not on screen: they must not catch clicks meant for what is there (the
    // column header above, the status bar below).
    if !viewport(content, mode).contains(point) {
        return None;
    }

    let scrolled = scrolled(content, scroll);

    for index in 0..entry_count {
        let hit = match mode {
            ViewMode::List => row_rect(scrolled, index).contains(point),
            ViewMode::Grid => tile_rect(scrolled, index).contains(point),
        };
        if hit {
            return Some(index);
        }
    }
    None
}

/// Draw the listing, scrolled by `scroll` pixels, with the overlay scroll
/// indicator at `indicator` opacity (0 = hidden).
///
/// Rows and tiles that are scrolled partly out of the content area spill over
/// its edges; the caller draws the toolbar and status line after this so they
/// cover the spill, and the list's column header is drawn here, last.
pub fn draw(frame: &mut Frame<'_>, content: Rect, entries: &[Entry], mode: ViewMode, selected: Option<usize>, pressed: Option<usize>, scroll: i32, indicator: u8) {
    frame.fill_rect(content, theme::WINDOW_BACKGROUND);

    if entries.is_empty() {
        if mode == ViewMode::List {
            draw_stripes(frame, content, content, 0);
            draw_list_header(frame, content);
        }
        draw_empty_state(frame, content);
        return;
    }

    let scrolled = scrolled(content, scroll);

    match mode {
        ViewMode::List => draw_list(frame, content, scrolled, entries, selected, pressed),
        ViewMode::Grid => draw_grid(frame, content, scrolled, entries, selected, pressed),
    }

    draw_scroll_indicator(frame, content, mode, entries.len(), scroll, indicator);
}

/// The overlay scroll indicator: a slim rounded bar at the right edge, as long
/// as the visible part is of the whole, shown while scrolling and fading out.
fn draw_scroll_indicator(frame: &mut Frame<'_>, content: Rect, mode: ViewMode, entry_count: usize, scroll: i32, opacity: u8) {
    let max = max_scroll(content, entry_count, mode);
    if opacity == 0 || max == 0 {
        return;
    }

    let track = viewport(content, mode);
    let total = content_height(content.size.width, entry_count, mode).max(1) as i64;
    let inset = scale::pt_i32(3);
    let track_height = (track.size.height as i32 - inset * 2).max(1);
    let thumb_height = ((track_height as i64 * content.size.height as i64 / total) as i32).clamp(scale::pt_i32(28).min(track_height), track_height);
    let travel = track_height - thumb_height;
    let thumb_y = track.origin.y + inset + (travel as i64 * scroll.clamp(0, max) as i64 / max as i64) as i32;

    let width = scale::pt(5);
    let thumb = Rect::new(content.origin.x + content.size.width as i32 - width as i32 - scale::pt_i32(3), thumb_y, width, thumb_height as u32);
    let alpha = (u32::from(opacity) * 140 / 255) as u8;
    draw2d::fill_round_rect(frame.canvas(), thumb, width / 2, Color::rgba(50, 52, 60, alpha));
}

/// A folder with nothing in it still deserves a legible screen instead of a
/// content area that just silently stops -- easy to mistake for a rendering
/// bug otherwise.
fn draw_empty_state(frame: &mut Frame<'_>, content: Rect) {
    let point_size = theme::label_point_size();
    let text = "This folder is empty";
    let measured = frame.measure(text, point_size);
    let x = content.origin.x + (content.size.width.saturating_sub(measured.width) / 2) as i32;
    let y = content.origin.y + (content.size.height.saturating_sub(measured.height) / 2) as i32;
    frame.text(Point::new(x, y), text, theme::TEXT_SECONDARY, point_size);
}

// ---- list view -------------------------------------------------------------

/// The column header, sticky at the top of the content area. It paints its own
/// background: rows scrolled up under it must not show through.
fn draw_list_header(frame: &mut Frame<'_>, content: Rect) {
    let header = Rect::new(content.origin.x, content.origin.y, content.size.width, header_height());
    frame.fill_rect(header, theme::WINDOW_BACKGROUND);
    let columns = Columns::compute(content);
    let point_size = theme::caption_point_size();
    let color = theme::TEXT_SECONDARY;

    theme::text_in(frame, header, columns.name_x, "Name", color, point_size, FontWeight::Semibold);
    if let Some(kind_x) = columns.kind_x {
        theme::text_in(frame, header, kind_x, "Kind", color, point_size, FontWeight::Semibold);
    }
    let size_width = frame.measure_with_weight("Size", point_size, FontWeight::Semibold).width as i32;
    theme::text_in(frame, header, columns.size_right - size_width, "Size", color, point_size, FontWeight::Semibold);

    let line = theme::hline(content.origin.x, content.origin.y + header_height() as i32 - theme::hairline() as i32, content.size.width);
    frame.fill_rect(line, theme::HAIRLINE);
}

/// The background of list row `index`: white and pale alternately.
fn row_background(index: usize) -> Color {
    if index % 2 == 1 { theme::STRIPE } else { theme::WINDOW_BACKGROUND }
}

/// Continue the striping down the empty part of the window, from row `first`.
/// `scrolled` is the content area moved by the scroll offset; `content` the
/// real one, which the stripes are clipped to.
fn draw_stripes(frame: &mut Frame<'_>, content: Rect, scrolled: Rect, first: usize) {
    let row_height = theme::row_height() as i32;
    let bottom = content.origin.y + content.size.height as i32;
    let top = content.origin.y + header_height() as i32;
    let mut index = first;

    loop {
        let row = row_rect(scrolled, index);
        if row.origin.y >= bottom {
            break;
        }
        if index % 2 == 1 && row.origin.y + row_height > top {
            let y = row.origin.y.max(top);
            let visible = Rect::new(row.origin.x, y, row.size.width, (row.origin.y + row_height).min(bottom).saturating_sub(y).max(0) as u32);
            frame.fill_rect(visible, theme::STRIPE);
        }
        index += 1;
    }
}

fn draw_list(frame: &mut Frame<'_>, content: Rect, scrolled: Rect, entries: &[Entry], selected: Option<usize>, pressed: Option<usize>) {
    draw_stripes(frame, content, scrolled, entries.len());

    for index in 0..entries.len() {
        draw_list_row(frame, content, scrolled, entries, index, selected, pressed);
    }

    // Last, over any row that is scrolled up beneath it.
    draw_list_header(frame, content);
}

/// One list row, in isolation -- shared by the full `draw_list` pass and by
/// [`redraw_row`]'s single-row repaint. It paints its own background, so it
/// can be drawn over whatever was there. `scrolled` is where the geometry says
/// the row is; `content` the real area it is clipped to.
fn draw_list_row(frame: &mut Frame<'_>, content: Rect, scrolled: Rect, entries: &[Entry], index: usize, selected: Option<usize>, pressed: Option<usize>) {
    let Some(entry) = entries.get(index) else { return; };

    let row = row_rect(scrolled, index);
    let columns = Columns::compute(content);
    let is_selected = selected == Some(index);
    let is_pressed = pressed == Some(index);

    // Rows scrolled out of the content area, above or below, are not on screen.
    let content_bottom = content.origin.y + content.size.height as i32;
    if row.origin.y >= content_bottom || row.origin.y + row.size.height as i32 <= content.origin.y + header_height() as i32 {
        return;
    }

    frame.fill_rect(Rect::new(row.origin.x, row.origin.y, row.size.width, row.size.height.min((content_bottom - row.origin.y) as u32)), row_background(index));

    if is_selected || is_pressed {
        let fill = match (is_selected, is_pressed) {
            (true, true) => theme::ACCENT_PRESSED,
            (true, false) => theme::ACCENT,
            _ => theme::ACCENT_SOFT,
        };
        draw2d::fill_round_rect(frame.canvas(), selection_rect(row), scale::pt(6), fill);
    }
    let on_accent = is_selected;

    let icon_size = list_icon_size();
    let icon = Rect::new(columns.name_x, row.origin.y + (row.size.height.saturating_sub(icon_size) / 2) as i32, icon_size, icon_size);
    icons::draw_file_icon(frame.canvas(), icon, entry.kind.icon());

    let point_size = theme::label_point_size();
    let text_x = columns.name_x + icon_size as i32 + scale::pt_i32(8);
    let name_width = (columns.name_right - text_x).max(0) as u32;
    let mut scratch = [0u8; crate::model::NAME_MAX + 4];
    let name = theme::fit_name(frame, entry.name(), point_size, FontWeight::Regular, name_width, &mut scratch);
    let name_color = if on_accent { theme::ON_ACCENT } else { theme::TEXT_PRIMARY };
    theme::text_in(frame, row, text_x, name, name_color, point_size, FontWeight::Regular);

    let secondary = if on_accent { theme::ON_ACCENT } else { theme::TEXT_SECONDARY };
    let secondary_size = theme::label_point_size();

    if let Some(kind_x) = columns.kind_x {
        theme::text_in(frame, row, kind_x, entry.kind.label(), secondary, secondary_size, FontWeight::Regular);
    }

    let mut size_scratch = [0u8; 20];
    let size_text = if entry.is_folder() { "--" } else { format_size(entry.size_bytes, &mut size_scratch) };
    let size_width = frame.measure(size_text, secondary_size).width as i32;
    theme::text_in(frame, row, columns.size_right - size_width, size_text, secondary, secondary_size, FontWeight::Regular);

    if entry.is_folder() {
        let size = disclosure_size();
        let chevron = Rect::new(columns.disclosure_x, row.origin.y + (row.size.height.saturating_sub(size) / 2) as i32, size, size);
        let color = if on_accent { theme::ON_ACCENT } else { theme::TEXT_TERTIARY };
        icons::draw_disclosure(frame.canvas(), chevron, color);
    }
}

// ---- grid view -------------------------------------------------------------

fn draw_grid(frame: &mut Frame<'_>, content: Rect, scrolled: Rect, entries: &[Entry], selected: Option<usize>, pressed: Option<usize>) {
    for index in 0..entries.len() {
        draw_grid_tile(frame, content, scrolled, entries, index, selected, pressed);
    }
}

/// One grid tile, in isolation -- shared by the full `draw_grid` pass and by
/// [`redraw_row`]'s single-tile repaint. `scrolled` is where the geometry says
/// the tile is; `content` the real area it is clipped to.
fn draw_grid_tile(frame: &mut Frame<'_>, content: Rect, scrolled: Rect, entries: &[Entry], index: usize, selected: Option<usize>, pressed: Option<usize>) {
    let Some(entry) = entries.get(index) else { return; };
    let tile = tile_rect(scrolled, index);
    // Tiles scrolled out of the content area, above or below, are not on screen.
    if tile.origin.y >= content.origin.y + content.size.height as i32 || tile.origin.y + tile.size.height as i32 <= content.origin.y {
        return;
    }

    let is_selected = selected == Some(index);
    let is_pressed = pressed == Some(index);

    let plate_size = plate_size();
    let plate = Rect::new(tile.origin.x + (tile.size.width.saturating_sub(plate_size) / 2) as i32, tile.origin.y + scale::pt_i32(2), plate_size, plate_size);

    if is_selected || is_pressed {
        let color = if is_pressed { theme::ICON_PLATE_PRESSED } else { theme::ICON_PLATE };
        draw2d::fill_round_rect(frame.canvas(), plate, scale::pt(9), color);
    }

    let icon_size = grid_icon_size();
    let icon = Rect::new(plate.origin.x + (plate_size.saturating_sub(icon_size) / 2) as i32, plate.origin.y + (plate_size.saturating_sub(icon_size) / 2) as i32, icon_size, icon_size);
    icons::draw_file_icon(frame.canvas(), icon, entry.kind.icon());

    // The name, centred under the icon, on an accent pill when selected.
    let point_size = theme::grid_name_point_size();
    let line = theme::line_height(frame, point_size);
    let name_area = Rect::new(tile.origin.x, plate.origin.y + plate_size as i32 + scale::pt_i32(5), tile.size.width, line + scale::pt(3));
    let padding = scale::pt(5);
    let mut scratch = [0u8; crate::model::NAME_MAX + 4];
    let name = theme::fit_name(frame, entry.name(), point_size, FontWeight::Regular, tile.size.width.saturating_sub(padding * 2), &mut scratch);
    let measured = frame.measure_with_weight(name, point_size, FontWeight::Regular);

    if is_selected {
        let pill_width = (measured.width + padding * 2).min(tile.size.width);
        let pill = Rect::new(tile.origin.x + (tile.size.width.saturating_sub(pill_width) / 2) as i32, name_area.origin.y, pill_width, name_area.size.height);
        let fill = if is_pressed { theme::ACCENT_PRESSED } else { theme::ACCENT };
        draw2d::fill_round_rect(frame.canvas(), pill, scale::pt(5), fill);
    }

    let color = if is_selected { theme::ON_ACCENT } else { theme::TEXT_PRIMARY };
    let x = tile.origin.x + (tile.size.width.saturating_sub(measured.width) / 2) as i32;
    theme::text_in(frame, name_area, x, name, color, point_size, FontWeight::Regular);
}

/// Repaint a single row/tile in place with its current selected/pressed
/// state. Used when nothing else on screen needs to move -- see
/// `VoyagerApp`'s `PendingRedraw::Partial`, the reason this exists instead of
/// always going through `draw`'s full-list pass (measuring and drawing every
/// row's text on every event was the dominant cost of using a file list under
/// software rendering).
///
/// Only for an unscrolled listing: a scrolled row can sit partly under the
/// column header or the toolbar, which a single-row repaint would overwrite, so
/// the app redraws in full once it has scrolled.
pub fn redraw_row(frame: &mut Frame<'_>, content: Rect, entries: &[Entry], mode: ViewMode, index: usize, selected: Option<usize>, pressed: bool) {
    if index >= entries.len() {
        return;
    }

    let pressed = pressed.then_some(index);

    match mode {
        ViewMode::List => draw_list_row(frame, content, content, entries, index, selected, pressed),
        ViewMode::Grid => {
            // A tile has no background of its own: clear it first.
            frame.fill_rect(tile_rect(content, index), theme::WINDOW_BACKGROUND);
            draw_grid_tile(frame, content, content, entries, index, selected, pressed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTENT: Rect = Rect::new(0, 0, 620, 400);

    #[test]
    fn grid_always_has_at_least_one_column() {
        assert!(grid_columns(0) >= 1);
        assert!(grid_columns(50) >= 1);
        assert!(grid_columns(CONTENT.size.width) >= 1);
    }

    #[test]
    fn list_rows_do_not_overlap() {
        let first = row_rect(CONTENT, 0);
        let second = row_rect(CONTENT, 1);
        assert_eq!(first.origin.x, second.origin.x);
        assert!(second.origin.y >= first.origin.y + first.size.height as i32);
    }

    #[test]
    fn tiles_wrap_onto_a_new_row() {
        let columns = grid_columns(CONTENT.size.width);
        let last_in_row = tile_rect(CONTENT, columns - 1);
        let first_of_next_row = tile_rect(CONTENT, columns);
        assert_eq!(last_in_row.origin.y, tile_rect(CONTENT, 0).origin.y);
        assert!(first_of_next_row.origin.y > last_in_row.origin.y);
    }

    #[test]
    fn list_hit_test_finds_the_row_under_the_point() {
        let row = row_rect(CONTENT, 2);
        let center = Point::new(row.origin.x + 5, row.origin.y + 5);
        assert_eq!(hit_test(CONTENT, 5, ViewMode::List, 0, center), Some(2));
    }

    #[test]
    fn grid_hit_test_finds_the_tile_under_the_point() {
        let tile = tile_rect(CONTENT, 1);
        let center = Point::new(tile.origin.x + 5, tile.origin.y + 5);
        assert_eq!(hit_test(CONTENT, 5, ViewMode::Grid, 0, center), Some(1));
    }

    #[test]
    fn hit_test_ignores_points_past_the_entry_count() {
        let row = row_rect(CONTENT, 4);
        let point = Point::new(row.origin.x + 5, row.origin.y + 5);
        assert_eq!(hit_test(CONTENT, 3, ViewMode::List, 0, point), None);
    }

    #[test]
    fn hit_test_ignores_rows_beyond_the_content_area() {
        // A row past the bottom must not catch a click that landed on whatever
        // is below the content (the status bar).
        let below = Point::new(20, CONTENT.size.height as i32 + 4);
        assert!((0..48).any(|index| row_rect(CONTENT, index).contains(below)));
        assert_eq!(hit_test(CONTENT, 48, ViewMode::List, 0, below), None);
    }

    #[test]
    fn a_scrolled_list_hits_the_row_that_is_now_under_the_point() {
        // Scrolled by exactly three rows, the row under what used to be row 0
        // is row 3.
        let scroll = 3 * theme::row_height() as i32;
        let row = row_rect(CONTENT, 0);
        let point = Point::new(row.origin.x + 5, row.origin.y + 5);
        assert_eq!(hit_test(CONTENT, 20, ViewMode::List, scroll, point), Some(3));

        // Half a row further, the same point is in the lower half of row 3.
        assert_eq!(hit_test(CONTENT, 20, ViewMode::List, scroll + theme::row_height() as i32 / 2, point), Some(3));
    }

    #[test]
    fn the_column_header_is_not_a_row_however_far_the_list_has_scrolled() {
        let header = Point::new(20, header_height() as i32 / 2);
        assert_eq!(hit_test(CONTENT, 20, ViewMode::List, 200, header), None);
        assert!(viewport(CONTENT, ViewMode::List).origin.y >= header_height() as i32);
    }

    #[test]
    fn a_scrolled_grid_hits_the_tile_now_under_the_point() {
        let columns = grid_columns(CONTENT.size.width);
        let pitch = tile_height() as i32 + tile_gap();
        let tile = tile_rect(CONTENT, 0);
        let point = Point::new(tile.origin.x + 5, tile.origin.y + 5);
        // One tile row down: the point is over the first tile of the second row.
        assert_eq!(hit_test(CONTENT, 40, ViewMode::Grid, pitch, point), Some(columns));
    }

    #[test]
    fn nothing_scrolls_when_everything_fits() {
        assert_eq!(max_scroll(CONTENT, 3, ViewMode::List), 0);
        assert_eq!(max_scroll(CONTENT, 3, ViewMode::Grid), 0);
        assert_eq!(max_scroll(CONTENT, 0, ViewMode::List), 0);
    }

    #[test]
    fn the_scroll_range_is_exactly_the_overflow() {
        let rows = 60;
        let overflow = header_height() as i32 + rows * theme::row_height() as i32 + scale::pt_i32(8) - CONTENT.size.height as i32;
        assert_eq!(max_scroll(CONTENT, rows as usize, ViewMode::List), overflow);
        assert!(overflow > 0);

        // Scrolled to the very end, the last row's bottom edge is in view.
        let end = scrolled(CONTENT, overflow);
        let last = row_rect(end, rows as usize - 1);
        assert!(last.origin.y + last.size.height as i32 <= CONTENT.size.height as i32);
    }

    #[test]
    fn the_grid_range_grows_with_the_number_of_rows_of_tiles() {
        let columns = grid_columns(CONTENT.size.width);
        let few = max_scroll(CONTENT, columns * 2, ViewMode::Grid);
        let many = max_scroll(CONTENT, columns * 20, ViewMode::Grid);
        assert!(many > few);
        assert_eq!(many - few, 18 * (tile_height() as i32 + tile_gap()));
    }

    #[test]
    fn the_kind_column_drops_out_of_a_narrow_window() {
        let wide = Columns::compute(Rect::new(0, 0, scale::pt(700), scale::pt(400)));
        let narrow = Columns::compute(Rect::new(0, 0, scale::pt(320), scale::pt(400)));
        assert!(wide.kind_x.is_some());
        assert!(narrow.kind_x.is_none());
        // The name column keeps a usable width either way.
        assert!(wide.name_right - wide.name_x > scale::pt_i32(120));
        assert!(narrow.name_right - narrow.name_x > scale::pt_i32(60));
    }

    #[test]
    fn list_columns_are_ordered_left_to_right() {
        let columns = Columns::compute(Rect::new(0, 0, scale::pt(700), scale::pt(400)));
        let kind_x = columns.kind_x.expect("wide window has a kind column");
        assert!(columns.name_x < columns.name_right);
        assert!(columns.name_right < kind_x);
        assert!(kind_x < columns.size_right);
        assert!(columns.size_right < columns.disclosure_x);
    }
}
