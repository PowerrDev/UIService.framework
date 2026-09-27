//! Drawing the Dock's own pixels: the icons, the running-app dots, the
//! dividers and the Trash, over transparency (ARGB8888, straight alpha).
//! The panel's translucent material under them is the desktop's (it has the
//! wallpaper); see UIService's `ui-service-nxu/src/dockhost.rs`.

use crate::layout::{Layout, Slot};

/// How one app tile shows right now.
#[derive(Clone, Copy)]
pub struct TileView<'a> {
    /// `tile` x `tile` ARGB pixels, or `None` for a generic icon.
    pub icon: Option<&'a [u32]>,
    pub running: bool,
    pub pressed: bool,
}

const DOT: u32 = 0x00_1E_1E_22;
const DOT_ALPHA: u32 = 0xD8;
const DIVIDER: u32 = 0x00_00_00_00;
const DIVIDER_ALPHA: u32 = 0x40;

/// Straight-alpha "over": `over` drawn on `under`.
pub fn over(under: u32, over: u32) -> u32 {
    let sa = over >> 24;
    if sa == 0 {
        return under;
    }
    if sa == 255 {
        return over;
    }
    let da = under >> 24;
    let out_a = sa * 255 + da * (255 - sa); // x255
    if out_a == 0 {
        return 0;
    }
    let channel = |shift: u32| {
        let s = (over >> shift) & 0xFF;
        let d = (under >> shift) & 0xFF;
        ((s * sa * 255 + d * da * (255 - sa)) / out_a).min(255)
    };
    ((out_a / 255).min(255) << 24) | (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

/// Fill the pixels a shape covers, antialiased with 4x4 samples. `inside`
/// answers for a point in 1/4-pixel units.
pub fn fill_shape(out: &mut [u32], stride: usize, x0: i32, y0: i32, width: i32, height: i32, color: u32, inside: impl Fn(i32, i32) -> bool) {
    let rows = out.len() / stride.max(1);
    for y in 0..height {
        for x in 0..width {
            let (px, py) = (x0 + x, y0 + y);
            if px < 0 || py < 0 || px as usize >= stride || py as usize >= rows {
                continue;
            }
            let mut hits = 0u32;
            for sy in 0..4 {
                for sx in 0..4 {
                    if inside(x * 4 + sx, y * 4 + sy) {
                        hits += 1;
                    }
                }
            }
            if hits == 0 {
                continue;
            }
            let alpha = ((color >> 24) * hits + 8) / 16;
            let index = py as usize * stride + px as usize;
            out[index] = over(out[index], (alpha << 24) | (color & 0x00FF_FFFF));
        }
    }
}

/// The Dock's pixels for `layout` into `out` (`layout.width` wide).
/// `tiles[i]` is `Slot::App(i)`; `trash` is the Trash's icon.
pub fn draw(layout: &Layout, tiles: &[TileView<'_>], trash: &[u32], out: &mut [u32]) {
    let width = layout.width as usize;
    let height = layout.height as usize;
    if out.len() < width * height {
        return;
    }
    let out = &mut out[..width * height];
    out.fill(0);

    let metrics = layout.metrics;
    let tile = metrics.tile as usize;
    let top = layout.tile_top() as usize;

    for &(slot, left) in layout.slots() {
        let left = left as usize;
        match slot {
            Slot::App(index) => {
                let Some(view) = tiles.get(index) else { continue; };
                match view.icon {
                    Some(icon) => blit(out, width, left, top, tile, icon, view.pressed),
                    None => generic_icon(out, width, left, top, tile, view.pressed),
                }
                if view.running {
                    let cx = (left * 2 + tile) as i32 * 2; // quarter pixels
                    let cy = (top + tile + metrics.dot_offset as usize) as i32 * 4;
                    let r = (metrics.dot_radius_x16 / 4) as i32; // quarter pixels
                    let span = r / 4 + 2;
                    let (x0, y0) = (cx / 4 - span, cy / 4 - span);
                    fill_shape(out, width, x0, y0, span * 2 + 1, span * 2 + 1, (DOT_ALPHA << 24) | DOT, |x, y| {
                        let dx = x0 * 4 + x - cx + 2;
                        let dy = y0 * 4 + y - cy + 2;
                        dx * dx + dy * dy <= r * r
                    });
                }
            }
            Slot::Divider => {
                let thickness = (metrics.tile / 48).max(1) as usize;
                let from = top + tile / 10;
                let to = top + tile - tile / 10;
                for y in from..to.min(height) {
                    for x in left..(left + thickness).min(width) {
                        out[y * width + x] = (DIVIDER_ALPHA << 24) | DIVIDER;
                    }
                }
            }
            Slot::Trash => blit(out, width, left, top, tile, trash, false),
        }
    }
}

/// Copy a tile icon in, darkened while it is pressed (as macOS does).
fn blit(out: &mut [u32], stride: usize, left: usize, top: usize, tile: usize, icon: &[u32], pressed: bool) {
    if icon.len() < tile * tile {
        return;
    }
    let rows = out.len() / stride;
    for y in 0..tile {
        if top + y >= rows {
            break;
        }
        for x in 0..tile {
            if left + x >= stride {
                break;
            }
            let mut pixel = icon[y * tile + x];
            if pressed {
                let dim = |shift: u32| (((pixel >> shift) & 0xFF) * 150 / 255) << shift;
                pixel = (pixel & 0xFF00_0000) | dim(16) | dim(8) | dim(0);
            }
            out[(top + y) * stride + left + x] = pixel;
        }
    }
}

/// For an app whose icon would not load: a plain rounded tile.
fn generic_icon(out: &mut [u32], stride: usize, left: usize, top: usize, tile: usize, pressed: bool) {
    let size = tile as i32 * 4;
    let inset = size / 12;
    let radius = size * 22 / 100;
    let color = if pressed { 0xFF_7C_80_88 } else { 0xFF_B8_BD_C6 };
    fill_shape(out, stride, left as i32, top as i32, tile as i32, tile as i32, color, |x, y| {
        rounded_rect_contains(x, y, inset, inset, size - inset, size - inset, radius)
    });
}

/// Whether (x, y) is inside the rounded rectangle (left, top)-(right, bottom).
pub fn rounded_rect_contains(x: i32, y: i32, left: i32, top: i32, right: i32, bottom: i32, radius: i32) -> bool {
    if x < left || y < top || x >= right || y >= bottom {
        return false;
    }
    let cx = x.clamp(left + radius, right - radius - 1);
    let cy = y.clamp(top + radius, bottom - radius - 1);
    let (dx, dy) = (x - cx, y - cy);
    dx * dx + dy * dy <= radius * radius
}

/// The Trash: a translucent white bin with ribs, like the macOS one.
pub fn draw_trash(size: u32, out: &mut [u32]) {
    let tile = size as usize;
    if out.len() < tile * tile {
        return;
    }
    let out = &mut out[..tile * tile];
    out.fill(0);
    let s = size as i32 * 4; // quarter pixels
    let at = |fraction: i32| s * fraction / 100;

    // The body narrows a little towards the bottom.
    let (top, bottom) = (at(22), at(92));
    let (half_top, half_bottom) = (at(33), at(28));
    let center = s / 2;
    let body = move |x: i32, y: i32| -> bool {
        if y < top || y >= bottom {
            return false;
        }
        let t = (y - top) * 1000 / (bottom - top);
        let half = half_top + (half_bottom - half_top) * t / 1000;
        // Rounded bottom corners.
        let corner = at(7);
        if y > bottom - corner {
            let cx = if x < center { center - half + corner } else { center + half - corner };
            let dy = y - (bottom - corner);
            let dx = (x - cx).abs();
            if (x < center - half + corner || x > center + half - corner) && dx * dx + dy * dy > corner * corner {
                return false;
            }
        }
        (x - center).abs() <= half
    };
    fill_shape(out, tile, 0, 0, size as i32, size as i32, 0xC8_F4_F6_FA, body);
    // A soft shade down the right side for roundness.
    fill_shape(out, tile, 0, 0, size as i32, size as i32, 0x30_7A_84_94, move |x, y| body(x, y) && x > center + at(12));
    // Ribs.
    for rib in [-2, -1, 0, 1, 2] {
        let rib_x = center + rib * at(11);
        fill_shape(out, tile, 0, 0, size as i32, size as i32, 0x55_8C_94_A4, move |x, y| {
            body(x, y) && y > top + at(6) && y < bottom - at(6) && (x - rib_x).abs() <= at(1)
        });
    }
    // The rim.
    let (rim_top, rim_bottom) = (at(15), at(24));
    fill_shape(out, tile, 0, 0, size as i32, size as i32, 0xE6_FD_FD_FE, move |x, y| {
        rounded_rect_contains(x, y, center - at(37), rim_top, center + at(37), rim_bottom, at(4))
    });
    fill_shape(out, tile, 0, 0, size as i32, size as i32, 0x60_8C_94_A4, move |x, y| {
        rounded_rect_contains(x, y, center - at(37), rim_bottom - at(2), center + at(37), rim_bottom, at(1))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Metrics;
    use std::{vec, vec::Vec};

    #[test]
    fn over_blends_straight_alpha() {
        assert_eq!(over(0, 0x80FF0000), 0x80FF0000);
        assert_eq!(over(0xFF0000FF, 0xFFFF0000), 0xFFFF0000);
        let mixed = over(0xFF0000FF, 0x80FF0000);
        assert_eq!(mixed >> 24, 0xFF);
        assert!((mixed >> 16) & 0xFF > 0x70 && mixed & 0xFF > 0x70);
    }

    #[test]
    fn running_apps_get_a_dot_and_others_do_not() {
        let metrics = Metrics::new(48);
        let layout = Layout::new(metrics, &[Slot::App(0), Slot::App(1)]);
        let icon = vec![0xFF_20_60_C0u32; 48 * 48];
        let tiles = [
            TileView { icon: Some(&icon), running: true, pressed: false },
            TileView { icon: Some(&icon), running: false, pressed: false },
        ];
        let mut trash = vec![0u32; 48 * 48];
        draw_trash(48, &mut trash);
        let mut out: Vec<u32> = vec![0; (layout.width * layout.height) as usize];
        draw(&layout, &tiles, &trash, &mut out);

        let dot_y = (layout.tile_top() + 48 + metrics.dot_offset) as usize;
        let at = |index: usize| out[dot_y * layout.width as usize + layout.center(index) as usize];
        assert!(at(0) >> 24 > 0x80, "dot under the running app");
        assert_eq!(at(1), 0, "nothing under the other");
        // The icon itself was copied.
        let icon_pixel = out[(layout.tile_top() as usize + 10) * layout.width as usize + layout.center(1) as usize];
        assert_eq!(icon_pixel, 0xFF_20_60_C0);
    }

    #[test]
    fn the_trash_is_drawn_inside_its_tile() {
        let mut trash = vec![0u32; 64 * 64];
        draw_trash(64, &mut trash);
        assert!(trash[32 * 64 + 32] >> 24 > 0x80, "the body is solid in the middle");
        assert_eq!(trash[0], 0, "the corners stay clear");
    }
}
