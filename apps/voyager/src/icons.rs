//! Procedural icons and glyphs.
//!
//! Every icon is described on a 128-unit square and scaled to whatever
//! physical size it is asked for, so the same code gives a crisp 16 px
//! sidebar glyph at 1x, a 32 px one at 2x, and a 48 pt Finder-style file icon
//! in the grid view. Shapes come from `draw2d`: anti-aliased rounded rects,
//! convex polygons and round-capped strokes.
//!
//! Two families:
//! - *file icons* ([`draw_file_icon`]): coloured, Finder-style folder, page and
//!   photo, used in lists and the grid;
//! - *glyphs* (chevrons, view modes, sidebar toggle, drive, ...): one colour,
//!   drawn with strokes and fills, used in the toolbar and sidebar.

use ui::prelude::*;
use ui::render::Canvas;

use crate::draw2d::{self, fx, Fx, FxPoint};
use crate::theme;

/// Which picture a file gets.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FileIcon {
    Folder,
    Image,
    /// A gradient tile with a waveform on it.
    Audio,
    /// A page with lines of text on it.
    Document,
    /// A blank page with a folded corner.
    Generic,
    /// An app with no icon of its own: a plain rounded tile.
    Application,
}

/// A square drawing area with the 128-unit design grid laid over it.
#[derive(Clone, Copy)]
struct Grid {
    x: i32,
    y: i32,
    size: i32,
}

impl Grid {
    /// The largest square centred in `rect`.
    fn new(rect: Rect) -> Self {
        let size = rect.size.width.min(rect.size.height) as i32;
        Self {
            x: rect.origin.x + (rect.size.width as i32 - size) / 2,
            y: rect.origin.y + (rect.size.height as i32 - size) / 2,
            size,
        }
    }

    /// A design-grid point, in fixed point.
    fn p(&self, u: i32, v: i32) -> FxPoint {
        (fx(self.x) + u * self.size / 2, fx(self.y) + v * self.size / 2)
    }

    /// A design-grid length, in fixed point.
    fn len(&self, units: i32) -> Fx {
        units * self.size / 2
    }

    fn px_x(&self, u: i32) -> i32 {
        self.x + (u * self.size + 64) / 128
    }

    fn px_y(&self, v: i32) -> i32 {
        self.y + (v * self.size + 64) / 128
    }

    /// The pixel rect covering design-grid (`u0`, `v0`) to (`u1`, `v1`).
    fn rect(&self, u0: i32, v0: i32, u1: i32, v1: i32) -> Rect {
        let (left, top) = (self.px_x(u0), self.px_y(v0));
        let (right, bottom) = (self.px_x(u1), self.px_y(v1));
        Rect::new(left, top, (right - left).max(0) as u32, (bottom - top).max(0) as u32)
    }

    fn radius(&self, units: i32) -> u32 {
        ((units * self.size + 64) / 128).max(0) as u32
    }
}

// ---- file icons ------------------------------------------------------------

const FOLDER_BACK: Color = Color::rgb(66, 146, 236);
const FOLDER_FRONT_TOP: Color = Color::rgb(128, 204, 252);
const FOLDER_FRONT_BOTTOM: Color = Color::rgb(62, 150, 242);
const PAGE_BORDER: Color = Color::rgb(198, 202, 210);
const PAGE_FOLD: Color = Color::rgb(224, 228, 236);
const PAGE_LINE: Color = Color::rgb(178, 183, 194);
const SKY_TOP: Color = Color::rgb(126, 196, 252);
const SKY_BOTTOM: Color = Color::rgb(208, 236, 255);

/// Draw the Finder-style icon for `icon` into the square centred in `rect`.
pub fn draw_file_icon(canvas: &mut dyn Canvas, rect: Rect, icon: FileIcon) {
    let grid = Grid::new(rect);
    if grid.size < 4 {
        return;
    }

    match icon {
        FileIcon::Folder => draw_folder(canvas, &grid),
        FileIcon::Image => draw_image(canvas, &grid),
        FileIcon::Audio => draw_audio(canvas, &grid),
        FileIcon::Document => draw_page(canvas, &grid, true),
        FileIcon::Generic => draw_page(canvas, &grid, false),
        FileIcon::Application => draw_app(canvas, &grid),
    }
}

fn draw_app(canvas: &mut dyn Canvas, grid: &Grid) {
    let tile = grid.rect(14, 14, 114, 114);
    draw2d::fill_round_rect(canvas, tile, grid.radius(24), Color::rgb(184, 189, 198));
    let inner = grid.rect(42, 42, 86, 86);
    draw2d::fill_round_rect(canvas, inner, grid.radius(10), Color::rgb(236, 238, 242));
}

fn draw_folder(canvas: &mut dyn Canvas, grid: &Grid) {
    // Back panel with its tab, then the lighter front on top.
    draw2d::fill_round_rect(canvas, grid.rect(8, 16, 62, 46), grid.radius(10), FOLDER_BACK);
    draw2d::fill_round_rect(canvas, grid.rect(8, 28, 120, 108), grid.radius(12), FOLDER_BACK);

    let front = grid.rect(8, 42, 120, 110);
    let (top, height) = (front.origin.y, front.size.height as i32);
    draw2d::fill_round_rect_shaded(canvas, front, grid.radius(12), &|y| draw2d::mix(FOLDER_FRONT_TOP, FOLDER_FRONT_BOTTOM, y - top, height));

    // A one-pixel highlight along the front's top edge.
    draw2d::stroke_line(canvas, grid.p(20, 43), grid.p(108, 43), theme::glyph_stroke() / 2, Color::rgba(255, 255, 255, 110));
}

fn draw_page(canvas: &mut dyn Canvas, grid: &Grid, lines: bool) {
    // A page with chamfered corners and one folded corner.
    let outline = [
        grid.p(34, 8),
        grid.p(78, 8),
        grid.p(102, 32),
        grid.p(102, 112),
        grid.p(94, 120),
        grid.p(34, 120),
        grid.p(26, 112),
        grid.p(26, 16),
    ];
    let border = theme::glyph_stroke() * 2 / 3;

    draw2d::fill_convex(canvas, &outline, Color::WHITE);
    draw2d::stroke_polygon(canvas, &outline, border, PAGE_BORDER);

    let fold = [grid.p(78, 8), grid.p(102, 32), grid.p(78, 32)];
    draw2d::fill_convex(canvas, &fold, PAGE_FOLD);
    draw2d::stroke_polyline(canvas, &[grid.p(78, 8), grid.p(78, 32), grid.p(102, 32)], border, PAGE_BORDER);

    if lines {
        let width = grid.len(6);
        for (v, end) in [(54, 88), (68, 88), (82, 88), (96, 66)] {
            draw2d::stroke_line(canvas, grid.p(40, v), grid.p(end, v), width, PAGE_LINE);
        }
    }
}

fn draw_image(canvas: &mut dyn Canvas, grid: &Grid) {
    // A photo: a white card, the picture inset in it, a sun and two hills.
    let card = grid.rect(10, 22, 118, 106);
    draw2d::bordered_round_rect(canvas, card, grid.radius(12), PAGE_BORDER, Color::WHITE, theme::hairline());

    let picture = grid.rect(18, 30, 110, 98);
    let (top, height) = (picture.origin.y, picture.size.height as i32);
    draw2d::fill_round_rect_shaded(canvas, picture, grid.radius(6), &|y| draw2d::mix(SKY_TOP, SKY_BOTTOM, y - top, height));

    draw2d::fill_disc(canvas, grid.p(90, 50), grid.len(8), Color::rgb(255, 208, 84));
    draw2d::fill_convex(canvas, &[grid.p(28, 98), grid.p(56, 60), grid.p(84, 98)], Color::rgb(84, 176, 120));
    draw2d::fill_convex(canvas, &[grid.p(58, 98), grid.p(82, 72), grid.p(102, 98)], Color::rgb(52, 148, 98));
}

const AUDIO_TOP: Color = Color::rgb(255, 122, 160);
const AUDIO_BOTTOM: Color = Color::rgb(156, 88, 236);

fn draw_audio(canvas: &mut dyn Canvas, grid: &Grid) {
    // A rounded tile with five waveform bars, tallest in the middle.
    let tile = grid.rect(12, 12, 116, 116);
    let (top, height) = (tile.origin.y, tile.size.height as i32);
    draw2d::fill_round_rect_shaded(canvas, tile, grid.radius(26), &|y| draw2d::mix(AUDIO_TOP, AUDIO_BOTTOM, y - top, height));

    for (u, half) in [(32, 26), (48, 46), (64, 70), (80, 46), (96, 26)] {
        draw2d::stroke_line(canvas, grid.p(u, 64 - half / 2 - 4), grid.p(u, 64 + half / 2 + 4), grid.len(9), Color::WHITE);
    }
}

// ---- glyphs ----------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
}

fn stroke(scale_units: i32) -> Fx {
    theme::glyph_stroke() * scale_units / 10
}

/// A `<` or `>` chevron.
pub fn draw_chevron(canvas: &mut dyn Canvas, rect: Rect, direction: Direction, color: Color) {
    let grid = Grid::new(rect);
    let (near, far) = match direction {
        Direction::Left => (76, 44),
        Direction::Right => (44, 76),
    };
    let points = [grid.p(near, 22), grid.p(far, 64), grid.p(near, 106)];
    draw2d::stroke_polyline(canvas, &points, stroke(13), color);
}

/// Finder's "show sidebar" symbol: a window outline with a divider and a few
/// lines in its left pane.
pub fn draw_sidebar_glyph(canvas: &mut dyn Canvas, rect: Rect, color: Color) {
    let grid = Grid::new(rect);
    draw2d::stroke_round_rect(canvas, grid.rect(6, 20, 122, 108), grid.radius(18), stroke(11), color);
    draw2d::stroke_line(canvas, grid.p(48, 24), grid.p(48, 104), stroke(11), color);
    for v in [50, 64, 78] {
        draw2d::stroke_line(canvas, grid.p(22, v), grid.p(34, v), stroke(9), color);
    }
}

/// Four rounded squares: the icon-grid view.
pub fn draw_grid_glyph(canvas: &mut dyn Canvas, rect: Rect, color: Color) {
    let grid = Grid::new(rect);
    for (u, v) in [(16, 16), (72, 16), (16, 72), (72, 72)] {
        draw2d::fill_round_rect(canvas, grid.rect(u, v, u + 40, v + 40), grid.radius(9), color);
    }
}

/// Three dotted rows: the list view.
pub fn draw_list_glyph(canvas: &mut dyn Canvas, rect: Rect, color: Color) {
    let grid = Grid::new(rect);
    for v in [30, 64, 98] {
        draw2d::fill_disc(canvas, grid.p(20, v), grid.len(8), color);
        draw2d::stroke_line(canvas, grid.p(44, v), grid.p(112, v), stroke(11), color);
    }
}

/// A single-colour folder, for sidebar destinations.
pub fn draw_folder_glyph(canvas: &mut dyn Canvas, rect: Rect, color: Color) {
    let grid = Grid::new(rect);
    draw2d::fill_round_rect(canvas, grid.rect(8, 22, 62, 54), grid.radius(11), color);
    draw2d::fill_round_rect(canvas, grid.rect(8, 34, 120, 108), grid.radius(14), color);
}

/// A disk drive, for volumes.
pub fn draw_drive_glyph(canvas: &mut dyn Canvas, rect: Rect, color: Color) {
    let grid = Grid::new(rect);
    draw2d::fill_round_rect(canvas, grid.rect(6, 38, 122, 98), grid.radius(16), color);
    draw2d::stroke_line(canvas, grid.p(22, 66), grid.p(66, 66), stroke(9), draw2d::mix(color, Color::WHITE, 6, 10));
    draw2d::fill_disc(canvas, grid.p(100, 66), grid.len(7), Color::WHITE);
}

/// A chevron for "this row opens", small and quiet.
pub fn draw_disclosure(canvas: &mut dyn Canvas, rect: Rect, color: Color) {
    let grid = Grid::new(rect);
    let points = [grid.p(50, 30), grid.p(80, 64), grid.p(50, 98)];
    draw2d::stroke_polyline(canvas, &points, stroke(11), color);
}
