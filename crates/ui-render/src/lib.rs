#![no_std]

//! Software canvas and surface rendering primitives.

use ui_core::{Color, Point, Rect, Size};

/// How far along each edge a rounded corner of `radius` reaches in a
/// `width` x `height` rectangle.
///
/// The corners are squircles (a quartic superellipse, the usual cheap
/// stand-in for Apple's continuous corners), and a quartic cuts much less
/// off a corner than a circle of the same size: spread over `radius` alone
/// it read as barely rounded. Over one and a half times the radius it cuts
/// about as deep as a circle of `radius` would, with the smoother run-in.
pub fn corner_extent(radius: u32, width: u32, height: u32) -> u32 {
    (radius + radius / 2).min(width / 2).min(height / 2)
}

/// Pixels a corner reaching `extent` cuts from the side of the row `row`
/// pixels in from the top or bottom edge (0 is the outermost row).
///
/// Pixel centres against the curve: in doubled coordinates the corner's
/// centre is `2 * extent` in from both edges and pixel `i`'s centre is
/// `2 * i + 1`, so the curve meets each edge exactly `extent` pixels from
/// the corner. Integer only: these targets have no FPU.
pub fn corner_inset(row: u32, extent: u32) -> u32 {
    if row >= extent {
        return 0;
    }
    let reach = 2 * extent as i64;
    let reach4 = reach.pow(4);
    let dy4 = (reach - (2 * row as i64 + 1)).pow(4);
    let mut inset = 0;
    while inset < extent && (reach - (2 * inset as i64 + 1)).pow(4) + dy4 > reach4 {
        inset += 1;
    }
    inset
}

pub trait Canvas {
    fn size(&self) -> Size;
    fn fill(&mut self, color: Color);
    fn fill_rect(&mut self, rect: Rect, color: Color);
    fn fill_rounded_rect(&mut self, rect: Rect, radius: u32, color: Color);
    fn blend_pixel(&mut self, point: Point, color: Color);

    /*
     * Draw a small anti-aliased circle without requiring floating point.
     * Four quarter-pixel samples are enough for controls such as traffic lights
     * while keeping the freestanding soft-float build tiny.
     */
    fn fill_circle(&mut self, center: Point, radius: u32, color: Color) {
        if radius == 0 {
            self.blend_pixel(center, color);
            return;
        }

        let radius_scaled = radius as i64 * 4;
        let radius_squared = radius_scaled * radius_scaled;
        let radius_i = radius as i32;

        for y in (center.y - radius_i - 1)..=(center.y + radius_i + 1) {
            for x in (center.x - radius_i - 1)..=(center.x + radius_i + 1) {
                let mut covered = 0u32;

                for sample_y in [1i64, 3i64] {
                    for sample_x in [1i64, 3i64] {
                        let dx = (x as i64 - center.x as i64) * 4 + sample_x - 2;
                        let dy = (y as i64 - center.y as i64) * 4 + sample_y - 2;
                        if dx * dx + dy * dy <= radius_squared {
                            covered += 1;
                        }
                    }
                }

                if covered == 0 {
                    continue;
                }

                let alpha = (color.alpha as u32 * covered / 4) as u8;
                self.blend_pixel(Point::new(x, y), color.with_alpha(alpha));
            }
        }
    }

    /*
     * Fill a rect whose top-left and top-right corners are rounded with the
     * same squircle curve as `fill_rounded_rect`, but whose bottom edge is a
     * plain straight line. A titlebar band sits flush against a window's
     * rounded top corners but its own bottom edge is an internal division,
     * not a real window corner, so it must not be rounded there too.
     */
    fn fill_top_rounded_rect(&mut self, rect: Rect, radius: u32, color: Color) {
        if rect.size.width == 0 || rect.size.height == 0 {
            return;
        }

        // The top corners only: the band's height is not halved, its bottom
        // edge is straight.
        let extent = corner_extent(radius, rect.size.width, rect.size.height * 2);
        for row in 0..rect.size.height {
            // Deliberately not anti-aliased -- see `fill_rounded_rect` for
            // why a blended edge pixel would corrupt this surface's
            // color-key transport to WindowServer.
            let inset = corner_inset(row, extent);
            if rect.size.width > inset * 2 {
                self.fill_rect(Rect::new(rect.origin.x + inset as i32, rect.origin.y + row as i32, rect.size.width - inset * 2, 1), color);
            }
        }
    }
}

pub struct Surface<'a> {
    pixels: &'a mut [u32],
    width: u32,
    height: u32,
    stride: u32,
}

impl<'a> Surface<'a> {
    pub fn new(pixels: &'a mut [u32], width: u32, height: u32, stride: u32) -> Option<Self> {
        if width == 0 || height == 0 || stride < width {
            return None;
        }

        let required = (stride as usize).checked_mul(height as usize)?;
        if pixels.len() < required {
            return None;
        }

        Some(Self {
            pixels,
            width,
            height,
            stride,
        })
    }

    pub fn pixels(&self) -> &[u32] {
        self.pixels
    }

    pub fn pixels_mut(&mut self) -> &mut [u32] {
        self.pixels
    }

    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }

    pub const fn stride(&self) -> u32 {
        self.stride
    }

    fn pixel_index(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return None;
        }

        Some(y as usize * self.stride as usize + x as usize)
    }

    pub fn blit_xrgb(&mut self, origin: Point, source: &[u32], width: u32, height: u32, stride: u32) {
        if width == 0 || height == 0 || stride < width {
            return;
        }

        let required = stride as usize * height as usize;
        if source.len() < required {
            return;
        }

        for source_y in 0..height as i32 {
            let target_y = origin.y + source_y;
            if target_y < 0 || target_y >= self.height as i32 {
                continue;
            }

            let source_left = if origin.x < 0 { (-origin.x) as u32 } else { 0 };
            let target_left = origin.x.max(0) as u32;
            if source_left >= width || target_left >= self.width {
                continue;
            }

            let copy_width = (width - source_left).min(self.width - target_left);
            let source_start = source_y as usize * stride as usize + source_left as usize;
            let target_start = target_y as usize * self.stride as usize + target_left as usize;
            self.pixels[target_start..target_start + copy_width as usize]
                .copy_from_slice(&source[source_start..source_start + copy_width as usize]);
        }
    }

    /*
     * Copy an XRGB surface while treating one color as transparent. This keeps
     * cached rounded window corners independent from the desktop underneath.
     */
    pub fn blit_xrgb_keyed(
        &mut self,
        origin: Point,
        source: &[u32],
        width: u32,
        height: u32,
        stride: u32,
        transparent: u32,
    ) {
        if width == 0 || height == 0 || stride < width {
            return;
        }

        let required = stride as usize * height as usize;
        if source.len() < required {
            return;
        }

        for source_y in 0..height as i32 {
            let target_y = origin.y + source_y;
            if target_y < 0 || target_y >= self.height as i32 {
                continue;
            }

            for source_x in 0..width as i32 {
                let target_x = origin.x + source_x;
                if target_x < 0 || target_x >= self.width as i32 {
                    continue;
                }

                let source_index = source_y as usize * stride as usize + source_x as usize;
                let pixel = source[source_index];
                if pixel == transparent {
                    continue;
                }

                let target_index = target_y as usize * self.stride as usize + target_x as usize;
                self.pixels[target_index] = pixel;
            }
        }
    }
}

impl Canvas for Surface<'_> {
    fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }

    fn fill(&mut self, color: Color) {
        let value = color.to_xrgb8888();

        for y in 0..self.height as usize {
            let start = y * self.stride as usize;
            self.pixels[start..start + self.width as usize].fill(value);
        }
    }

    fn fill_rect(&mut self, rect: Rect, color: Color) {
        let left = rect.origin.x.max(0);
        let top = rect.origin.y.max(0);
        let right = rect.origin.x.saturating_add(rect.size.width as i32).min(self.width as i32);
        let bottom = rect.origin.y.saturating_add(rect.size.height as i32).min(self.height as i32);

        if left >= right || top >= bottom {
            return;
        }

        let value = color.to_xrgb8888();
        for y in top..bottom {
            let start = y as usize * self.stride as usize + left as usize;
            let end = y as usize * self.stride as usize + right as usize;
            self.pixels[start..end].fill(value);
        }
    }

    fn fill_rounded_rect(&mut self, rect: Rect, radius: u32, color: Color) {
        if rect.size.width == 0 || rect.size.height == 0 {
            return;
        }

        // Squircle corners (see `corner_extent`). Deliberately NOT
        // anti-aliased: this window surface is transported to WindowServer
        // via an exact-match color key, not a real alpha channel (see
        // `WS_Render_Window`'s `transparent_key`), so a partially-blended
        // edge pixel would round-trip as some other RGB value that fails the
        // key match and comes back fully OPAQUE -- a visible off-color fringe
        // right at the curve, worse than a one-pixel-hard edge.
        //
        // Each row is one solid span, cut in by the same number of pixels on
        // both sides, so the curve is worked out once per row and the span is
        // one fill.
        let extent = corner_extent(radius, rect.size.width, rect.size.height);
        let height = rect.size.height;
        for row in 0..height {
            let inset = corner_inset(row.min(height - 1 - row), extent);
            if rect.size.width > inset * 2 {
                self.fill_rect(Rect::new(rect.origin.x + inset as i32, rect.origin.y + row as i32, rect.size.width - inset * 2, 1), color);
            }
        }
    }

    fn blend_pixel(&mut self, point: Point, color: Color) {
        let Some(index) = self.pixel_index(point.x, point.y) else {
            return;
        };

        if color.alpha == 0 {
            return;
        }

        if color.alpha == 255 {
            self.pixels[index] = color.to_xrgb8888();
            return;
        }

        let destination = Color::from_xrgb8888(self.pixels[index]);
        let alpha = color.alpha as u32;
        let inverse = 255 - alpha;
        let red = (color.red as u32 * alpha + destination.red as u32 * inverse + 127) / 255;
        let green = (color.green as u32 * alpha + destination.green as u32 * inverse + 127) / 255;
        let blue = (color.blue as u32 * alpha + destination.blue as u32 * inverse + 127) / 255;
        self.pixels[index] = Color::rgb(red as u8, green as u8, blue as u8).to_xrgb8888();
    }
}

pub struct CanvasView<'a, C: Canvas + ?Sized> {
    canvas: &'a mut C,
    frame: Rect,
}

impl<'a, C: Canvas + ?Sized> CanvasView<'a, C> {
    pub fn new(canvas: &'a mut C, frame: Rect) -> Self {
        Self { canvas, frame }
    }

    fn translate_rect(&self, rect: Rect) -> Rect {
        rect.translated(self.frame.origin.x, self.frame.origin.y)
    }

    fn local_contains(&self, point: Point) -> bool {
        point.x >= 0
            && point.y >= 0
            && point.x < self.frame.size.width as i32
            && point.y < self.frame.size.height as i32
    }
}

impl<C: Canvas + ?Sized> Canvas for CanvasView<'_, C> {
    fn size(&self) -> Size {
        self.frame.size
    }

    fn fill(&mut self, color: Color) {
        self.canvas.fill_rect(self.frame, color);
    }

    fn fill_rect(&mut self, rect: Rect, color: Color) {
        self.canvas.fill_rect(self.translate_rect(rect), color);
    }

    fn fill_rounded_rect(&mut self, rect: Rect, radius: u32, color: Color) {
        self.canvas.fill_rounded_rect(self.translate_rect(rect), radius, color);
    }

    fn blend_pixel(&mut self, point: Point, color: Color) {
        if !self.local_contains(point) {
            return;
        }

        self.canvas.blend_pixel(
            Point::new(point.x + self.frame.origin.x, point.y + self.frame.origin.y),
            color,
        );
    }
}

pub trait TextRenderer {
    fn measure(&self, text: &str, point_size: u32, semibold: bool) -> Size;

    fn draw<C: Canvas + ?Sized>(
        &mut self,
        canvas: &mut C,
        origin: Point,
        text: &str,
        color: Color,
        point_size: u32,
        semibold: bool,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The curve, tested pixel by pixel: pixel centres against a quartic
    /// whose centre sits `extent` in from both edges.
    fn inside(x: i32, y: i32, width: i32, height: i32, extent: i32) -> bool {
        let fold = |v: i32, size: i32| (v.min(size - 1 - v) * 2 + 1) as i64;
        let reach = 2 * extent as i64;
        let (dx, dy) = ((reach - fold(x, width)).max(0), (reach - fold(y, height)).max(0));
        dx.pow(4) + dy.pow(4) <= reach.pow(4)
    }

    #[test]
    fn rounded_rect_spans_match_the_per_pixel_curve() {
        for (width, height, radius) in [(40u32, 30u32, 10u32), (21, 21, 10), (64, 12, 6), (9, 40, 4), (33, 33, 1)] {
            let (full_width, full_height) = (width + 6, height + 6);
            let mut pixels = [0u32; 80 * 80];
            let mut surface = Surface::new(&mut pixels, full_width, full_height, full_width).unwrap();
            surface.fill_rounded_rect(Rect::new(3, 3, width, height), radius, Color::rgb(255, 255, 255));

            let extent = corner_extent(radius, width, height) as i32;
            for y in 0..full_height as i32 {
                for x in 0..full_width as i32 {
                    let (local_x, local_y) = (x - 3, y - 3);
                    let expected = local_x >= 0 && local_y >= 0 && local_x < width as i32 && local_y < height as i32
                        && inside(local_x, local_y, width as i32, height as i32, extent);
                    let painted = pixels[(y as u32 * full_width + x as u32) as usize] != 0;
                    assert_eq!(painted, expected, "{width}x{height} r{radius} at ({local_x}, {local_y})");
                }
            }
        }
    }

    #[test]
    fn a_corner_cuts_about_as_deep_as_a_circle_of_its_radius() {
        // A circle of radius 12 leaves 12 * (1 - 1/sqrt 2) ~ 3.5 px between
        // the corner and the curve along the diagonal.
        let extent = corner_extent(12, 200, 200);
        let diagonal = (0..extent).find(|&i| corner_inset(i, extent) <= i).unwrap();
        assert!((3..=5).contains(&diagonal), "diagonal cut {diagonal}");
        // And it reaches the edges `extent` from the corner, no further.
        assert_eq!(corner_inset(extent, extent), 0);
        assert!(corner_inset(0, extent) > 0, "the outermost row is cut");
    }

    #[test]
    fn top_rounded_rects_round_only_the_top() {
        let mut pixels = [0u32; 40 * 20];
        let mut surface = Surface::new(&mut pixels, 40, 20, 40).unwrap();
        surface.fill_top_rounded_rect(Rect::new(0, 0, 40, 20), 8, Color::rgb(255, 255, 255));
        assert_eq!(pixels[0], 0, "top-left cut");
        assert_eq!(pixels[39], 0, "top-right cut");
        assert_ne!(pixels[19 * 40], 0, "bottom-left square");
        assert_ne!(pixels[19 * 40 + 39], 0, "bottom-right square");
    }
}
