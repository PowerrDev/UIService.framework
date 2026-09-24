//! Anti-aliased 2D shapes for Voyager, built on `Canvas::blend_pixel`.
//!
//! `Canvas` only offers filled rects, hard-edged rounded rects and a circle,
//! and `Frame` has no stroke or path primitive at all. Icons and crisp
//! rounded controls need both, so this module adds the missing pieces:
//! round-capped strokes, convex polygons, and rounded rects/outlines whose
//! edges are anti-aliased.
//!
//! Everything is integer-only: the freestanding target is soft-float, where
//! every `f32` operation is a library call. Coordinates are 26.6 fixed point
//! ([`Fx`]: 64 units per pixel), coverage is 0..=64, and distances come from
//! [`isqrt`]. A pixel's coverage is how far its centre is inside the shape,
//! clamped to a one-pixel ramp, which is exact for a straight edge and close
//! enough at corners for shapes this small.
//!
//! These draw into the window's interior only. The window's outer corners are
//! transported to WindowServer through an exact colour key, so a blended
//! pixel there would come back opaque (see `Surface::fill_rounded_rect`); the
//! shapes here never touch those edges.

use ui::prelude::*;
use ui::render::Canvas;

/// A 26.6 fixed-point coordinate: 64 units to the pixel.
pub type Fx = i32;

const FX_SHIFT: i32 = 6;
pub const FX_ONE: i32 = 1 << FX_SHIFT;
const FX_HALF: i32 = FX_ONE / 2;

pub const fn fx(pixels: i32) -> Fx {
    pixels << FX_SHIFT
}

pub type FxPoint = (Fx, Fx);

/// Integer square root, rounding down.
pub fn isqrt(value: u64) -> u64 {
    if value < 2 {
        return value;
    }

    let mut root = value;
    let mut next = (root + 1) / 2;
    while next < root {
        root = next;
        next = (root + value / root) / 2;
    }
    root
}

/// Blend `color` over the pixel at (`x`, `y`), scaled by `coverage` (0..=64).
fn plot(canvas: &mut dyn Canvas, x: i32, y: i32, color: Color, coverage: i32) {
    if coverage <= 0 {
        return;
    }

    let alpha = if coverage >= FX_ONE {
        color.alpha
    } else {
        (color.alpha as i32 * coverage / FX_ONE) as u8
    };
    if alpha != 0 {
        canvas.blend_pixel(Point::new(x, y), color.with_alpha(alpha));
    }
}

fn coverage_from_distance(inside: i32) -> i32 {
    (inside + FX_HALF).clamp(0, FX_ONE)
}

/// `rect`, filled with `color` blended over what is there. Opaque colours take
/// the fast path.
pub fn fill_rect_blend(canvas: &mut dyn Canvas, rect: Rect, color: Color) {
    if rect.size.width == 0 || rect.size.height == 0 {
        return;
    }
    if color.alpha == 255 {
        canvas.fill_rect(rect, color);
        return;
    }
    for y in rect.origin.y..rect.origin.y + rect.size.height as i32 {
        for x in rect.origin.x..rect.origin.x + rect.size.width as i32 {
            canvas.blend_pixel(Point::new(x, y), color);
        }
    }
}

// ---- strokes ---------------------------------------------------------------

fn segment_distance(p: FxPoint, a: FxPoint, b: FxPoint) -> i32 {
    let (abx, aby) = ((b.0 - a.0) as i64, (b.1 - a.1) as i64);
    let (apx, apy) = ((p.0 - a.0) as i64, (p.1 - a.1) as i64);
    let length_squared = abx * abx + aby * aby;

    let closest = if length_squared == 0 {
        a
    } else {
        let dot = apx * abx + apy * aby;
        if dot <= 0 {
            a
        } else if dot >= length_squared {
            b
        } else {
            (a.0 + (abx * dot / length_squared) as i32, a.1 + (aby * dot / length_squared) as i32)
        }
    };

    let (dx, dy) = ((p.0 - closest.0) as i64, (p.1 - closest.1) as i64);
    isqrt((dx * dx + dy * dy) as u64) as i32
}

/// A line of `width` (fixed point) with round caps.
pub fn stroke_line(canvas: &mut dyn Canvas, a: FxPoint, b: FxPoint, width: Fx, color: Color) {
    let half = width / 2;
    let left = (a.0.min(b.0) - half) >> FX_SHIFT;
    let right = ((a.0.max(b.0) + half) >> FX_SHIFT) + 1;
    let top = (a.1.min(b.1) - half) >> FX_SHIFT;
    let bottom = ((a.1.max(b.1) + half) >> FX_SHIFT) + 1;

    for y in top..=bottom {
        for x in left..=right {
            let centre = ((x << FX_SHIFT) + FX_HALF, (y << FX_SHIFT) + FX_HALF);
            let distance = segment_distance(centre, a, b);
            plot(canvas, x, y, color, coverage_from_distance(half - distance));
        }
    }
}

pub fn stroke_polyline(canvas: &mut dyn Canvas, points: &[FxPoint], width: Fx, color: Color) {
    for pair in points.windows(2) {
        stroke_line(canvas, pair[0], pair[1], width, color);
    }
}

/// A closed outline through `points`.
pub fn stroke_polygon(canvas: &mut dyn Canvas, points: &[FxPoint], width: Fx, color: Color) {
    if points.len() < 2 {
        return;
    }
    stroke_polyline(canvas, points, width, color);
    stroke_line(canvas, points[points.len() - 1], points[0], width, color);
}

// ---- polygons --------------------------------------------------------------

/// Fill a convex polygon (up to `MAX_POLYGON_POINTS` corners, either winding).
pub fn fill_convex(canvas: &mut dyn Canvas, points: &[FxPoint], color: Color) {
    fill_convex_shaded(canvas, points, &|_| color);
}

pub const MAX_POLYGON_POINTS: usize = 16;

/// As [`fill_convex`], with the colour chosen per pixel row by `shade` (the
/// row's y in pixels), for vertical gradients.
pub fn fill_convex_shaded(canvas: &mut dyn Canvas, points: &[FxPoint], shade: &dyn Fn(i32) -> Color) {
    let count = points.len().min(MAX_POLYGON_POINTS);
    if count < 3 {
        return;
    }

    let mut area: i64 = 0;
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
    for index in 0..count {
        let (a, b) = (points[index], points[(index + 1) % count]);
        area += a.0 as i64 * b.1 as i64 - b.0 as i64 * a.1 as i64;
        min_x = min_x.min(a.0);
        max_x = max_x.max(a.0);
        min_y = min_y.min(a.1);
        max_y = max_y.max(a.1);
    }
    let sign: i64 = if area >= 0 { 1 } else { -1 };

    let mut lengths = [1i64; MAX_POLYGON_POINTS];
    for index in 0..count {
        let (a, b) = (points[index], points[(index + 1) % count]);
        let (dx, dy) = ((b.0 - a.0) as i64, (b.1 - a.1) as i64);
        lengths[index] = (isqrt((dx * dx + dy * dy) as u64) as i64).max(1);
    }

    for y in (min_y >> FX_SHIFT)..=((max_y >> FX_SHIFT) + 1) {
        let color = shade(y);
        for x in (min_x >> FX_SHIFT)..=((max_x >> FX_SHIFT) + 1) {
            let (px, py) = ((x << FX_SHIFT) + FX_HALF, (y << FX_SHIFT) + FX_HALF);
            let mut inside = i32::MAX;

            for index in 0..count {
                let (a, b) = (points[index], points[(index + 1) % count]);
                let cross = (b.0 - a.0) as i64 * (py - a.1) as i64 - (b.1 - a.1) as i64 * (px - a.0) as i64;
                inside = inside.min((sign * cross / lengths[index]) as i32);
                if inside < -FX_HALF {
                    break;
                }
            }

            plot(canvas, x, y, color, coverage_from_distance(inside));
        }
    }
}

// ---- rounded rects ---------------------------------------------------------

/// A rounded rect filled with `color`, corners anti-aliased.
pub fn fill_round_rect(canvas: &mut dyn Canvas, rect: Rect, radius: u32, color: Color) {
    fill_round_rect_shaded(canvas, rect, radius, &|_| color);
}

/// As [`fill_round_rect`], with the colour chosen per pixel row by `shade`
/// (the row's y in pixels), for vertical gradients.
pub fn fill_round_rect_shaded(canvas: &mut dyn Canvas, rect: Rect, radius: u32, shade: &dyn Fn(i32) -> Color) {
    let (width, height) = (rect.size.width as i32, rect.size.height as i32);
    if width <= 0 || height <= 0 {
        return;
    }

    let radius = (radius as i32).min(width / 2).min(height / 2);
    let (left, top) = (rect.origin.x, rect.origin.y);

    for row in 0..height {
        let y = top + row;
        let color = shade(y);
        let from_edge = if row < radius { radius - row } else if row >= height - radius { row - (height - radius) + 1 } else { 0 };

        if from_edge == 0 || radius == 0 {
            fill_span(canvas, left, y, width, color);
            continue;
        }

        // Inside a corner row: the straight middle, then a few AA pixels each side.
        let centre_y = if row < radius { (top + radius) << FX_SHIFT } else { (top + height - radius) << FX_SHIFT };
        let py = (y << FX_SHIFT) + FX_HALF;
        let dy = (py - centre_y) as i64;

        let mut solid_from = left + radius;
        let mut solid_to = left + width - radius;
        for column in 0..radius {
            let left_x = left + column;
            let right_x = left + width - 1 - column;
            let left_centre = (left + radius) << FX_SHIFT;
            let right_centre = (left + width - radius) << FX_SHIFT;
            let coverage_of = |x: i32, centre: i32| {
                let dx = (((x << FX_SHIFT) + FX_HALF) - centre) as i64;
                let distance = isqrt((dx * dx + dy * dy) as u64) as i32;
                coverage_from_distance((radius << FX_SHIFT) - distance)
            };
            let (left_cov, right_cov) = (coverage_of(left_x, left_centre), coverage_of(right_x, right_centre));

            plot(canvas, left_x, y, color, left_cov);
            plot(canvas, right_x, y, color, right_cov);

            if left_cov >= FX_ONE && right_cov >= FX_ONE {
                solid_from = left_x + 1;
                solid_to = right_x;
                break;
            }
        }
        if solid_to > solid_from {
            fill_span(canvas, solid_from, y, solid_to - solid_from, color);
        }
    }
}

fn fill_span(canvas: &mut dyn Canvas, x: i32, y: i32, width: i32, color: Color) {
    if width > 0 {
        fill_rect_blend(canvas, Rect::new(x, y, width as u32, 1), color);
    }
}

/// Signed distance (fixed point, positive inside) from a pixel centre to the
/// edge of a rounded rect.
fn round_rect_inside(rect: Rect, radius: i32, px: i32, py: i32) -> i32 {
    let half_w = (rect.size.width as i32) << (FX_SHIFT - 1);
    let half_h = (rect.size.height as i32) << (FX_SHIFT - 1);
    let centre_x = (rect.origin.x << FX_SHIFT) + half_w;
    let centre_y = (rect.origin.y << FX_SHIFT) + half_h;
    let radius = radius << FX_SHIFT;

    let qx = (px - centre_x).abs() - (half_w - radius);
    let qy = (py - centre_y).abs() - (half_h - radius);

    let outside = if qx > 0 && qy > 0 {
        isqrt((qx as i64 * qx as i64 + qy as i64 * qy as i64) as u64) as i32
    } else {
        qx.max(0).max(qy.max(0))
    };
    let inside_depth = qx.max(qy).min(0);

    radius - (outside + inside_depth)
}

/// The outline of a rounded rect, `width` (fixed point) thick, drawn inside
/// the rect's edge.
pub fn stroke_round_rect(canvas: &mut dyn Canvas, rect: Rect, radius: u32, width: Fx, color: Color) {
    let radius = (radius as i32).min(rect.size.width as i32 / 2).min(rect.size.height as i32 / 2);
    let half = width / 2;

    for y in rect.origin.y..rect.origin.y + rect.size.height as i32 {
        for x in rect.origin.x..rect.origin.x + rect.size.width as i32 {
            let inside = round_rect_inside(rect, radius, (x << FX_SHIFT) + FX_HALF, (y << FX_SHIFT) + FX_HALF);
            // The band centred `half` inside the edge, `half` either side.
            plot(canvas, x, y, color, coverage_from_distance(half - (inside - half).abs()));
        }
    }
}

/// A filled rounded rect with a hairline outline: the outer shape in the
/// border colour, the inner one on top.
pub fn bordered_round_rect(canvas: &mut dyn Canvas, rect: Rect, radius: u32, border: Color, fill: Color, border_width: u32) {
    fill_round_rect(canvas, rect, radius, border);
    let inset = border_width.min(rect.size.width / 2).min(rect.size.height / 2);
    let inner = Rect::new(
        rect.origin.x + inset as i32,
        rect.origin.y + inset as i32,
        rect.size.width.saturating_sub(inset * 2),
        rect.size.height.saturating_sub(inset * 2),
    );
    fill_round_rect(canvas, inner, radius.saturating_sub(inset), fill);
}

/// A filled anti-aliased circle at a fixed-point centre.
pub fn fill_disc(canvas: &mut dyn Canvas, centre: FxPoint, radius: Fx, color: Color) {
    let left = (centre.0 - radius) >> FX_SHIFT;
    let right = ((centre.0 + radius) >> FX_SHIFT) + 1;
    let top = (centre.1 - radius) >> FX_SHIFT;
    let bottom = ((centre.1 + radius) >> FX_SHIFT) + 1;

    for y in top..=bottom {
        for x in left..=right {
            let (dx, dy) = ((((x << FX_SHIFT) + FX_HALF) - centre.0) as i64, (((y << FX_SHIFT) + FX_HALF) - centre.1) as i64);
            let distance = isqrt((dx * dx + dy * dy) as u64) as i32;
            plot(canvas, x, y, color, coverage_from_distance(radius - distance));
        }
    }
}

/// Linear blend of two colours, `step` of `steps` of the way from `from` to `to`.
pub fn mix(from: Color, to: Color, step: i32, steps: i32) -> Color {
    let steps = steps.max(1);
    let step = step.clamp(0, steps);
    let lerp = |a: u8, b: u8| ((a as i32 * (steps - step) + b as i32 * step) / steps) as u8;
    Color::rgba(lerp(from.red, to.red), lerp(from.green, to.green), lerp(from.blue, to.blue), lerp(from.alpha, to.alpha))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isqrt_is_exact_on_squares_and_floors_between() {
        for value in [0u64, 1, 2, 3, 4, 15, 16, 17, 99, 100, 1 << 40] {
            let root = isqrt(value);
            assert!(root * root <= value);
            assert!((root + 1) * (root + 1) > value);
        }
    }

    #[test]
    fn mix_hits_both_ends_and_the_middle() {
        let (black, white) = (Color::rgb(0, 0, 0), Color::rgb(200, 100, 50));
        assert_eq!(mix(black, white, 0, 4), black);
        assert_eq!(mix(black, white, 4, 4), white);
        assert_eq!(mix(black, white, 2, 4), Color::rgb(100, 50, 25));
    }

    #[test]
    fn inside_distance_is_positive_inside_and_negative_outside() {
        let rect = Rect::new(0, 0, 20, 20);
        let centre = (10 << FX_SHIFT, 10 << FX_SHIFT);
        assert!(round_rect_inside(rect, 4, centre.0, centre.1) > 0);
        assert!(round_rect_inside(rect, 4, -5 << FX_SHIFT, centre.1) < 0);
        // A corner pixel of a rounded rect is outside; the same pixel of a square one is not.
        assert!(round_rect_inside(rect, 6, FX_HALF, FX_HALF) < FX_HALF);
        assert!(round_rect_inside(rect, 0, FX_HALF, FX_HALF) >= FX_HALF);
    }
}
