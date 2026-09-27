#![no_std]

//! no_std TrueType/OpenType text parsing and rasterization.
//!
//! Glyphs are rasterized with exact-area coverage: every outline edge adds the
//! signed area it covers to the pixels it crosses, and a running sum along each
//! row turns that into a coverage value from 0 to 255 per pixel. That gives
//! smooth anti-aliasing at every angle (the previous 2x2 sampling had five
//! levels) in one pass over the edges, and it is pure integer maths: the
//! freestanding target is soft-float, where each `f32` operation is a library
//! call.
//!
//! A rasterized glyph is kept in a cache, keyed by glyph, size, weight and a
//! quarter-pixel horizontal phase, so text that is drawn again (every frame,
//! for every label) costs a blit of stored coverage instead of rebuilding and
//! rasterizing the outline. The cache lives in [`TextScratch`], which is
//! large and belongs in a static, never on a stack.

#[cfg(test)]
extern crate std;

mod bootstrap;

pub use bootstrap::BootstrapText;

use ttf_parser::{Face, GlyphId, OutlineBuilder};
use ui_core::{Color, Point, Size};
use ui_render::{Canvas, TextRenderer};

const SUBPIXEL: i32 = 64;
const QUAD_STEPS: i32 = 8;
const CUBIC_STEPS: i32 = 12;
const MAX_SEGMENTS: usize = 768;

/// The widest and tallest glyph, in pixels, that is rasterized (and cached).
/// A glyph outside it is skipped rather than clipped.
const MAX_GLYPH_DIM: usize = 128;
/// A bitmap is its bounding box plus one column for the area that spills
/// past the last edge, and one spare row.
const MAX_BITMAP_WIDTH: usize = MAX_GLYPH_DIM + 2;
const MAX_BITMAP_HEIGHT: usize = MAX_GLYPH_DIM + 1;
const ACCUM_CELLS: usize = MAX_BITMAP_WIDTH * MAX_BITMAP_HEIGHT + 8;

const CACHE_SLOTS: usize = 1024;
const POOL_BYTES: usize = 192 * 1024;

/// Q16 fixed point: the accumulation and edge maths.
const ONE: i64 = 1 << 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FontError {
    InvalidFont,
}

pub struct FontFamily<'a> {
    regular: Face<'a>,
    semibold: Option<Face<'a>>,
    cache_tag: u8,
}

impl<'a> FontFamily<'a> {
    pub fn from_bytes(regular: &'a [u8], semibold: Option<&'a [u8]>) -> Result<Self, FontError> {
        let regular = Face::parse(regular, 0).map_err(|_| FontError::InvalidFont)?;
        let semibold = match semibold {
            Some(data) if !data.is_empty() => Face::parse(data, 0).ok(),
            _ => None,
        };

        Ok(Self { regular, semibold, cache_tag: 0 })
    }

    /// Tells this family's glyphs apart from another family's in a shared
    /// [`TextScratch`]: glyph ids and sizes alone would collide (glyph 5 of
    /// one font is some other shape in the next). 0..=127; families drawn
    /// through the same scratch need different tags.
    pub fn with_cache_tag(mut self, tag: u8) -> Self {
        self.cache_tag = tag & 0x7F;
        self
    }

    fn face(&self, semibold: bool) -> &Face<'a> {
        if semibold {
            self.semibold.as_ref().unwrap_or(&self.regular)
        } else {
            &self.regular
        }
    }
}

#[derive(Clone, Copy)]
struct Segment {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl Segment {
    const ZERO: Self = Self {
        x0: 0,
        y0: 0,
        x1: 0,
        y1: 0,
    };
}

/// One cached glyph: where its coverage bitmap is in the pool, how big it is,
/// and where it sits relative to the pen and baseline. `key == 0` is a free slot.
#[derive(Clone, Copy)]
struct CacheEntry {
    key: u64,
    offset: u32,
    width: u8,
    height: u8,
    left: i16,
    top: i16,
}

impl CacheEntry {
    const EMPTY: Self = Self {
        key: 0,
        offset: 0,
        width: 0,
        height: 0,
        left: 0,
        top: 0,
    };
}

/// Working memory and the glyph cache. About 300 KiB: keep it in a static.
pub struct TextScratch {
    segments: [Segment; MAX_SEGMENTS],
    accum: [i32; ACCUM_CELLS],
    entries: [CacheEntry; CACHE_SLOTS],
    pool: [u8; POOL_BYTES],
    pool_used: usize,
    cached: usize,
}

impl TextScratch {
    pub const fn new() -> Self {
        Self {
            segments: [Segment::ZERO; MAX_SEGMENTS],
            accum: [0; ACCUM_CELLS],
            entries: [CacheEntry::EMPTY; CACHE_SLOTS],
            pool: [0; POOL_BYTES],
            pool_used: 0,
            cached: 0,
        }
    }

    /// How many glyphs are cached right now.
    pub const fn cached_glyphs(&self) -> usize {
        self.cached
    }

    /// Forget every cached glyph (they are rebuilt on demand).
    pub fn flush_cache(&mut self) {
        self.entries.fill(CacheEntry::EMPTY);
        self.pool_used = 0;
        self.cached = 0;
    }
}

impl Default for TextScratch {
    fn default() -> Self {
        Self::new()
    }
}

// ---- coverage --------------------------------------------------------------

/// Add the signed area of one edge to `accum`. Coordinates are Q16 pixels
/// relative to the bitmap's top-left corner; the bitmap is `width` x `height`.
///
/// This is the accumulation-buffer scan converter used by font-rs: an edge
/// deposits, in each row it crosses, the fraction of the row it covers split
/// across the pixels it passes through, and summing along the row later gives
/// the covered area of every pixel exactly.
fn accumulate_edge(accum: &mut [i32], width: usize, height: usize, from: (i64, i64), to: (i64, i64)) {
    if from.1 == to.1 {
        return;
    }

    let (direction, top, bottom) = if from.1 < to.1 { (1i64, from, to) } else { (-1i64, to, from) };
    let dx_dy = ((bottom.0 - top.0) << 16) / (bottom.1 - top.1);
    let mut x = top.0;

    let first_row = if top.1 < 0 {
        x -= (top.1 * dx_dy) >> 16;
        0
    } else {
        (top.1 >> 16) as usize
    };
    let last_row = (((bottom.1 + ONE - 1) >> 16) as usize).min(height);

    let mut add = |index: i64, value: i64| {
        if index >= 0 && (index as usize) < accum.len() {
            accum[index as usize] += value as i32;
        }
    };

    for row in first_row..last_row {
        let row_top = (row as i64) << 16;
        let dy = (row_top + ONE).min(bottom.1) - row_top.max(top.1);
        let next_x = x + ((dx_dy * dy) >> 16);
        let d = dy * direction;
        let (left, right) = if x < next_x { (x, next_x) } else { (next_x, x) };
        let left_pixel = (left >> 16).max(0);
        let right_pixel = ((right + ONE - 1) >> 16).max(0);
        let base = (row * width) as i64;

        if right_pixel <= left_pixel + 1 {
            // The edge stays within one pixel of this row.
            let mid_fraction = ((x + next_x) >> 1) - (left_pixel << 16);
            add(base + left_pixel, d - ((d * mid_fraction) >> 16));
            add(base + left_pixel + 1, (d * mid_fraction) >> 16);
        } else {
            // A shallow edge: it spreads over several pixels of the row.
            let s = (1i64 << 32) / (right - left).max(1);
            let left_fraction = left - (left_pixel << 16);
            let first = (s * (((ONE - left_fraction) * (ONE - left_fraction)) >> 16) >> 16) / 2;
            let right_fraction = right - (right_pixel << 16) + ONE;
            let last = (s * ((right_fraction * right_fraction) >> 16) >> 16) / 2;

            add(base + left_pixel, (d * first) >> 16);

            if right_pixel == left_pixel + 2 {
                add(base + left_pixel + 1, (d * (ONE - first - last)) >> 16);
            } else {
                let second = (s * (ONE + ONE / 2 - left_fraction)) >> 16;
                add(base + left_pixel + 1, (d * (second - first)) >> 16);

                for pixel in (left_pixel + 2)..(right_pixel - 1) {
                    add(base + pixel, (d * s) >> 16);
                }

                let through = second + (right_pixel - left_pixel - 3) * s;
                add(base + right_pixel - 1, (d * (ONE - through - last)) >> 16);
            }

            add(base + right_pixel, (d * last) >> 16);
        }

        x = next_x;
    }
}

/// Rasterize `segments` (26.6 fixed point, relative to the bitmap's top-left
/// pixel corner) into `out`, one byte of coverage per pixel, `width` x `height`.
fn rasterize_coverage(segments: &[Segment], width: usize, height: usize, accum: &mut [i32], out: &mut [u8]) {
    let cells = width * height;
    let clear = (cells + 4).min(accum.len());
    accum[..clear].fill(0);

    for segment in segments {
        accumulate_edge(
            accum,
            width,
            height,
            (i64::from(segment.x0) << 10, i64::from(segment.y0) << 10),
            (i64::from(segment.x1) << 10, i64::from(segment.y1) << 10),
        );
    }

    for row in 0..height {
        // A closed outline deposits exactly as much as it takes back in every
        // row, so each row's running sum starts from zero.
        let mut sum = 0i64;
        for column in 0..width {
            let index = row * width + column;
            sum += i64::from(accum[index]);
            let covered = sum.abs().min(ONE);
            out[index] = ((covered * 255 + ONE / 2) >> 16) as u8;
        }
    }
}

/// A slight boost to mid coverage: dark text on a light background reads
/// thinner than it is when blended linearly.
fn contrast(coverage: u8) -> u32 {
    let value = u32::from(coverage);
    (value + value * (255 - value) / 1300).min(255)
}

// ---- the renderer ----------------------------------------------------------

pub struct TtfTextRenderer<'font, 'scratch> {
    family: FontFamily<'font>,
    scratch: &'scratch mut TextScratch,
}

impl<'font, 'scratch> TtfTextRenderer<'font, 'scratch> {
    pub fn new(family: FontFamily<'font>, scratch: &'scratch mut TextScratch) -> Self {
        Self { family, scratch }
    }

    fn line_metrics(face: &Face<'_>, point_size: u32) -> Size {
        let units = face.units_per_em().max(1) as i64;
        let height_units = i64::from(face.ascender()) - i64::from(face.descender());
        let height = ((height_units.max(0) * i64::from(point_size) + units - 1) / units) as u32;
        Size::new(0, height.max(1))
    }

    fn advance_subpixels(face: &Face<'_>, glyph: GlyphId, point_size: u32) -> i32 {
        let advance = face.glyph_hor_advance(glyph).unwrap_or(0) as i64;
        let units = face.units_per_em().max(1) as i64;
        ((advance * i64::from(point_size) * i64::from(SUBPIXEL)) / units) as i32
    }

    /// The cached coverage bitmap for a glyph, building it first if needed.
    /// `phase` is the pen's quarter-pixel offset (0..=3).
    fn glyph_entry(&mut self, face: &Face<'_>, semibold: bool, glyph: GlyphId, point_size: u32, phase: i32) -> CacheEntry {
        let tag = self.family.cache_tag;
        let key = (1u64 << 63) | (u64::from(tag) << 56) | (u64::from(semibold) << 48) | (u64::from(glyph.0) << 32) | (u64::from(point_size & 0xFFFF) << 8) | phase as u64;
        let slot = (glyph.0 as usize * 31 + point_size as usize * 17 + phase as usize * 7 + usize::from(semibold) + usize::from(tag) * 101) % CACHE_SLOTS;

        for probe in 0..CACHE_SLOTS {
            let entry = self.scratch.entries[(slot + probe) % CACHE_SLOTS];
            if entry.key == key {
                return entry;
            }
            if entry.key == 0 {
                break;
            }
        }

        // Building may flush the pool (and with it the table) when the pool is
        // full, so the free slot is looked for only afterwards.
        let mut entry = self.build_entry(face, key, glyph, point_size, phase);

        let free = (0..CACHE_SLOTS)
            .map(|probe| (slot + probe) % CACHE_SLOTS)
            .find(|&index| self.scratch.entries[index].key == 0);

        let index = match free {
            Some(index) => index,
            None => {
                // The table itself is full: start over. The bitmap just built
                // goes with the pool, so it is built again into the fresh one.
                self.scratch.flush_cache();
                entry = self.build_entry(face, key, glyph, point_size, phase);
                slot
            }
        };

        self.scratch.entries[index] = entry;
        self.scratch.cached += 1;
        entry
    }

    /// Rasterize one glyph at the given quarter-pixel phase into the pool.
    fn build_entry(&mut self, face: &Face<'_>, key: u64, glyph: GlyphId, point_size: u32, phase: i32) -> CacheEntry {
        let empty = CacheEntry { key, ..CacheEntry::EMPTY };

        let units = face.units_per_em().max(1) as f32;
        let scale = point_size as f32 * SUBPIXEL as f32 / units;

        let (segment_count, min_x, min_y, max_x, max_y) = {
            let mut builder = GlyphBuilder::new(&mut self.scratch.segments, phase * (SUBPIXEL / 4), 0, scale);
            if face.outline_glyph(glyph, &mut builder).is_none() {
                return empty;
            }
            builder.finish()
        };

        if segment_count == 0 {
            return empty;
        }

        let left = floor_subpixel(min_x);
        let top = floor_subpixel(min_y);
        let right = ceil_subpixel(max_x);
        let bottom = ceil_subpixel(max_y);
        let width = (right - left + 2) as usize;
        let height = (bottom - top + 1) as usize;

        if width > MAX_BITMAP_WIDTH || height > MAX_BITMAP_HEIGHT {
            return empty;
        }

        if self.scratch.pool_used + width * height > POOL_BYTES {
            self.scratch.flush_cache();
        }

        for segment in &mut self.scratch.segments[..segment_count] {
            segment.x0 -= left * SUBPIXEL;
            segment.x1 -= left * SUBPIXEL;
            segment.y0 -= top * SUBPIXEL;
            segment.y1 -= top * SUBPIXEL;
        }

        let offset = self.scratch.pool_used;
        let scratch = &mut *self.scratch;
        rasterize_coverage(
            &scratch.segments[..segment_count],
            width,
            height,
            &mut scratch.accum,
            &mut scratch.pool[offset..offset + width * height],
        );
        scratch.pool_used += width * height;

        CacheEntry {
            key,
            offset: offset as u32,
            width: width as u8,
            height: height as u8,
            left: left as i16,
            top: top as i16,
        }
    }

    fn blit_glyph<C: Canvas + ?Sized>(&self, canvas: &mut C, entry: CacheEntry, pen_x: i32, baseline: i32, color: Color) {
        let (width, height) = (entry.width as usize, entry.height as usize);
        if width == 0 || height == 0 {
            return;
        }

        let bitmap = &self.scratch.pool[entry.offset as usize..entry.offset as usize + width * height];
        let origin_x = pen_x + i32::from(entry.left);
        let origin_y = baseline + i32::from(entry.top);

        for row in 0..height {
            let y = origin_y + row as i32;
            for column in 0..width {
                let coverage = bitmap[row * width + column];
                if coverage == 0 {
                    continue;
                }

                let alpha = (u32::from(color.alpha) * contrast(coverage) / 255) as u8;
                canvas.blend_pixel(Point::new(origin_x + column as i32, y), color.with_alpha(alpha));
            }
        }
    }
}

impl TextRenderer for TtfTextRenderer<'_, '_> {
    fn measure(&self, text: &str, point_size: u32, semibold: bool) -> Size {
        let face = self.family.face(semibold);
        let units = face.units_per_em().max(1) as u64;
        let mut advance_units = 0u64;

        for character in text.chars() {
            let Some(glyph) = face.glyph_index(character) else {
                continue;
            };
            advance_units = advance_units.saturating_add(
                u64::from(face.glyph_hor_advance(glyph).unwrap_or(0)),
            );
        }

        let width = ((advance_units * u64::from(point_size) + units - 1) / units) as u32;
        let mut metrics = Self::line_metrics(face, point_size);
        metrics.width = width;
        metrics
    }

    fn draw<C: Canvas + ?Sized>(
        &mut self,
        canvas: &mut C,
        origin: Point,
        text: &str,
        color: Color,
        point_size: u32,
        semibold: bool,
    ) {
        let face_ptr = self.family.face(semibold) as *const Face<'_>;
        let face = unsafe { &*face_ptr };
        let units = face.units_per_em().max(1) as i64;
        let baseline_offset = (i64::from(face.ascender()) * i64::from(point_size) * i64::from(SUBPIXEL))
            / units;
        // Glyphs sit on a whole-pixel baseline (crisp horizontal edges) but
        // keep the pen's fractional position along the line, in quarters.
        let baseline_subpixels = origin
            .y
            .saturating_mul(SUBPIXEL)
            .saturating_add(baseline_offset as i32);
        let baseline = baseline_subpixels.saturating_add(SUBPIXEL / 2) >> 6;
        let mut pen = origin.x.saturating_mul(SUBPIXEL);

        for character in text.chars() {
            let Some(glyph) = face.glyph_index(character) else {
                continue;
            };

            let quarters = ((pen & (SUBPIXEL - 1)) + SUBPIXEL / 8) / (SUBPIXEL / 4);
            let pen_pixel = (pen >> 6) + quarters / 4;
            let entry = self.glyph_entry(face, semibold, glyph, point_size, quarters % 4);
            self.blit_glyph(canvas, entry, pen_pixel, baseline, color);
            pen = pen.saturating_add(Self::advance_subpixels(face, glyph, point_size));
        }
    }
}

struct GlyphBuilder<'a> {
    segments: &'a mut [Segment],
    count: usize,
    pen_x: i32,
    baseline: i32,
    scale: f32,
    current: Point,
    start: Point,
    has_current: bool,
    min_x: i32,
    min_y: i32,
    max_x: i32,
    max_y: i32,
}

impl<'a> GlyphBuilder<'a> {
    fn new(segments: &'a mut [Segment], pen_x: i32, baseline: i32, scale: f32) -> Self {
        Self {
            segments,
            count: 0,
            pen_x,
            baseline,
            scale,
            current: Point::new(0, 0),
            start: Point::new(0, 0),
            has_current: false,
            min_x: i32::MAX,
            min_y: i32::MAX,
            max_x: i32::MIN,
            max_y: i32::MIN,
        }
    }

    fn point(&self, x: f32, y: f32) -> Point {
        Point::new(
            self.pen_x.saturating_add(round_i32(x * self.scale)),
            self.baseline.saturating_sub(round_i32(y * self.scale)),
        )
    }

    fn add_segment(&mut self, from: Point, to: Point) {
        if from == to || self.count >= self.segments.len() {
            return;
        }

        self.segments[self.count] = Segment {
            x0: from.x,
            y0: from.y,
            x1: to.x,
            y1: to.y,
        };
        self.count += 1;
        self.min_x = self.min_x.min(from.x).min(to.x);
        self.min_y = self.min_y.min(from.y).min(to.y);
        self.max_x = self.max_x.max(from.x).max(to.x);
        self.max_y = self.max_y.max(from.y).max(to.y);
    }

    fn finish(self) -> (usize, i32, i32, i32, i32) {
        if self.count == 0 {
            (0, 0, 0, 0, 0)
        } else {
            (self.count, self.min_x, self.min_y, self.max_x, self.max_y)
        }
    }
}

impl OutlineBuilder for GlyphBuilder<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let point = self.point(x, y);
        self.current = point;
        self.start = point;
        self.has_current = true;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        if !self.has_current {
            self.move_to(x, y);
            return;
        }

        let next = self.point(x, y);
        self.add_segment(self.current, next);
        self.current = next;
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        if !self.has_current {
            self.move_to(x, y);
            return;
        }

        let p0 = self.current;
        let p1 = self.point(x1, y1);
        let p2 = self.point(x, y);
        let denominator = QUAD_STEPS * QUAD_STEPS;
        let mut previous = p0;

        for step in 1..=QUAD_STEPS {
            let inverse = QUAD_STEPS - step;
            let px = (
                i64::from(inverse * inverse) * i64::from(p0.x)
                    + i64::from(2 * inverse * step) * i64::from(p1.x)
                    + i64::from(step * step) * i64::from(p2.x)
            ) / i64::from(denominator);
            let py = (
                i64::from(inverse * inverse) * i64::from(p0.y)
                    + i64::from(2 * inverse * step) * i64::from(p1.y)
                    + i64::from(step * step) * i64::from(p2.y)
            ) / i64::from(denominator);
            let next = Point::new(px as i32, py as i32);
            self.add_segment(previous, next);
            previous = next;
        }

        self.current = p2;
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        if !self.has_current {
            self.move_to(x, y);
            return;
        }

        let p0 = self.current;
        let p1 = self.point(x1, y1);
        let p2 = self.point(x2, y2);
        let p3 = self.point(x, y);
        let denominator = CUBIC_STEPS * CUBIC_STEPS * CUBIC_STEPS;
        let mut previous = p0;

        for step in 1..=CUBIC_STEPS {
            let inverse = CUBIC_STEPS - step;
            let px = (
                i64::from(inverse * inverse * inverse) * i64::from(p0.x)
                    + i64::from(3 * inverse * inverse * step) * i64::from(p1.x)
                    + i64::from(3 * inverse * step * step) * i64::from(p2.x)
                    + i64::from(step * step * step) * i64::from(p3.x)
            ) / i64::from(denominator);
            let py = (
                i64::from(inverse * inverse * inverse) * i64::from(p0.y)
                    + i64::from(3 * inverse * inverse * step) * i64::from(p1.y)
                    + i64::from(3 * inverse * step * step) * i64::from(p2.y)
                    + i64::from(step * step * step) * i64::from(p3.y)
            ) / i64::from(denominator);
            let next = Point::new(px as i32, py as i32);
            self.add_segment(previous, next);
            previous = next;
        }

        self.current = p3;
    }

    fn close(&mut self) {
        if self.has_current {
            self.add_segment(self.current, self.start);
            self.current = self.start;
        }
    }
}

fn round_i32(value: f32) -> i32 {
    if value >= 0.0 {
        (value + 0.5) as i32
    } else {
        (value - 0.5) as i32
    }
}

fn floor_subpixel(value: i32) -> i32 {
    if value >= 0 {
        value / SUBPIXEL
    } else {
        -((-value + SUBPIXEL - 1) / SUBPIXEL)
    }
}

fn ceil_subpixel(value: i32) -> i32 {
    if value >= 0 {
        (value + SUBPIXEL - 1) / SUBPIXEL
    } else {
        -((-value) / SUBPIXEL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::boxed::Box;
    use std::vec::Vec;
    use ui_render::Surface;

    /// A closed polygon as edges, in 26.6 fixed point.
    fn polygon(points: &[(i32, i32)]) -> Vec<Segment> {
        (0..points.len())
            .map(|index| {
                let (a, b) = (points[index], points[(index + 1) % points.len()]);
                Segment { x0: a.0, y0: a.1, x1: b.0, y1: b.1 }
            })
            .collect()
    }

    fn fp(pixels: f32) -> i32 {
        (pixels * SUBPIXEL as f32) as i32
    }

    fn coverage_of(points: &[(f32, f32)], width: usize, height: usize) -> Vec<u8> {
        let fixed: Vec<(i32, i32)> = points.iter().map(|&(x, y)| (fp(x), fp(y))).collect();
        let mut accum = std::vec![0i32; width * height + 8];
        let mut out = std::vec![0u8; width * height];
        rasterize_coverage(&polygon(&fixed), width, height, &mut accum, &mut out);
        out
    }

    fn at(bitmap: &[u8], width: usize, x: usize, y: usize) -> u8 {
        bitmap[y * width + x]
    }

    #[test]
    fn a_whole_pixel_rectangle_is_fully_covered_inside_and_empty_outside() {
        let bitmap = coverage_of(&[(2.0, 1.0), (6.0, 1.0), (6.0, 4.0), (2.0, 4.0)], 8, 6);
        for y in 1..4 {
            for x in 2..6 {
                assert_eq!(at(&bitmap, 8, x, y), 255, "({x},{y})");
            }
        }
        assert_eq!(at(&bitmap, 8, 1, 2), 0);
        assert_eq!(at(&bitmap, 8, 6, 2), 0);
        assert_eq!(at(&bitmap, 8, 3, 0), 0);
        assert_eq!(at(&bitmap, 8, 3, 4), 0);
    }

    #[test]
    fn a_half_pixel_edge_gets_half_coverage_in_both_directions() {
        // x from 2.5 to 5.5, y from 1.25 to 3.75.
        let bitmap = coverage_of(&[(2.5, 1.25), (5.5, 1.25), (5.5, 3.75), (2.5, 3.75)], 8, 6);
        let near = |value: u8, expected: i32| (i32::from(value) - expected).abs() <= 2;
        assert!(near(at(&bitmap, 8, 2, 2), 128), "left edge {}", at(&bitmap, 8, 2, 2));
        assert!(near(at(&bitmap, 8, 5, 2), 128), "right edge {}", at(&bitmap, 8, 5, 2));
        assert!(near(at(&bitmap, 8, 3, 1), 191), "top edge {}", at(&bitmap, 8, 3, 1));
        assert!(near(at(&bitmap, 8, 3, 3), 191), "bottom edge {}", at(&bitmap, 8, 3, 3));
        assert_eq!(at(&bitmap, 8, 3, 2), 255);
        // A corner pixel is the product of both edges (0.5 x 0.75).
        assert!(near(at(&bitmap, 8, 2, 1), 96), "corner {}", at(&bitmap, 8, 2, 1));
    }

    #[test]
    fn a_diagonal_edge_has_smooth_intermediate_coverage() {
        // A right triangle whose hypotenuse (slope 7/13) crosses the pixel grid
        // at a different offset in every column: a 45 degree edge would give
        // every boundary pixel exactly half and prove nothing.
        let bitmap = coverage_of(&[(1.0, 1.0), (14.0, 1.0), (1.0, 8.0)], 16, 16);
        let mut levels: Vec<u8> = bitmap.iter().copied().filter(|&value| value != 0 && value != 255).collect();
        levels.sort_unstable();
        levels.dedup();
        assert!(levels.len() >= 8, "only {} distinct partial levels", levels.len());

        // The covered area matches the geometric area (0.5 x 13 x 7 = 45.5 px^2).
        let total: u32 = bitmap.iter().map(|&value| u32::from(value)).sum();
        let area = total as f32 / 255.0;
        assert!((area - 45.5).abs() < 0.75, "area {area}");
    }

    #[test]
    fn a_shallow_edge_spanning_several_pixels_conserves_area() {
        // Nearly horizontal: a wedge from (1,2) to (15,3): area = 0.5 * 14 * 1.
        let bitmap = coverage_of(&[(1.0, 2.0), (15.0, 3.0), (1.0, 3.0)], 18, 6);
        let total: u32 = bitmap.iter().map(|&value| u32::from(value)).sum();
        let area = total as f32 / 255.0;
        assert!((area - 7.0).abs() < 0.25, "area {area}");
    }

    #[test]
    fn winding_direction_does_not_change_coverage() {
        let clockwise = coverage_of(&[(2.0, 1.0), (7.0, 1.0), (7.0, 5.0), (2.0, 5.0)], 9, 7);
        let counter = coverage_of(&[(2.0, 1.0), (2.0, 5.0), (7.0, 5.0), (7.0, 1.0)], 9, 7);
        assert_eq!(clockwise, counter);
    }

    const REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
    const SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf");

    fn render(scratch: &mut TextScratch, text: &str, semibold: bool) -> Vec<u32> {
        let family = FontFamily::from_bytes(REGULAR, Some(SEMIBOLD)).expect("Inter");
        let mut renderer = TtfTextRenderer::new(family, scratch);
        let mut pixels = std::vec![0x00FF_FFFFu32; 200 * 40];
        let mut surface = Surface::new(&mut pixels, 200, 40, 200).expect("surface");
        renderer.draw(&mut surface, Point::new(4, 6), text, Color::BLACK, 20, semibold);
        pixels
    }

    #[test]
    fn text_uses_many_grey_levels_not_five() {
        let mut scratch = Box::new(TextScratch::new());
        let pixels = render(&mut scratch, "Voyager Resources", false);
        let mut greys: Vec<u32> = pixels.iter().map(|&pixel| pixel & 0xFF).filter(|&grey| grey != 0xFF).collect();
        greys.sort_unstable();
        greys.dedup();
        assert!(greys.len() > 40, "only {} distinct grey levels", greys.len());
    }

    #[test]
    fn drawing_the_same_text_again_reuses_the_cache_and_looks_identical() {
        let mut scratch = Box::new(TextScratch::new());
        let first = render(&mut scratch, "Library", false);
        let cached = scratch.cached_glyphs();
        assert!(cached >= 5, "{cached}");

        let second = render(&mut scratch, "Library", false);
        assert_eq!(scratch.cached_glyphs(), cached, "a second draw built more glyphs");
        assert_eq!(first, second);
    }

    #[test]
    fn weights_and_sizes_are_cached_separately() {
        let mut scratch = Box::new(TextScratch::new());
        let regular = render(&mut scratch, "Disk", false);
        let semibold = render(&mut scratch, "Disk", true);
        assert_ne!(regular, semibold);
    }

    #[test]
    fn a_full_cache_flushes_and_keeps_drawing() {
        let mut scratch = Box::new(TextScratch::new());
        let expected = render(&mut scratch, "Sevos", false);
        // Fill the pool past its end with big glyphs at many sizes.
        let family = FontFamily::from_bytes(REGULAR, Some(SEMIBOLD)).expect("Inter");
        {
            let mut renderer = TtfTextRenderer::new(family, &mut scratch);
            let mut pixels = std::vec![0u32; 4];
            let mut surface = Surface::new(&mut pixels, 2, 2, 2).expect("surface");
            for size in 30..90 {
                renderer.draw(&mut surface, Point::new(0, 0), "WMQ@", Color::BLACK, size, false);
            }
        }
        assert_eq!(render(&mut scratch, "Sevos", false), expected);
    }

    const BOREL: &[u8] = include_bytes!("../../../assets/fonts/Borel-Regular.ttf");

    /// How many pixels one character of `family` covers at `size`.
    fn inked(family: FontFamily<'_>, scratch: &mut TextScratch, character: &str, size: u32) -> usize {
        let mut renderer = TtfTextRenderer::new(family, scratch);
        let mut pixels = std::vec![0x00FF_FFFFu32; 400 * 400];
        let mut surface = Surface::new(&mut pixels, 400, 400, 400).expect("surface");
        renderer.draw(&mut surface, Point::new(100, 20), character, Color::BLACK, size, false);
        pixels.iter().filter(|&&pixel| pixel != 0x00FF_FFFF).count()
    }

    #[test]
    fn borel_letters_all_rasterize_at_greeting_sizes() {
        // A glyph over MAX_GLYPH_DIM or MAX_SEGMENTS is skipped, not clipped:
        // the setup greeting would silently lose letters. 42 and 84 are its
        // 1x and 2x sizes; Borel's tallest letter is 1.5 em, so 84 px is
        // about the most that fits.
        let mut scratch = Box::new(TextScratch::new());
        for size in [42u32, 84] {
            for character in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ".chars() {
                let mut buffer = [0u8; 4];
                let text = character.encode_utf8(&mut buffer);
                let family = FontFamily::from_bytes(BOREL, None).expect("Borel");
                assert!(inked(family, &mut scratch, text, size) > 20, "{character:?} at {size} px drew nothing");
            }
        }
    }

    #[test]
    fn two_families_sharing_a_scratch_keep_their_own_glyphs() {
        let mut scratch = Box::new(TextScratch::new());
        let inter_alone = inked(FontFamily::from_bytes(REGULAR, None).expect("Inter"), &mut scratch, "e", 40);
        let borel_alone = {
            let mut fresh = Box::new(TextScratch::new());
            inked(FontFamily::from_bytes(BOREL, None).expect("Borel"), &mut fresh, "e", 40)
        };
        // Same glyph id space, same size: without the tag Borel would get Inter's "e".
        let borel_shared = inked(FontFamily::from_bytes(BOREL, None).expect("Borel").with_cache_tag(1), &mut scratch, "e", 40);
        assert_eq!(borel_shared, borel_alone);
        assert_ne!(borel_shared, inter_alone);
    }
}
