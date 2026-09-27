#![allow(non_snake_case)]

use ui_core::{scale::pt, Color, Point, Rect, Size};
use ui_render::Canvas;

pub(crate) const TRANSPARENT_KEY: u32 = 0x00FF00FF;

const WALLPAPER_TOP: Color = Color::rgb(58, 78, 110);
const WALLPAPER_MIDDLE: Color = Color::rgb(39, 54, 79);
const WALLPAPER_BOTTOM: Color = Color::rgb(20, 27, 41);

/// The desktop wallpaper: the embedded image (see build.rs's
/// `embedded_background`, `BACKGROUND` in UIService.framework's own
/// Makefile), falling back to the procedural gradient above if none was
/// embedded at build time.
const DRAW_WALLPAPER: bool = true;
const DESKTOP_OFF_WHITE: Color = Color::rgb(245, 244, 240);

// Real Aqua's menu bar is 24pt at 1x; this one runs a step above that (30pt
// equivalent) since the first pass at 24pt-equivalent read as too small.
// Values are 1x design points, converted to physical pixels for the host's
// actual content scale through `ui_core::scale::pt` (see menubar.rs, which
// draws the bar itself).
pub(crate) fn menubar_height() -> u32 {
    pt(30)
}

pub(crate) mod generated_logo {
    include!(concat!(env!("OUT_DIR"), "/ui_menubar_logo.rs"));
}

fn lerp_channel(start: u8, end: u8, amount: u32, extent: u32) -> u8 {
    if extent == 0 { return start; }
    let start = start as i32;
    let delta = end as i32 - start;
    (start + delta * amount as i32 / extent as i32).clamp(0, 255) as u8
}

fn wallpaper_color(y: u32, height: u32) -> Color {
    if height <= 1 { return WALLPAPER_TOP; }

    let midpoint = height / 2;
    if y <= midpoint {
        let extent = midpoint.max(1);
        Color::rgb(
            lerp_channel(WALLPAPER_TOP.red, WALLPAPER_MIDDLE.red, y, extent),
            lerp_channel(WALLPAPER_TOP.green, WALLPAPER_MIDDLE.green, y, extent),
            lerp_channel(WALLPAPER_TOP.blue, WALLPAPER_MIDDLE.blue, y, extent),
        )
    } else {
        let amount = y - midpoint;
        let extent = height.saturating_sub(midpoint + 1).max(1);
        Color::rgb(
            lerp_channel(WALLPAPER_MIDDLE.red, WALLPAPER_BOTTOM.red, amount, extent),
            lerp_channel(WALLPAPER_MIDDLE.green, WALLPAPER_BOTTOM.green, amount, extent),
            lerp_channel(WALLPAPER_MIDDLE.blue, WALLPAPER_BOTTOM.blue, amount, extent),
        )
    }
}

mod generated_background {
    include!(concat!(env!("OUT_DIR"), "/ui_background.rs"));
}

/// Read one pixel (XRGB8888) from the embedded, build-time-downscaled
/// desktop background. `None` when no background asset was embedded.
fn background_pixel(x: u32, y: u32) -> Option<u32> {
    use generated_background::{BACKGROUND_DATA, BACKGROUND_HEIGHT, BACKGROUND_WIDTH};

    if x >= BACKGROUND_WIDTH || y >= BACKGROUND_HEIGHT {
        return None;
    }
    let index = (y as usize * BACKGROUND_WIDTH as usize + x as usize) * 4;
    let bytes = BACKGROUND_DATA.get(index..index + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Blit the embedded desktop background with a "cover" fit (scale to fill,
/// crop overflow) using integer-only nearest-neighbor sampling. Returns
/// `false` when no background asset was embedded at build time, so the
/// caller can fall back to the procedural gradient.
///
/// Fixed-point (16.16), not floating point: this target has no hardware FPU
/// (`aarch64-unknown-none-softfloat`), and this blit runs once at boot over
/// the full desktop, so soft-float division/multiplication per pixel would
/// cost a real, visible amount of boot time.
fn draw_background_image<C: Canvas>(canvas: &mut C, size: Size) -> bool {
    use generated_background::{BACKGROUND_HEIGHT, BACKGROUND_WIDTH};

    if BACKGROUND_WIDTH == 0 || BACKGROUND_HEIGHT == 0 || size.width == 0 || size.height == 0 {
        return false;
    }

    // "Cover": scale so the *tighter* axis (the smaller source-pixels-per-
    // dest-pixel ratio) lands exactly on the destination's edge, using every
    // row or column on that axis; the other axis then has source left over
    // outside the destination, which crop_x/crop_y below discard evenly from
    // each side. The larger ratio is `contain` (fits inside, may letterbox)
    // -- picking it here made a source noticeably wider than the destination
    // (this background is 1600x900 against a 1024x768 window) compute a
    // cropped_height *taller than the source actually is*, so every row past
    // the real image clamped to its last row, visibly squashed and repeated.
    const FIXED_SHIFT: u32 = 16;
    let scale_x = ((BACKGROUND_WIDTH as u64) << FIXED_SHIFT) / size.width as u64;
    let scale_y = ((BACKGROUND_HEIGHT as u64) << FIXED_SHIFT) / size.height as u64;
    let scale = scale_x.min(scale_y);

    let cropped_width = (size.width as u64 * scale) >> FIXED_SHIFT;
    let cropped_height = (size.height as u64 * scale) >> FIXED_SHIFT;
    let crop_x = (BACKGROUND_WIDTH as u64).saturating_sub(cropped_width) / 2;
    let crop_y = (BACKGROUND_HEIGHT as u64).saturating_sub(cropped_height) / 2;

    for y in 0..size.height {
        let source_y = (crop_y + ((y as u64 * scale) >> FIXED_SHIFT))
            .min(BACKGROUND_HEIGHT as u64 - 1) as u32;

        for x in 0..size.width {
            let source_x = (crop_x + ((x as u64 * scale) >> FIXED_SHIFT))
                .min(BACKGROUND_WIDTH as u64 - 1) as u32;

            if let Some(pixel) = background_pixel(source_x, source_y) {
                canvas.fill_rect(
                    Rect::new(x as i32, y as i32, 1, 1),
                    Color::from_xrgb8888(pixel),
                );
            }
        }
    }

    true
}

fn color_from_argb8888(value: u32) -> Color {
    Color::rgba(
        ((value >> 16) & 0xFF) as u8,
        ((value >> 8) & 0xFF) as u8,
        (value & 0xFF) as u8,
        (value >> 24) as u8,
    )
}

/// Alpha-blend the embedded, build-time-fitted menu bar logo at `origin`.
/// No-op when no logo asset was embedded at build time.
pub(crate) fn draw_logo<C: Canvas>(canvas: &mut C, origin: Point) {
    use generated_logo::{LOGO_DATA, LOGO_SIZE};

    for y in 0..LOGO_SIZE {
        for x in 0..LOGO_SIZE {
            let index = (y as usize * LOGO_SIZE as usize + x as usize) * 4;
            let Some(bytes) = LOGO_DATA.get(index..index + 4) else {
                continue;
            };
            let value = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            let color = color_from_argb8888(value);
            if color.alpha == 0 {
                continue;
            }

            canvas.blend_pixel(Point::new(origin.x + x as i32, origin.y + y as i32), color);
        }
    }
}

/// The desktop wallpaper alone, without the menu bar: what the desktop draws
/// once under everything, and what the login screen blurs behind itself.
pub(crate) fn UIDrawWallpaper<C: Canvas>(canvas: &mut C) {
    let size = canvas.size();

    if DRAW_WALLPAPER {
        if !draw_background_image(canvas, size) {
            for y in 0..size.height {
                canvas.fill_rect(
                    Rect::new(0, y as i32, size.width, 1),
                    wallpaper_color(y, size.height),
                );
            }
        }
    } else {
        canvas.fill(DESKTOP_OFF_WHITE);
    }
}
