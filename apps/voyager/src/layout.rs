//! One geometry pass, shared by drawing and hit-testing.
//!
//! `App::event` is not given a `Frame`, only an `Event` — there is no way to
//! measure text there. So every control here gets a fixed size (chosen
//! generously for its fixed label or glyph) instead of a text-measured one;
//! `Layout::compute` is then a pure function of the content size and the
//! sidebar's visibility, callable identically from `draw` and from `event`
//! without needing to cache anything between them.

use ui::prelude::*;

use crate::model::DESTINATIONS;
use crate::theme::{sidebar_width_for, status_height, toolbar_height};

fn pt(points: u32) -> u32 {
    scale::pt(points)
}

fn pt_i(points: i32) -> i32 {
    scale::pt_i32(points)
}

/// Height of a toolbar control (button, segmented control).
pub fn control_height() -> u32 {
    pt(28)
}
pub fn sidebar_toggle_width() -> u32 {
    pt(36)
}
pub fn nav_button_width() -> u32 {
    pt(30)
}
pub fn view_segment_width() -> u32 {
    pt(34)
}
pub fn open_width() -> u32 {
    pt(56)
}

/// The sidebar's vertical rhythm.
pub fn sidebar_top_padding() -> i32 {
    pt_i(10)
}
pub fn sidebar_header_height() -> u32 {
    pt(20)
}
pub fn sidebar_row_pitch() -> u32 {
    pt(26)
}
pub fn sidebar_section_gap() -> i32 {
    pt_i(8)
}
/// How far a sidebar row's glyph sits in from the row's own leading edge.
/// Section headings start at the same x as the glyphs, so each heading lines
/// up with the column it heads.
pub fn sidebar_glyph_inset() -> i32 {
    pt_i(8)
}

/// Below this content width the Open button gives way: the toolbar has no room
/// for it, and clicking a selected folder again opens it anyway.
pub fn open_button_min_width() -> u32 {
    pt(340)
}

#[derive(Clone, Copy)]
pub struct Layout {
    pub toolbar: Rect,
    pub toolbar_divider: Rect,
    pub sidebar_toggle: Rect,
    pub back: Rect,
    pub forward: Rect,
    /// Where the current folder's icon and name go: from the navigation
    /// buttons to the view controls.
    pub title_area: Rect,
    /// The segmented control's track, and its two segments (Finder's order:
    /// icons, then list).
    pub view_group: Rect,
    pub view_grid: Rect,
    pub view_list: Rect,
    pub open: Rect,
    pub sidebar: Rect,
    pub sidebar_divider: Rect,
    pub sidebar_visible: bool,
    pub content: Rect,
    pub status: Rect,
    pub status_divider: Rect,
}

impl Layout {
    pub fn compute(size: Size, sidebar_visible: bool) -> Self {
        let width = size.width;
        let toolbar_height = toolbar_height();
        let control_height = control_height();
        let control_y = ((toolbar_height - control_height) / 2) as i32;
        let edge = pt_i(12);
        let gap = pt_i(10);

        let toggle_width = sidebar_toggle_width();
        let nav_width = nav_button_width();
        let segment_width = view_segment_width();
        let show_open = width >= open_button_min_width();
        let open_width = if show_open { open_width() } else { 0 };

        let sidebar_toggle = Rect::new(edge, control_y, toggle_width, control_height);
        let mut x = edge + toggle_width as i32 + gap;
        let back = Rect::new(x, control_y, nav_width, control_height);
        x += nav_width as i32;
        let forward = Rect::new(x, control_y, nav_width, control_height);
        x += nav_width as i32 + gap + pt_i(4);

        let mut right = width as i32 - edge;
        right -= open_width as i32;
        let open = Rect::new(right, control_y, open_width, control_height);
        right -= if show_open { gap } else { 0 } + (segment_width * 2) as i32;
        let view_group = Rect::new(right, control_y, segment_width * 2, control_height);
        let view_grid = Rect::new(right, control_y, segment_width, control_height);
        let view_list = Rect::new(right + segment_width as i32, control_y, segment_width, control_height);

        let title_right = right - gap;
        let title_area = Rect::new(x, control_y, (title_right - x).max(0) as u32, control_height);

        let toolbar = Rect::new(0, 0, width, toolbar_height);
        let toolbar_divider = Rect::new(0, toolbar_height as i32 - 1, width, 1);

        let status_height = status_height();
        let body_top = toolbar_height as i32;
        let body_height = size.height.saturating_sub(toolbar_height + status_height);

        let sidebar_width = if sidebar_visible { sidebar_width_for(width) } else { 0 };
        let sidebar = Rect::new(0, body_top, sidebar_width, body_height);
        let sidebar_divider = Rect::new(sidebar_width as i32, body_top, 1, body_height);

        let content_x = sidebar_width as i32 + if sidebar_visible { 1 } else { 0 };
        let content = Rect::new(content_x, body_top, width.saturating_sub(content_x as u32), body_height);

        let status_y = body_top + body_height as i32;
        let status = Rect::new(0, status_y, width, status_height);
        let status_divider = Rect::new(0, status_y, width, 1);

        Self {
            toolbar,
            toolbar_divider,
            sidebar_toggle,
            back,
            forward,
            title_area,
            view_group,
            view_grid,
            view_list,
            open,
            sidebar,
            sidebar_divider,
            sidebar_visible,
            content,
            status,
            status_divider,
        }
    }

    /// Where destination `index` sits: the top of its section's heading, and
    /// the top of its own row.
    fn sidebar_position(&self, index: usize) -> (i32, i32) {
        let mut y = self.sidebar.origin.y + sidebar_top_padding();
        let mut section = usize::MAX;
        let mut header_y = y;

        for (position, destination) in DESTINATIONS.iter().enumerate() {
            if destination.section != section {
                if section != usize::MAX {
                    y += sidebar_section_gap();
                }
                section = destination.section;
                header_y = y;
                y += sidebar_header_height() as i32;
            }
            if position == index {
                return (header_y, y);
            }
            y += sidebar_row_pitch() as i32;
        }

        (header_y, y)
    }

    /// The heading above the section that holds destination `index`.
    pub fn sidebar_section_header(&self, index: usize) -> Rect {
        let (header_y, _) = self.sidebar_position(index);
        let row_x = self.sidebar.origin.x + pt_i(8);
        Rect::new(
            row_x + sidebar_glyph_inset(),
            header_y,
            self.sidebar.size.width.saturating_sub(pt(24)),
            sidebar_header_height(),
        )
    }

    pub fn sidebar_row(&self, index: usize) -> Rect {
        let (_, row_y) = self.sidebar_position(index);
        Rect::new(
            self.sidebar.origin.x + pt_i(8),
            row_y,
            self.sidebar.size.width.saturating_sub(pt(16)),
            sidebar_row_pitch().saturating_sub(pt(2)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default window's content area at the tests' default 2x scale:
    /// 430 x 268 pt.
    const CONTENT_SIZE: Size = Size::new(860, 536);

    #[test]
    fn content_starts_right_of_a_visible_sidebar() {
        let layout = Layout::compute(CONTENT_SIZE, true);
        assert_eq!(layout.sidebar.size.width, sidebar_width_for(CONTENT_SIZE.width));
        assert!(layout.content.origin.x >= layout.sidebar.size.width as i32);
    }

    #[test]
    fn content_fills_the_width_when_the_sidebar_is_hidden() {
        let layout = Layout::compute(CONTENT_SIZE, false);
        assert_eq!(layout.sidebar.size.width, 0);
        assert_eq!(layout.content.origin.x, 0);
        assert_eq!(layout.content.size.width, CONTENT_SIZE.width);
    }

    #[test]
    fn toolbar_controls_stay_within_the_toolbar_band() {
        let layout = Layout::compute(CONTENT_SIZE, true);
        for rect in [layout.sidebar_toggle, layout.back, layout.forward, layout.view_grid, layout.view_list, layout.open] {
            assert!(rect.origin.y >= 0);
            assert!(rect.origin.y + rect.size.height as i32 <= toolbar_height() as i32);
            assert!(rect.origin.x >= 0);
            assert!(rect.origin.x + rect.size.width as i32 <= CONTENT_SIZE.width as i32);
        }
    }

    #[test]
    fn toolbar_controls_do_not_overlap_at_the_smallest_window() {
        // The smallest window Voyager allows (240 x 180 pt, less its titlebar).
        let layout = Layout::compute(Size::new(scale::pt(240), scale::pt(148)), true);
        let controls = [layout.sidebar_toggle, layout.back, layout.forward, layout.title_area, layout.view_group, layout.open];
        for pair in controls.windows(2) {
            assert!(pair[0].origin.x + pair[0].size.width as i32 <= pair[1].origin.x);
        }
    }

    #[test]
    fn the_open_button_gives_way_in_a_narrow_window() {
        let wide = Layout::compute(Size::new(scale::pt(430), scale::pt(268)), true);
        let narrow = Layout::compute(Size::new(scale::pt(300), scale::pt(268)), true);
        assert!(wide.open.size.width > 0);
        assert_eq!(narrow.open.size.width, 0);
        // Nothing can land on a zero-width Open.
        assert!(!narrow.open.contains(Point::new(narrow.open.origin.x, narrow.open.origin.y)));
    }

    #[test]
    fn the_sidebar_is_proportional_within_limits() {
        assert_eq!(sidebar_width_for(scale::pt(100)), scale::pt(104));
        assert_eq!(sidebar_width_for(scale::pt(430)), scale::pt(430) * 30 / 100);
        assert_eq!(sidebar_width_for(scale::pt(2000)), scale::pt(168));
    }

    #[test]
    fn body_regions_stack_without_overlapping() {
        let layout = Layout::compute(CONTENT_SIZE, true);
        assert_eq!(layout.content.origin.y, layout.toolbar.size.height as i32);
        assert_eq!(layout.status.origin.y, layout.content.origin.y + layout.content.size.height as i32);
        assert_eq!(layout.status.origin.y + layout.status.size.height as i32, CONTENT_SIZE.height as i32);
    }

    #[test]
    fn sidebar_rows_stay_inside_the_sidebar_and_in_order() {
        let layout = Layout::compute(CONTENT_SIZE, true);
        let mut previous_bottom = layout.sidebar.origin.y;
        for index in 0..DESTINATIONS.len() {
            let row = layout.sidebar_row(index);
            assert!(row.origin.x >= layout.sidebar.origin.x);
            assert!(row.origin.x + row.size.width as i32 <= layout.sidebar.origin.x + layout.sidebar.size.width as i32);
            assert!(row.origin.y >= previous_bottom);
            previous_bottom = row.origin.y + row.size.height as i32;
        }
        assert!(previous_bottom <= layout.sidebar.origin.y + layout.sidebar.size.height as i32);
    }

    #[test]
    fn section_headings_line_up_with_the_row_glyphs() {
        let layout = Layout::compute(CONTENT_SIZE, true);
        for index in 0..DESTINATIONS.len() {
            let heading = layout.sidebar_section_header(index);
            let glyph_x = layout.sidebar_row(index).origin.x + sidebar_glyph_inset();
            assert_eq!(heading.origin.x, glyph_x);
            // And they end where the rows do.
            let row = layout.sidebar_row(index);
            assert_eq!(heading.origin.x + heading.size.width as i32, row.origin.x + row.size.width as i32);
        }
    }

    #[test]
    fn a_new_section_starts_below_a_heading() {
        let layout = Layout::compute(CONTENT_SIZE, true);
        for index in 1..DESTINATIONS.len() {
            if DESTINATIONS[index].section != DESTINATIONS[index - 1].section {
                let header = layout.sidebar_section_header(index);
                let previous = layout.sidebar_row(index - 1);
                assert!(header.origin.y >= previous.origin.y + previous.size.height as i32);
                assert!(layout.sidebar_row(index).origin.y >= header.origin.y + header.size.height as i32);
            }
        }
    }
}
