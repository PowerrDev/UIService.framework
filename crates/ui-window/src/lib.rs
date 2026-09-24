#![no_std]

//! Standard sevOS window chrome, geometry and dragging policy.

use ui_core::{system_color, Color, Event, Point, PointerButton, Rect, Size};
use ui_render::Canvas;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowStyle {
    pub background: Color,
    pub titlebar: Color,
    pub border: Color,
    pub titlebar_height: u32,
    pub corner_radius: u32,
}

impl WindowStyle {
    // 56/22, not a real Aqua window's 28pt titlebar and 11pt corner radius at
    // 1x: this canvas is physical-pixel (2x) density, so chrome metrics need
    // twice the real point value to land at the same on-screen size (see
    // about-sevos/src/panel.rs). corner_radius is a further deliberate 20%
    // increase on top of that (19pt-equivalent at 1x, up from 16pt) for a
    // visibly rounder window than the plain doubled value would give. Note
    // that what is drawn is `Window::corner_radius()`, which caps this at the
    // titlebar's height (and half the window): the 256 here effectively means
    // "as round as the titlebar allows", the same at the top and the bottom.
    pub const DEFAULT: Self = Self {
        background: system_color::WINDOW_BACKGROUND,
        titlebar: system_color::TITLEBAR,
        border: Color::rgb(190, 192, 198),
        titlebar_height: 64,
        corner_radius: 256,
    };
}

impl Default for WindowStyle {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Which edge(s) of a window a resize drag is anchored to. More than one
/// field set at once means a corner grab (e.g. `right && bottom`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResizeEdges {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

impl ResizeEdges {
    pub const fn none() -> Self {
        Self { left: false, right: false, top: false, bottom: false }
    }

    pub const fn is_none(self) -> bool {
        !self.left && !self.right && !self.top && !self.bottom
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ResizeState {
    edges: ResizeEdges,
    start_frame: Rect,
    start_pointer: Point,
}

/// How close a point needs to be to a resizable window's edge, in 1x design
/// points, to count as grabbing that edge rather than the titlebar or the
/// content behind it.
const RESIZE_MARGIN_POINTS: u32 = 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Window {
    frame: Rect,
    style: WindowStyle,
    min_size: Size,
    max_size: Size,
    resizable: bool,
    drag_offset: Option<Point>,
    resize_state: Option<ResizeState>,
}

impl Window {
    pub const fn new(frame: Rect, style: WindowStyle) -> Self {
        Self {
            frame,
            style,
            min_size: frame.size,
            max_size: frame.size,
            resizable: false,
            drag_offset: None,
            resize_state: None,
        }
    }

    /// Allow this window to be resized by dragging its edges/corners,
    /// between `min_size` and `max_size` (both already rescaled for the
    /// host's content scale -- see `WindowConfig::effective_min_size`/
    /// `effective_max_size`).
    pub fn with_resize_limits(mut self, min_size: Size, max_size: Size) -> Self {
        self.min_size = Size::new(min_size.width.min(max_size.width), min_size.height.min(max_size.height));
        self.max_size = Size::new(min_size.width.max(max_size.width), min_size.height.max(max_size.height));
        self.resizable = true;
        self
    }

    pub const fn frame(&self) -> Rect {
        self.frame
    }

    pub const fn style(&self) -> WindowStyle {
        self.style
    }

    /// The radius the window's corners are actually drawn with, top and
    /// bottom alike: `style.corner_radius`, but never more than half the
    /// window or the titlebar's height. The titlebar is drawn as its own
    /// top-rounded band, which cannot curve further than it is tall, so a
    /// larger radius would only ever show at the bottom and the window would
    /// no longer match itself.
    pub fn corner_radius(&self) -> u32 {
        let radius = self.style.corner_radius.min(self.frame.size.width / 2).min(self.frame.size.height / 2);
        if self.style.titlebar_height > 0 { radius.min(self.style.titlebar_height) } else { radius }
    }

    pub const fn is_dragging(&self) -> bool {
        self.drag_offset.is_some()
    }

    pub const fn is_resizing(&self) -> bool {
        self.resize_state.is_some()
    }

    /// The edge(s) an in-progress resize is anchored to (`ResizeEdges::none()`
    /// if not resizing) -- the ones a drag started on, not `resize_edges_at`
    /// recomputed from wherever the pointer has since dragged to, which can
    /// easily be well past the edge-grab margin mid-drag (e.g. a fast pull
    /// toward the corner of the screen).
    pub fn resizing_edges(&self) -> ResizeEdges {
        self.resize_state.map_or(ResizeEdges::none(), |state| state.edges)
    }

    /// Which edge(s) of this window `point` (in the same coordinate space
    /// as `frame`) is close enough to grab for a resize, or
    /// `ResizeEdges::none()` if this window isn't resizable or `point`
    /// isn't near any edge at all.
    pub fn resize_edges_at(&self, point: Point) -> ResizeEdges {
        if !self.resizable {
            return ResizeEdges::none();
        }

        let margin = ui_core::scale::pt(RESIZE_MARGIN_POINTS) as i32;
        let left_edge = self.frame.origin.x;
        let top_edge = self.frame.origin.y;
        let right_edge = left_edge + self.frame.size.width as i32;
        let bottom_edge = top_edge + self.frame.size.height as i32;

        let outer = Rect::new(
            left_edge - margin,
            top_edge - margin,
            self.frame.size.width + (margin as u32) * 2,
            self.frame.size.height + (margin as u32) * 2,
        );
        if !outer.contains(point) {
            return ResizeEdges::none();
        }

        ResizeEdges {
            left: (point.x - left_edge).abs() <= margin,
            right: (point.x - right_edge).abs() <= margin,
            top: (point.y - top_edge).abs() <= margin,
            bottom: (point.y - bottom_edge).abs() <= margin,
        }
    }

    /// The frame a resize drag anchored at `state` produces at `pointer`'s
    /// current position: the edge(s) being dragged move with the pointer,
    /// the opposite edge(s) stay put, and the result is clamped to
    /// `min_size`/`max_size` by pulling the *dragged* edge back in (not the
    /// anchored one), so the window never creeps past its limit at the
    /// wrong corner.
    fn resized_frame(&self, state: ResizeState, pointer: Point) -> Rect {
        let dx = pointer.x - state.start_pointer.x;
        let dy = pointer.y - state.start_pointer.y;
        let start = state.start_frame;

        let mut left = start.origin.x;
        let mut top = start.origin.y;
        let mut right = left + start.size.width as i32;
        let mut bottom = top + start.size.height as i32;

        if state.edges.left {
            left += dx;
        }
        if state.edges.right {
            right += dx;
        }
        if state.edges.top {
            top += dy;
        }
        if state.edges.bottom {
            bottom += dy;
        }

        let width = ((right - left).max(0) as u32).clamp(self.min_size.width, self.max_size.width);
        let height = ((bottom - top).max(0) as u32).clamp(self.min_size.height, self.max_size.height);

        // Only the dragged edge needs pulling back in to land on the
        // clamped size -- the anchored (opposite) edge was never moved
        // from `start`, so `left`/`top` already sit at the right place
        // for a right/bottom-edge drag.
        if state.edges.left {
            left = right - width as i32;
        }
        if state.edges.top {
            top = bottom - height as i32;
        }

        Rect::new(left, top, width, height)
    }

    pub fn set_origin(&mut self, origin: Point) {
        self.frame.origin = origin;
    }

    pub fn titlebar_rect(&self) -> Rect {
        Rect::new(
            self.frame.origin.x,
            self.frame.origin.y,
            self.frame.size.width,
            self.style.titlebar_height.min(self.frame.size.height),
        )
    }

    pub fn content_rect(&self) -> Rect {
        let titlebar_height = self.style.titlebar_height.min(self.frame.size.height);
        Rect::new(
            self.frame.origin.x,
            self.frame.origin.y + titlebar_height as i32,
            self.frame.size.width,
            self.frame.size.height.saturating_sub(titlebar_height),
        )
    }

    pub fn local_content_rect(&self) -> Rect {
        let titlebar_height = self.style.titlebar_height.min(self.frame.size.height);
        Rect::new(
            0,
            titlebar_height as i32,
            self.frame.size.width,
            self.frame.size.height.saturating_sub(titlebar_height),
        )
    }

    /*
     * Track a titlebar drag without snapping the window origin to the pointer.
     * drag_offset remembers the exact point at which the titlebar was grabbed.
     */
    pub fn handle_event(&mut self, event: Event, bounds: Size) -> bool {
        match event {
            Event::PointerDown {
                position,
                button: PointerButton::Primary,
            } => {
                // Edges win over the titlebar drag: a corner near the
                // titlebar should resize, not move, matching how every
                // desktop window manager treats that overlap.
                let edges = self.resize_edges_at(position);
                if !edges.is_none() {
                    self.resize_state = Some(ResizeState {
                        edges,
                        start_frame: self.frame,
                        start_pointer: position,
                    });
                    true
                } else if self.titlebar_rect().contains(position) {
                    self.drag_offset = Some(Point::new(
                        position.x - self.frame.origin.x,
                        position.y - self.frame.origin.y,
                    ));
                    true
                } else {
                    false
                }
            }
            Event::PointerMoved { position } => {
                if let Some(state) = self.resize_state {
                    let old = self.frame;
                    self.frame = self.resized_frame(state, position);
                    self.frame != old
                } else if let Some(offset) = self.drag_offset {
                    let old = self.frame.origin;
                    self.frame.origin = Point::new(position.x - offset.x, position.y - offset.y);
                    self.clamp_to(bounds);
                    self.frame.origin != old
                } else {
                    false
                }
            }
            Event::PointerUp {
                button: PointerButton::Primary,
                ..
            }
            | Event::PointerLeft => {
                let was_resizing = self.resize_state.take().is_some();
                let was_dragging = self.drag_offset.take().is_some();
                was_resizing || was_dragging
            }
            _ => false,
        }
    }

    pub fn clamp_to(&mut self, bounds: Size) {
        let visible_titlebar = 80i32.min(self.frame.size.width as i32);
        let min_x = -(self.frame.size.width as i32 - visible_titlebar);
        let max_x = bounds.width as i32 - visible_titlebar;
        let min_y = 0;
        let max_y = (bounds.height as i32 - self.style.titlebar_height as i32).max(0);

        self.frame.origin.x = self.frame.origin.x.clamp(min_x, max_x);
        self.frame.origin.y = self.frame.origin.y.clamp(min_y, max_y);
    }

    pub fn draw_chrome<C: Canvas + ?Sized>(&self, canvas: &mut C, local: bool) {
        let origin = if local { Point::new(0, 0) } else { self.frame.origin };
        let frame = Rect::new(origin.x, origin.y, self.frame.size.width, self.frame.size.height);
        let titlebar_height = self.style.titlebar_height.min(self.frame.size.height);
        let radius = self.corner_radius();

        canvas.fill_rounded_rect(frame, radius, self.style.border);

        let inset = 1u32.min(self.frame.size.width / 2).min(self.frame.size.height / 2);
        let inner = Rect::new(
            origin.x + inset as i32,
            origin.y + inset as i32,
            self.frame.size.width.saturating_sub(inset * 2),
            self.frame.size.height.saturating_sub(inset * 2),
        );
        canvas.fill_rounded_rect(inner, radius.saturating_sub(inset), self.style.background);

        if titlebar_height > 0 && self.frame.size.width > 24 {
            /*
             * The titlebar is part of the rounded window, not a second pill
             * layered on top. A single separator is enough to establish the
             * drag region without making the chrome look like a demo widget.
             */
            canvas.fill_rect(
                Rect::new(
                    origin.x + 12,
                    origin.y + titlebar_height as i32 - 1,
                    self.frame.size.width - 24,
                    1,
                ),
                self.style.titlebar,
            );
        }
    }
}

/// Whether local pixel (`x`, `y`) of a `width` x `height` rectangle falls
/// outside its `radius` corners -- exactly the test `Surface::fill_rounded_rect`
/// uses, so a corner cut here lines up pixel for pixel with one drawn there.
fn outside_corner(x: i32, y: i32, width: i32, height: i32, radius: i32) -> bool {
    if radius <= 0 {
        return x < 0 || y < 0 || x >= width || y >= height;
    }
    let nearest_x = if x < radius { radius - 1 } else if x >= width - radius { width - radius } else { x };
    let nearest_y = if y < radius { radius - 1 } else if y >= height - radius { height - radius } else { y };
    let dx = (x - nearest_x) as i64;
    let dy = (y - nearest_y) as i64;
    dx.pow(4) + dy.pow(4) > (radius as i64).pow(4)
}

impl Window {
    /// Re-establish the window's outline below the titlebar after the content
    /// has been drawn: cut the bottom corners back to `outside` (the colour
    /// the compositor treats as "not part of the window") and redraw the
    /// border along the sides and bottom.
    ///
    /// Needed because content is drawn into a plain rectangle -- an app's
    /// `ui.fill` covers the whole of it, square corners and border included --
    /// so the rounded shape `draw_chrome` cut out would otherwise only
    /// survive at the top, where the titlebar is. Cheap enough to run after
    /// every redraw: it touches the one-pixel border and the two bottom
    /// corner squares, nothing else.
    pub fn draw_content_mask<C: Canvas + ?Sized>(&self, canvas: &mut C, outside: Color, local: bool) {
        let origin = if local { Point::new(0, 0) } else { self.frame.origin };
        let width = self.frame.size.width as i32;
        let height = self.frame.size.height as i32;
        if width < 2 || height < 2 {
            return;
        }
        let top = self.style.titlebar_height.min(self.frame.size.height) as i32;
        let radius = self.corner_radius() as i32;
        let inner_radius = (radius - 1).max(0);
        let border = self.style.border;
        let pixel = |canvas: &mut C, x: i32, y: i32, color: Color| {
            canvas.fill_rect(Rect::new(origin.x + x, origin.y + y, 1, 1), color);
        };

        // Straight edges, between the titlebar and the bottom corners.
        let corner_top = (height - radius).max(top);
        if corner_top > top {
            let length = (corner_top - top) as u32;
            canvas.fill_rect(Rect::new(origin.x, origin.y + top, 1, length), border);
            canvas.fill_rect(Rect::new(origin.x + width - 1, origin.y + top, 1, length), border);
        }
        if width > radius * 2 {
            canvas.fill_rect(Rect::new(origin.x + radius, origin.y + height - 1, (width - radius * 2) as u32, 1), border);
        }

        // The two bottom corners: outside the outer curve is not window at
        // all; between it and the inner curve is border.
        for y in corner_top..height {
            for x in (0..radius.min(width)).chain((width - radius).max(radius)..width) {
                if outside_corner(x, y, width, height, radius) {
                    pixel(canvas, x, y, outside);
                } else if x == 0 || x == width - 1 || y == height - 1
                    || outside_corner(x - 1, y - 1, width - 2, height - 2, inner_radius)
                {
                    pixel(canvas, x, y, border);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Draw a window the way the runtime does -- chrome, then content that
    /// covers its whole rectangle, then the mask -- and hand back its pixels.
    fn render(width: u32, height: u32, style: WindowStyle, pixels: &mut [u32]) -> Window {
        const OUTSIDE: Color = Color::rgb(255, 0, 255);
        let window = Window::new(Rect::new(0, 0, width, height), style);
        let mut surface = ui_render::Surface::new(pixels, width, height, width).expect("surface");
        surface.fill(OUTSIDE);
        window.draw_chrome(&mut surface, true);
        surface.fill_rect(window.local_content_rect(), Color::rgb(10, 200, 30));
        window.draw_content_mask(&mut surface, OUTSIDE, true);
        window
    }

    #[test]
    fn bottom_corners_are_as_round_as_the_top_ones_after_content_is_drawn() {
        let (width, height) = (200u32, 120u32);
        let style = WindowStyle { titlebar_height: 24, corner_radius: 20, ..WindowStyle::DEFAULT };
        let mut pixels = [0u32; 200 * 120];
        let window = render(width, height, style, &mut pixels);
        let outside = Color::rgb(255, 0, 255).to_xrgb8888();
        let at = |x: u32, y: u32| pixels[(y * width + x) as usize];

        assert_eq!(at(0, height - 1), outside, "bottom-left corner is cut");
        assert_eq!(at(width - 1, height - 1), outside, "bottom-right corner is cut");

        // Mirror image of the top edge: the same pixels are outside the window.
        let radius = window.corner_radius();
        for y in 0..radius {
            for x in 0..width {
                let top_out = at(x, y) == outside;
                let bottom_out = at(x, height - 1 - y) == outside;
                assert_eq!(top_out, bottom_out, "x {x}, {y} rows from the edge");
            }
        }
    }

    #[test]
    fn the_border_survives_content_drawn_over_it() {
        let (width, height) = (200u32, 120u32);
        let style = WindowStyle { titlebar_height: 24, corner_radius: 20, ..WindowStyle::DEFAULT };
        let mut pixels = [0u32; 200 * 120];
        render(width, height, style, &mut pixels);
        let border = style.border.to_xrgb8888();
        let at = |x: u32, y: u32| pixels[(y * width + x) as usize];

        assert_eq!(at(0, height / 2), border, "left edge");
        assert_eq!(at(width - 1, height / 2), border, "right edge");
        assert_eq!(at(width / 2, height - 1), border, "bottom edge");
        assert_ne!(at(width / 2, height / 2), border, "the content itself is untouched");
    }

    #[test]
    fn the_radius_never_exceeds_what_the_titlebar_can_show() {
        let style = WindowStyle { titlebar_height: 32, corner_radius: 256, ..WindowStyle::DEFAULT };
        let window = Window::new(Rect::new(0, 0, 800, 600), style);
        assert_eq!(window.corner_radius(), 32);
        let small = Window::new(Rect::new(0, 0, 40, 20), style);
        assert_eq!(small.corner_radius(), 10);
    }

    #[test]
    fn resizing_edges_is_none_until_a_resize_actually_starts() {
        let window = Window::new(Rect::new(0, 0, 200, 100), WindowStyle::DEFAULT)
            .with_resize_limits(Size::new(50, 50), Size::new(400, 400));
        assert!(window.resizing_edges().is_none());
    }

    #[test]
    fn resizing_edges_stays_anchored_to_the_grabbed_corner_even_after_the_pointer_leaves_the_margin() {
        let mut window = Window::new(Rect::new(0, 0, 200, 100), WindowStyle::DEFAULT)
            .with_resize_limits(Size::new(50, 50), Size::new(400, 400));

        // Grab the bottom-right corner.
        let bottom_right = Point::new(200, 100);
        assert!(window.handle_event(
            Event::PointerDown { position: bottom_right, button: PointerButton::Primary },
            Size::new(1024, 768),
        ));
        let edges = window.resizing_edges();
        assert!(edges.right && edges.bottom, "grabbed the bottom-right corner");
        assert!(!edges.left && !edges.top);

        // Drag far past where resize_edges_at would ever recognize an edge
        // (deep into the window's own interior): still anchored to the same
        // corner, not recomputed from the pointer's current position -- see
        // resizing_edges's own doc comment.
        window.handle_event(Event::PointerMoved { position: Point::new(150, 80) }, Size::new(1024, 768));
        let edges = window.resizing_edges();
        assert!(edges.right && edges.bottom, "still anchored to the corner the drag started on");

        window.handle_event(Event::PointerUp { position: Point::new(150, 80), button: PointerButton::Primary }, Size::new(1024, 768));
        assert!(window.resizing_edges().is_none(), "released: no longer resizing");
    }
}
