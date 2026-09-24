#![no_std]

//! Software canvas and surface rendering primitives.

use ui_core::{Color, Point, Rect, Size};

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

        let radius = radius.min(rect.size.width / 2).min(rect.size.height);
        if radius == 0 {
            self.fill_rect(rect, color);
            return;
        }

        let radius_i = radius as i32;
        let width_i = rect.size.width as i32;

        // Everything below the rounded band, and the flat middle of the
        // band itself, fill solid -- only the two top corners need the
        // per-pixel curve test below.
        self.fill_rect(
            Rect::new(rect.origin.x, rect.origin.y + radius_i, rect.size.width, rect.size.height - radius),
            color,
        );
        self.fill_rect(
            Rect::new(rect.origin.x + radius_i, rect.origin.y, rect.size.width - radius * 2, radius),
            color,
        );

        // Deliberately not anti-aliased -- see `fill_rounded_rect` for why a
        // blended edge pixel would corrupt this surface's color-key
        // transport to WindowServer.
        let radius_pow4 = (radius_i as i64).pow(4);
        let nearest_y = radius_i - 1;

        for local_y in 0..radius_i {
            let dy = (local_y - nearest_y) as i64;

            for local_x in (0..radius_i).chain((width_i - radius_i)..width_i) {
                let nearest_x = if local_x < radius_i { radius_i - 1 } else { width_i - radius_i };
                let dx = (local_x - nearest_x) as i64;
                if dx.pow(4) + dy.pow(4) > radius_pow4 {
                    continue;
                }

                let px = rect.origin.x + local_x;
                let py = rect.origin.y + local_y;
                self.blend_pixel(Point::new(px, py), color.with_alpha(255));
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

    fn set_pixel(&mut self, x: i32, y: i32, color: Color) {
        let Some(index) = self.pixel_index(x, y) else {
            return;
        };

        self.pixels[index] = color.to_xrgb8888();
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

        let radius = radius.min(rect.size.width / 2).min(rect.size.height / 2);
        if radius == 0 {
            self.fill_rect(rect, color);
            return;
        }

        let radius_i = radius as i32;

        // Squircle corners (Apple's "continuous corner" look), not a plain
        // circular arc: a quartic superellipse (dx^4 + dy^4 <= r^4) is the
        // standard cheap approximation of that continuous curvature. It's
        // pure integer power -- no pow(f64)/sqrt/soft-float, since this
        // target has no hardware FPU (aarch64-unknown-none-softfloat).
        //
        // Deliberately NOT anti-aliased: this window surface is transported
        // to WindowServer via an exact-match color key, not a real alpha
        // channel (see `WS_Render_Window`'s `transparent_key`), so a
        // partially-blended edge pixel would round-trip as some other RGB
        // value that fails the key match and comes back fully OPAQUE -- a
        // visible off-color fringe right at the curve, worse than a
        // one-pixel-hard edge.
        let radius_pow4 = (radius_i as i64).pow(4);

        for local_y in 0..rect.size.height as i32 {
            for local_x in 0..rect.size.width as i32 {
                let nearest_x = if local_x < radius_i {
                    radius_i - 1
                } else if local_x >= rect.size.width as i32 - radius_i {
                    rect.size.width as i32 - radius_i
                } else {
                    local_x
                };

                let nearest_y = if local_y < radius_i {
                    radius_i - 1
                } else if local_y >= rect.size.height as i32 - radius_i {
                    rect.size.height as i32 - radius_i
                } else {
                    local_y
                };

                let dx = (local_x - nearest_x) as i64;
                let dy = (local_y - nearest_y) as i64;
                if dx.pow(4) + dy.pow(4) > radius_pow4 {
                    continue;
                }

                self.set_pixel(rect.origin.x + local_x, rect.origin.y + local_y, color);
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
