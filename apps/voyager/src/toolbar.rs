//! Toolbar, in Finder's arrangement: a sidebar toggle and back/forward
//! chevrons on the left, the current folder's icon and name next to them, and
//! on the right a segmented icons/list control plus an Open button. Controls
//! are borderless glyphs that light up under the pointer; only the active view
//! segment and the Open button carry a shape of their own.

use ui::prelude::*;

use crate::content::ViewMode;
use crate::draw2d;
use crate::icons::{self, Direction, FileIcon};
use crate::layout::Layout;
use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToolbarHit {
    SidebarToggle,
    Back,
    Forward,
    ViewGrid,
    ViewList,
    Open,
}

pub fn hit_test(layout: &Layout, point: Point) -> Option<ToolbarHit> {
    if layout.sidebar_toggle.contains(point) {
        Some(ToolbarHit::SidebarToggle)
    } else if layout.back.contains(point) {
        Some(ToolbarHit::Back)
    } else if layout.forward.contains(point) {
        Some(ToolbarHit::Forward)
    } else if layout.view_grid.contains(point) {
        Some(ToolbarHit::ViewGrid)
    } else if layout.view_list.contains(point) {
        Some(ToolbarHit::ViewList)
    } else if layout.open.contains(point) {
        Some(ToolbarHit::Open)
    } else {
        None
    }
}

pub struct ToolbarState<'a> {
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub view_mode: ViewMode,
    pub open_enabled: bool,
    pub hovered: Option<ToolbarHit>,
    pub pressed: Option<ToolbarHit>,
    /// Borrowed from `Navigator::title`, not `'static`: a real path's
    /// current directory name is not known until runtime.
    pub title: &'a str,
}

fn button_radius() -> u32 {
    scale::pt(7)
}

fn glyph_size() -> u32 {
    scale::pt(16)
}

/// `size` square centred in `rect`.
fn centred(rect: Rect, size: u32) -> Rect {
    Rect::new(
        rect.origin.x + (rect.size.width.saturating_sub(size) / 2) as i32,
        rect.origin.y + (rect.size.height.saturating_sub(size) / 2) as i32,
        size,
        size,
    )
}

/// The light-up behind a button while the pointer is on it or it is pressed.
fn button_background(frame: &mut Frame<'_>, rect: Rect, hovered: bool, pressed: bool, enabled: bool) {
    if !enabled {
        return;
    }
    let color = if pressed {
        theme::PRESSED
    } else if hovered {
        theme::HOVER
    } else {
        return;
    };
    draw2d::fill_round_rect(frame.canvas(), rect, button_radius(), color);
}

pub fn draw(frame: &mut Frame<'_>, layout: &Layout, state: &ToolbarState<'_>) {
    frame.fill_rect(layout.toolbar, theme::TOOLBAR_BACKGROUND);
    frame.fill_rect(layout.toolbar_divider, theme::DIVIDER);

    let is = |hit: ToolbarHit, of: Option<ToolbarHit>| of == Some(hit);

    // Sidebar toggle.
    let hovered = is(ToolbarHit::SidebarToggle, state.hovered);
    let pressed = is(ToolbarHit::SidebarToggle, state.pressed);
    button_background(frame, layout.sidebar_toggle, hovered, pressed, true);
    icons::draw_sidebar_glyph(frame.canvas(), centred(layout.sidebar_toggle, glyph_size()), theme::GLYPH);

    // Back and forward.
    for (rect, hit, direction, enabled) in [
        (layout.back, ToolbarHit::Back, Direction::Left, state.can_go_back),
        (layout.forward, ToolbarHit::Forward, Direction::Right, state.can_go_forward),
    ] {
        button_background(frame, rect, is(hit, state.hovered), is(hit, state.pressed), enabled);
        let color = if enabled { theme::GLYPH } else { theme::TEXT_DISABLED };
        icons::draw_chevron(frame.canvas(), centred(rect, glyph_size()), direction, color);
    }

    draw_title(frame, layout, state.title);
    draw_view_control(frame, layout, state);
    draw_open_button(frame, layout, state);
}

/// The folder's icon and name, cut with an ellipsis if the toolbar is narrow.
fn draw_title(frame: &mut Frame<'_>, layout: &Layout, title: &str) {
    let area = layout.title_area;
    if area.size.width < scale::pt(40) {
        return;
    }

    let icon_size = scale::pt(18);
    let icon = Rect::new(area.origin.x, area.origin.y + (area.size.height.saturating_sub(icon_size) / 2) as i32, icon_size, icon_size);
    icons::draw_file_icon(frame.canvas(), icon, FileIcon::Folder);

    let text_x = icon.origin.x + icon_size as i32 + scale::pt_i32(7);
    let available = (area.origin.x + area.size.width as i32 - text_x).max(0) as u32;
    let point_size = theme::title_point_size();
    let mut scratch = [0u8; 64];
    let shown = theme::fit_text(frame, title, point_size, FontWeight::Semibold, available, &mut scratch);
    theme::text_in(frame, area, text_x, shown, theme::TEXT_PRIMARY, point_size, FontWeight::Semibold);
}

/// Finder's segmented control: a grey track, a white puck under the active
/// segment, one glyph per segment.
fn draw_view_control(frame: &mut Frame<'_>, layout: &Layout, state: &ToolbarState<'_>) {
    let radius = button_radius() + scale::pt(1);
    draw2d::fill_round_rect(frame.canvas(), layout.view_group, radius, theme::SEGMENT_TRACK);

    let inset = scale::pt(2);
    let is = |hit: ToolbarHit, of: Option<ToolbarHit>| of == Some(hit);

    for (rect, hit, mode) in [
        (layout.view_grid, ToolbarHit::ViewGrid, ViewMode::Grid),
        (layout.view_list, ToolbarHit::ViewList, ViewMode::List),
    ] {
        let active = state.view_mode == mode;
        let hovered = is(hit, state.hovered);
        let pressed = is(hit, state.pressed);
        let puck = Rect::new(
            rect.origin.x + inset as i32,
            rect.origin.y + inset as i32,
            rect.size.width.saturating_sub(inset * 2),
            rect.size.height.saturating_sub(inset * 2),
        );

        if active {
            let fill = if pressed { theme::PRESSED } else { theme::SEGMENT_PUCK };
            draw2d::bordered_round_rect(frame.canvas(), puck, radius.saturating_sub(inset), theme::SEGMENT_PUCK_BORDER, fill, theme::hairline());
        } else if pressed {
            draw2d::fill_round_rect(frame.canvas(), puck, radius.saturating_sub(inset), theme::PRESSED);
        } else if hovered {
            draw2d::fill_round_rect(frame.canvas(), puck, radius.saturating_sub(inset), theme::HOVER);
        }

        // The active segment's glyph takes the accent: the white puck alone
        // is barely distinguishable from the track (1.4:1), so the glyph is
        // what carries the selected state at 3:1 or better.
        let color = if active { theme::ACCENT } else { theme::GLYPH };
        let glyph = centred(rect, glyph_size());
        match mode {
            ViewMode::Grid => icons::draw_grid_glyph(frame.canvas(), glyph, color),
            ViewMode::List => icons::draw_list_glyph(frame.canvas(), glyph, color),
        }
    }
}

fn draw_open_button(frame: &mut Frame<'_>, layout: &Layout, state: &ToolbarState<'_>) {
    let rect = layout.open;
    // Dropped from a narrow window's toolbar (see `Layout::compute`).
    if rect.size.width == 0 {
        return;
    }
    let enabled = state.open_enabled;
    let hovered = state.hovered == Some(ToolbarHit::Open) && enabled;
    let pressed = state.pressed == Some(ToolbarHit::Open) && enabled;

    if enabled {
        let fill = if pressed {
            theme::PRESSED
        } else if hovered {
            theme::HOVER
        } else {
            Color::WHITE
        };
        draw2d::bordered_round_rect(frame.canvas(), rect, button_radius(), theme::SEGMENT_PUCK_BORDER, fill, theme::hairline());
    }

    let point_size = theme::label_point_size();
    let color = if enabled { theme::TEXT_PRIMARY } else { theme::TEXT_DISABLED };
    let measured = frame.measure_with_weight("Open", point_size, FontWeight::Regular);
    let x = rect.origin.x + (rect.size.width.saturating_sub(measured.width) / 2) as i32;
    theme::text_in(frame, rect, x, "Open", color, point_size, FontWeight::Regular);
}
