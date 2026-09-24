//! The source list, in Finder's style: small group headings ("Favorites",
//! "Locations"), a blue glyph and a label per destination, and a soft grey
//! pill behind the current one.

use ui::prelude::*;

use crate::draw2d;
use crate::icons;
use crate::layout::{sidebar_glyph_inset, Layout};
use crate::model::{DestinationIcon, DESTINATIONS, SECTIONS};
use crate::theme;

pub fn hit_test(layout: &Layout, point: Point) -> Option<usize> {
    if !layout.sidebar_visible || !layout.sidebar.contains(point) {
        return None;
    }
    for index in 0..DESTINATIONS.len() {
        if layout.sidebar_row(index).contains(point) {
            return Some(index);
        }
    }
    None
}

fn heading_point_size() -> u32 {
    scale::pt(11)
}

fn row_radius() -> u32 {
    scale::pt(7)
}

fn glyph_size() -> u32 {
    scale::pt(16)
}

pub fn draw(frame: &mut Frame<'_>, layout: &Layout, selected: Option<usize>, hovered: Option<usize>, pressed: Option<usize>) {
    if !layout.sidebar_visible {
        return;
    }

    frame.fill_rect(layout.sidebar, theme::SIDEBAR_BACKGROUND);
    frame.fill_rect(layout.sidebar_divider, theme::DIVIDER);

    let mut section = usize::MAX;
    for (index, destination) in DESTINATIONS.iter().enumerate() {
        if destination.section != section {
            section = destination.section;
            let heading = layout.sidebar_section_header(index);
            let label = SECTIONS.get(section).copied().unwrap_or("");
            theme::text_in(frame, heading, heading.origin.x, label, theme::SIDEBAR_HEADER, heading_point_size(), FontWeight::Semibold);
        }
        draw_row(frame, layout, index, selected, hovered, pressed);
    }
}

/// One destination row, in isolation -- shared by the full `draw` pass and
/// by [`redraw_row`]'s single-row repaint.
fn draw_row(frame: &mut Frame<'_>, layout: &Layout, index: usize, selected: Option<usize>, hovered: Option<usize>, pressed: Option<usize>) {
    let Some(destination) = DESTINATIONS.get(index) else { return; };
    let row = layout.sidebar_row(index);
    let is_selected = selected == Some(index);

    let background = if pressed == Some(index) {
        Some(theme::PRESSED)
    } else if is_selected {
        Some(theme::SIDEBAR_SELECTION)
    } else if hovered == Some(index) {
        Some(theme::SIDEBAR_HOVER)
    } else {
        None
    };
    if let Some(color) = background {
        draw2d::fill_round_rect(frame.canvas(), row, row_radius(), color);
    }

    let glyph_size = glyph_size();
    let glyph = Rect::new(
        row.origin.x + sidebar_glyph_inset(),
        row.origin.y + (row.size.height.saturating_sub(glyph_size) / 2) as i32,
        glyph_size,
        glyph_size,
    );
    match destination.icon {
        DestinationIcon::Folder => icons::draw_folder_glyph(frame.canvas(), glyph, theme::ACCENT),
        DestinationIcon::Drive => icons::draw_drive_glyph(frame.canvas(), glyph, theme::ACCENT),
    }

    let text_x = glyph.origin.x + glyph_size as i32 + scale::pt_i32(6);
    let available = (row.origin.x + row.size.width as i32 - scale::pt_i32(4) - text_x).max(0) as u32;
    let point_size = theme::label_point_size();
    let mut scratch = [0u8; 48];
    let label = theme::fit_text(frame, destination.label, point_size, FontWeight::Regular, available, &mut scratch);
    theme::text_in(frame, row, text_x, label, theme::TEXT_PRIMARY, point_size, FontWeight::Regular);
}

/// Repaint a single sidebar row in place: erase it back to the sidebar
/// background, then redraw it with its current selected/hovered/pressed
/// state. See `content::redraw_row` and `VoyagerApp::PendingRedraw::Partial`
/// for why this exists.
pub fn redraw_row(frame: &mut Frame<'_>, layout: &Layout, index: usize, selected: Option<usize>, hovered: bool, pressed: bool) {
    if !layout.sidebar_visible || index >= DESTINATIONS.len() {
        return;
    }

    frame.fill_rect(layout.sidebar_row(index), theme::SIDEBAR_BACKGROUND);
    draw_row(frame, layout, index, selected, hovered.then_some(index), pressed.then_some(index));
}
