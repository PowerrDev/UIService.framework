//! The Dock's window, as the desktop keeps it.
//!
//! The Dock is its own process (`/System/Library/CoreServices/Dock.app`,
//! started by bootd): it reads its configuration, loads the app icons, and
//! draws the icons and the running-app dots into a transparent frame it
//! sends through the UI session bridge. The desktop places that frame along
//! the bottom of the screen, keeps it in front of every app window, and puts
//! the panel's material under it -- the wallpaper behind the Dock, blurred
//! and lightened, like macOS's vibrancy -- since only the desktop has the
//! wallpaper. It also draws the name bubble over the icon under the pointer
//! (the Dock says which name, and where), as a window of its own so the
//! Dock's window stays exactly the panel and its shadow fits it.

use ui_core::{scale::pt, system_color, Color, Event, Point, PointerButton, Rect, Size};
use ui_render::{Canvas, Surface, TextRenderer};
use ui_session as session;

use crate::bridge::{self, Pixels};
use crate::windowserver::{self, CursorKind, WindowId};
use crate::{scene, text};

/// The gap between the Dock and the bottom of the screen.
fn bottom_margin() -> u32 {
    pt(4)
}

/// How far above the Dock the name bubble floats.
fn label_gap() -> u32 {
    pt(8)
}

/// The material: the blurred wallpaper under a white veil, and a hairline
/// edge that is lighter on top, as the light catches it.
const VEIL: Color = Color::rgba(255, 255, 255, 92);
const EDGE: Color = Color::rgba(255, 255, 255, 120);
const EDGE_SHADE: Color = Color::rgba(0, 0, 0, 38);

const LABEL_FILL: Color = Color::rgba(240, 240, 242, 235);
const LABEL_EDGE: Color = Color::rgba(0, 0, 0, 40);

pub(crate) struct DockHost {
    connection: u32,
    window: Option<WindowId>,
    frame: Rect,
    radius: u32,
    /// The Dock's own pixels (ARGB), as sent.
    frame_pixels: Pixels,
    /// The blurred wallpaper under `frame`, made once per placement.
    backdrop: Pixels,
    /// What is shown: backdrop, veil and the Dock's pixels composited.
    composed: Pixels,
    sequence: u64,
    state: session::Submit,
    label: Label,
    /// The pointer is over the Dock (so it is sent a leave when it goes).
    hovered: bool,
    /// A press went down on the Dock: the release is the Dock's too.
    pressed: bool,
}

struct Label {
    window: Option<WindowId>,
    text: [u8; session::LABEL_MAX],
    frame: Rect,
    pixels: Option<Pixels>,
}

impl DockHost {
    /// Take a Dock connection (`None` for any other kind).
    pub(crate) fn new(connection: u32, screen: Size) -> Option<Self> {
        let info = bridge::info(connection)?;
        if info.kind != session::KIND_DOCK {
            return None;
        }
        let capacity = screen.width as usize * (screen.height / 3) as usize;
        Some(Self {
            connection,
            window: None,
            frame: Rect::new(0, 0, 0, 0),
            radius: 0,
            frame_pixels: Pixels::new(capacity)?,
            backdrop: Pixels::new(capacity)?,
            composed: Pixels::new(capacity)?,
            sequence: 0,
            state: session::Submit::empty(),
            label: Label { window: None, text: [0; session::LABEL_MAX], frame: Rect::new(0, 0, 0, 0), pixels: None },
            hovered: false,
            pressed: false,
        })
    }

    pub(crate) fn alive(&self) -> bool {
        bridge::alive(self.connection)
    }

    /// Ask the Dock to open the bundle at `path` (it launches every app).
    pub(crate) fn launch(&self, path: &str) {
        let mut message = session::Message::new(session::MSG_LAUNCH);
        session::set_str(&mut message.path, path);
        bridge::post(self.connection, &message);
    }

    /// Put the Dock (and its bubble) back in front after a window was raised:
    /// WindowServer has no always-on-top level.
    pub(crate) fn raise(&self) {
        if let Some(id) = self.window {
            windowserver::Focus_Window(id);
        }
        if let Some(id) = self.label.window {
            windowserver::Focus_Window(id);
        }
    }

    /// Whether `point` (screen coordinates) is on the Dock's panel.
    pub(crate) fn contains(&self, point: Point) -> bool {
        self.window.is_some() && self.frame.contains(point)
    }

    /// One desktop pass: pick up a new frame, and the name bubble.
    /// `desktop` is the desktop layer (the wallpaper the material blurs).
    pub(crate) fn update(&mut self, screen: Size, desktop: &[u32], desktop_stride: u32) {
        let new_state = bridge::state(self.connection, &mut self.sequence);
        if let Some(state) = new_state {
            self.state = state;
        }

        let capacity = self.frame_pixels.len();
        let max_width = screen.width;
        let max_height = (capacity / screen.width.max(1) as usize) as u32;
        if let Some((width, height)) = bridge::take_frame(
            self.connection,
            self.frame_pixels.as_mut_slice(),
            max_width,
            max_width,
            max_height,
        ) {
            let width = width.min(max_width);
            let height = height.min(max_height);
            // The frame arrived at the dock-wide stride (screen width): pack it.
            let pixels = self.frame_pixels.as_mut_slice();
            for row in 1..height as usize {
                pixels.copy_within(row * max_width as usize..row * max_width as usize + width as usize, row * width as usize);
            }
            self.place(screen, Size::new(width, height), desktop, desktop_stride);
            self.compose();
        }

        if new_state.is_some() {
            self.update_label(screen);
        }
    }

    /// Centre a `size` Dock along the bottom; a new size or place gets a new
    /// window and a new backdrop.
    fn place(&mut self, screen: Size, size: Size, desktop: &[u32], desktop_stride: u32) {
        let x = (screen.width as i32 - size.width as i32) / 2;
        let y = screen.height as i32 - size.height as i32 - bottom_margin() as i32;
        let frame = Rect::new(x, y, size.width, size.height);
        let radius = self.state.panel_radius.min(size.height / 2);

        if frame == self.frame && radius == self.radius && self.window.is_some() {
            return;
        }
        if let Some(id) = self.window.take() {
            windowserver::Destroy_Window(id);
        }
        self.frame = frame;
        self.radius = radius;
        blur_backdrop(self.backdrop.as_mut_slice(), frame, desktop, desktop_stride, screen);
        self.window = windowserver::Create_Window(frame, false, radius);
        if let Some(id) = self.window {
            windowserver::Set_Window_Shadow(id, windowserver::Shadow::Popup);
        }
        self.raise();
    }

    /// The panel's material, then the Dock's own pixels over it. Outside the
    /// rounded panel stays see-through (alpha 0), its edge antialiased.
    fn compose(&mut self) {
        let Some(id) = self.window else { return; };
        let size = self.frame.size;
        let count = size.width as usize * size.height as usize;
        if count == 0 || count > self.composed.len() {
            return;
        }

        let radius = self.radius as i32;
        let (width, height) = (size.width as i32, size.height as i32);
        let backdrop = &self.backdrop.as_mut_slice()[..count] as *const [u32];
        let source = &self.frame_pixels.as_mut_slice()[..count] as *const [u32];
        let output = &mut self.composed.as_mut_slice()[..count];
        // SAFETY: three distinct heap buffers.
        let (backdrop, source) = unsafe { (&*backdrop, &*source) };

        for y in 0..height {
            for x in 0..width {
                let index = (y * width + x) as usize;
                let coverage = rounded_coverage(x, y, width, height, radius);
                if coverage == 0 {
                    output[index] = scene::TRANSPARENT_KEY;
                    continue;
                }

                let mut pixel = blend(backdrop[index] | 0xFF00_0000, VEIL);
                // The hairline: light along the top, a faint shade elsewhere.
                let edge = rounded_coverage(x, y, width, height, radius) == 255
                    && rounded_coverage_inset(x, y, width, height, radius, pt(1).max(1) as i32) < 255;
                if edge {
                    pixel = blend(pixel, if y < height / 2 { EDGE } else { EDGE_SHADE });
                }
                pixel = blend_argb(pixel, source[index]);
                output[index] = (pixel & 0x00FF_FFFF) | ((coverage as u32) << 24);
            }
        }

        windowserver::Render_Window(id, &self.composed.as_mut_slice()[..count], size.width, size.height, size.width);
    }

    /// Show, move or take down the name bubble to match what the Dock asked.
    fn update_label(&mut self, screen: Size) {
        let wanted = self.state.label;
        let name = session::get_str(&wanted);
        if name.is_empty() || self.window.is_none() {
            self.hide_label();
            return;
        }

        let text = text::shared();
        let point_size = pt(13);
        let measured = text.measure(name, point_size, false);
        let width = measured.width + pt(24);
        let height = measured.height + pt(12);
        let center = self.frame.origin.x + self.state.label_x;
        let x = (center - width as i32 / 2).clamp(0, (screen.width as i32 - width as i32).max(0));
        let y = self.frame.origin.y - label_gap() as i32 - height as i32;
        let frame = Rect::new(x, y, width, height);

        if self.label.window.is_some() && self.label.frame == frame && self.label.text == wanted {
            return;
        }
        self.hide_label();

        let count = width as usize * height as usize;
        if self.label.pixels.as_ref().is_none_or(|pixels| pixels.len() < count) {
            self.label.pixels = Pixels::new(count);
        }
        let Some(pixels) = self.label.pixels.as_mut() else { return; };
        let buffer = &mut pixels.as_mut_slice()[..count];
        {
            let Some(mut surface) = Surface::new(buffer, width, height, width) else { return; };
            surface.fill(Color::from_xrgb8888(scene::TRANSPARENT_KEY));
            let radius = pt(7);
            // Filled with alpha written straight in: the bubble is its own
            // translucent window over whatever is behind it.
            fill_rounded_argb(&mut surface, Rect::new(0, 0, width, height), radius, LABEL_EDGE);
            fill_rounded_argb(&mut surface, Rect::new(1, 1, width - 2, height - 2), radius.saturating_sub(1), LABEL_FILL);
            let origin = Point::new((width as i32 - measured.width as i32) / 2, (height as i32 - measured.height as i32) / 2);
            text.draw(&mut surface, origin, name, system_color::LABEL, point_size, false);
        }

        self.label.window = windowserver::Create_Window(frame, false, pt(7));
        if let Some(id) = self.label.window {
            windowserver::Set_Window_Shadow(id, windowserver::Shadow::Popup);
            windowserver::Render_Window(id, &pixels.as_mut_slice()[..count], width, height, width);
        }
        self.label.frame = frame;
        self.label.text = wanted;
    }

    fn hide_label(&mut self) {
        if let Some(id) = self.label.window.take() {
            windowserver::Destroy_Window(id);
        }
    }

    /// The Dock's share of `event`: `true` when it took it. `captured` is a
    /// drag that started in a window, which stays that window's.
    pub(crate) fn event(&mut self, event: Event, captured: bool) -> bool {
        let local = |position: Point| Point::new(position.x - self.frame.origin.x, position.y - self.frame.origin.y);
        let mut message = session::Message::new(session::MSG_EVENT);
        message.width = self.frame.size.width;
        message.height = self.frame.size.height;

        let taken = match event {
            Event::PointerMoved { position } if !captured || self.pressed => {
                let over = self.contains(position);
                if over || self.pressed {
                    let at = local(position);
                    message.event = session::EVENT_POINTER_MOVED;
                    message.x = at.x;
                    message.y = at.y;
                    self.hovered = true;
                    windowserver::Set_Cursor_Kind(CursorKind::Arrow);
                    true
                } else if self.hovered {
                    self.hovered = false;
                    message.event = session::EVENT_POINTER_LEFT;
                    bridge::post(self.connection, &message);
                    return false;
                } else {
                    return false;
                }
            }
            Event::PointerDown { position, button } if self.contains(position) => {
                self.pressed = true;
                let at = local(position);
                message.event = session::EVENT_POINTER_DOWN;
                message.x = at.x;
                message.y = at.y;
                message.button = button_code(button);
                true
            }
            Event::PointerUp { position, button } if self.pressed => {
                self.pressed = false;
                let at = local(position);
                message.event = session::EVENT_POINTER_UP;
                message.x = at.x;
                message.y = at.y;
                message.button = button_code(button);
                true
            }
            Event::Scroll { position, .. } => return self.contains(position),
            _ => return false,
        };

        if taken {
            bridge::post(self.connection, &message);
        }
        taken
    }
}

impl Drop for DockHost {
    fn drop(&mut self) {
        self.hide_label();
        if let Some(id) = self.window.take() {
            windowserver::Destroy_Window(id);
        }
        bridge::release(self.connection);
    }
}

fn button_code(button: PointerButton) -> u32 {
    match button {
        PointerButton::Primary => session::BUTTON_PRIMARY,
        PointerButton::Secondary => session::BUTTON_SECONDARY,
        PointerButton::Middle => session::BUTTON_MIDDLE,
    }
}

/// `color` (with its alpha) over the opaque `under` (XRGB).
fn blend(under: u32, color: Color) -> u32 {
    let alpha = color.alpha as u32;
    let mix = |under: u32, over: u8| (under * (255 - alpha) + over as u32 * alpha + 127) / 255;
    let red = mix((under >> 16) & 0xFF, color.red);
    let green = mix((under >> 8) & 0xFF, color.green);
    let blue = mix(under & 0xFF, color.blue);
    0xFF00_0000 | (red << 16) | (green << 8) | blue
}

/// An ARGB8888 pixel over the opaque `under`.
fn blend_argb(under: u32, over: u32) -> u32 {
    let alpha = (over >> 24) as u8;
    match alpha {
        0 => under,
        255 => over | 0xFF00_0000,
        _ => blend(under, Color::rgba((over >> 16) as u8, (over >> 8) as u8, over as u8, alpha)),
    }
}

/// How much of pixel (x, y) lies inside a `width` x `height` rectangle with
/// corners of `radius`, 0..=255: a 4x4 supersample at the corners.
fn rounded_coverage(x: i32, y: i32, width: i32, height: i32, radius: i32) -> u8 {
    rounded_coverage_inset(x, y, width, height, radius, 0)
}

fn rounded_coverage_inset(x: i32, y: i32, width: i32, height: i32, radius: i32, inset: i32) -> u8 {
    let (left, top, right, bottom) = (inset, inset, width - inset, height - inset);
    if x < left || y < top || x >= right || y >= bottom {
        return 0;
    }
    let radius = (radius - inset).max(0);
    let corner_x = if x < left + radius { left + radius } else if x >= right - radius { right - radius } else { return 255 };
    let corner_y = if y < top + radius { top + radius } else if y >= bottom - radius { bottom - radius } else { return 255 };

    // Sub-pixel samples at quarters, in units of 1/8 pixel.
    let r8 = radius * 8;
    let mut inside = 0;
    for sy in 0..4 {
        for sx in 0..4 {
            let px = x * 8 + sx * 2 + 1 - corner_x * 8;
            let py = y * 8 + sy * 2 + 1 - corner_y * 8;
            if px * px + py * py <= r8 * r8 {
                inside += 1;
            }
        }
    }
    (inside * 255 / 16) as u8
}

/// A rounded rectangle written (not blended) with `color`'s alpha, for a
/// window that is itself translucent.
fn fill_rounded_argb(surface: &mut Surface<'_>, rect: Rect, radius: u32, color: Color) {
    let (width, height) = (rect.size.width as i32, rect.size.height as i32);
    let stride = surface.stride() as usize;
    let pixels = surface.pixels_mut();
    for y in 0..height {
        for x in 0..width {
            let coverage = rounded_coverage(x, y, width, height, radius as i32) as u32;
            if coverage == 0 {
                continue;
            }
            let alpha = color.alpha as u32 * coverage / 255;
            let index = (rect.origin.y + y) as usize * stride + (rect.origin.x + x) as usize;
            pixels[index] = (alpha << 24) | ((color.red as u32) << 16) | ((color.green as u32) << 8) | color.blue as u32;
        }
    }
}

/// The wallpaper under `frame`, heavily blurred, into `output` (packed
/// `frame.size`). Three box blurs approximate a gaussian; each runs over a
/// quarter-resolution copy so the radius can be wide without the cost.
fn blur_backdrop(output: &mut [u32], frame: Rect, desktop: &[u32], stride: u32, screen: Size) {
    let (width, height) = (frame.size.width as usize, frame.size.height as usize);
    if width == 0 || height == 0 || output.len() < width * height {
        return;
    }

    const STEP: usize = 4;
    let small_w = width.div_ceil(STEP);
    let small_h = height.div_ceil(STEP);
    // Reuse the tail of `output` would alias; keep the small copy on the
    // heap instead (a Dock at 2x is ~0.2 MB here).
    let Some(mut small) = Pixels::new(small_w * small_h * 2) else { return; };
    let (small, scratch) = small.as_mut_slice().split_at_mut(small_w * small_h);

    let sample = |x: i32, y: i32| -> u32 {
        let x = x.clamp(0, screen.width as i32 - 1) as usize;
        let y = y.clamp(0, screen.height as i32 - 1) as usize;
        desktop.get(y * stride as usize + x).copied().unwrap_or(0)
    };

    // Downsample: the average of each STEP x STEP block.
    for sy in 0..small_h {
        for sx in 0..small_w {
            let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
            for dy in 0..STEP {
                for dx in 0..STEP {
                    let pixel = sample(frame.origin.x + (sx * STEP + dx) as i32, frame.origin.y + (sy * STEP + dy) as i32);
                    r += (pixel >> 16) & 0xFF;
                    g += (pixel >> 8) & 0xFF;
                    b += pixel & 0xFF;
                }
            }
            let n = (STEP * STEP) as u32;
            small[sy * small_w + sx] = ((r / n) << 16) | ((g / n) << 8) | (b / n);
        }
    }

    let radius = (pt(10) as usize / STEP).max(2);
    for _ in 0..3 {
        box_blur(small, scratch, small_w, small_h, radius, true);
        box_blur(scratch, small, small_w, small_h, radius, false);
    }

    // Upsample bilinearly back to full size.
    for y in 0..height {
        let fy = (y * 256 + 128) / STEP;
        let y0 = (fy / 256).saturating_sub(0).min(small_h - 1);
        let wy = (fy % 256) as u32;
        let y1 = (y0 + 1).min(small_h - 1);
        for x in 0..width {
            let fx = (x * 256 + 128) / STEP;
            let x0 = (fx / 256).min(small_w - 1);
            let wx = (fx % 256) as u32;
            let x1 = (x0 + 1).min(small_w - 1);
            let p = |xx: usize, yy: usize| small[yy * small_w + xx];
            let lerp = |a: u32, b: u32, w: u32, shift: u32| (((a >> shift) & 0xFF) * (256 - w) + ((b >> shift) & 0xFF) * w) >> 8;
            let mut out = 0u32;
            for shift in [0u32, 8, 16] {
                let top = lerp(p(x0, y0), p(x1, y0), wx, shift);
                let bottom = lerp(p(x0, y1), p(x1, y1), wx, shift);
                let value = (top * (256 - wy) + bottom * wy) >> 8;
                out |= value.min(255) << shift;
            }
            output[y * width + x] = out;
        }
    }
}

/// One box-blur pass of `radius` along rows (`horizontal`) or columns.
fn box_blur(input: &[u32], output: &mut [u32], width: usize, height: usize, radius: usize, horizontal: bool) {
    let (lines, length) = if horizontal { (height, width) } else { (width, height) };
    let at = |line: usize, index: usize| if horizontal { line * width + index } else { index * width + line };
    let window = (radius * 2 + 1) as u32;

    for line in 0..lines {
        let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
        let fetch = |index: isize| input[at(line, index.clamp(0, length as isize - 1) as usize)];
        for offset in -(radius as isize)..=radius as isize {
            let pixel = fetch(offset);
            r += (pixel >> 16) & 0xFF;
            g += (pixel >> 8) & 0xFF;
            b += pixel & 0xFF;
        }
        for index in 0..length {
            output[at(line, index)] = ((r / window) << 16) | ((g / window) << 8) | (b / window);
            let leaving = fetch(index as isize - radius as isize);
            let entering = fetch(index as isize + radius as isize + 1);
            r = r + ((entering >> 16) & 0xFF) - ((leaving >> 16) & 0xFF);
            g = g + ((entering >> 8) & 0xFF) - ((leaving >> 8) & 0xFF);
            b = b + (entering & 0xFF) - (leaving & 0xFF);
        }
    }
}
