#![no_std]

//! Small application model shared by sevOS apps and UIService runtimes.
//!
//! Apps implement [`App`]. UIService owns window chrome, positioning, input
//! routing, presentation and platform integration; app code only declares its
//! metadata/window preferences and draws its content.

use ui_core::{Color, Event, Point, Rect, Size};
use ui_render::{Canvas, TextRenderer};
use ui_support::AppInfo;
use ui_window::WindowStyle;

/// Result of delivering an input event to an application.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AppAction {
    /// No visual state changed.
    #[default]
    None,
    /// Rebuild the app's window contents and present the result.
    Redraw,
    /// Ask the runtime to close the application.
    Close,
}

/// What the pointer should show over an app's content -- orthogonal to
/// [`AppAction`] (hovering a button both redraws its highlight *and* wants a
/// different cursor, so this is a separate query, not another `AppAction`
/// variant) and to window chrome's own titlebar/drag/resize cursor, which the
/// runtime decides on its own without asking the app. Mirrors
/// `windowserver::CursorKind` (a different framework: no direct dependency,
/// just the same concept under the same name); see [`App::cursor_kind`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CursorKind {
    Arrow,
    Hand,
    NotAllowed,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FontWeight {
    #[default]
    Regular,
    Semibold,
}

impl FontWeight {
    const fn is_semibold(self) -> bool {
        matches!(self, Self::Semibold)
    }
}

trait FrameText {
    fn measure(&self, text: &str, point_size: u32, weight: FontWeight) -> Size;
    fn draw(
        &mut self,
        canvas: &mut dyn Canvas,
        origin: Point,
        text: &str,
        color: Color,
        point_size: u32,
        weight: FontWeight,
    );
}

impl<T: TextRenderer> FrameText for T {
    fn measure(&self, text: &str, point_size: u32, weight: FontWeight) -> Size {
        TextRenderer::measure(self, text, point_size, weight.is_semibold())
    }

    fn draw(
        &mut self,
        canvas: &mut dyn Canvas,
        origin: Point,
        text: &str,
        color: Color,
        point_size: u32,
        weight: FontWeight,
    ) {
        TextRenderer::draw(
            self,
            canvas,
            origin,
            text,
            color,
            point_size,
            weight.is_semibold(),
        );
    }
}

/// Drawing surface handed to an application.
///
/// `Frame` keeps the common app API compact while still exposing [`Canvas`]
/// for reusable lower-level widgets through [`Frame::canvas`].
pub struct Frame<'a> {
    canvas: &'a mut dyn Canvas,
    text: &'a mut dyn FrameText,
}

impl<'a> Frame<'a> {
    pub fn new<C: Canvas, T: TextRenderer>(canvas: &'a mut C, text: &'a mut T) -> Self {
        Self { canvas, text }
    }

    pub fn size(&self) -> Size {
        self.canvas.size()
    }

    pub fn canvas(&mut self) -> &mut dyn Canvas {
        self.canvas
    }

    pub fn fill(&mut self, color: Color) {
        self.canvas.fill(color);
    }

    pub fn fill_rect(&mut self, rect: Rect, color: Color) {
        self.canvas.fill_rect(rect, color);
    }

    pub fn fill_rounded_rect(&mut self, rect: Rect, radius: u32, color: Color) {
        self.canvas.fill_rounded_rect(rect, radius, color);
    }

    pub fn fill_circle(&mut self, center: Point, radius: u32, color: Color) {
        self.canvas.fill_circle(center, radius, color);
    }

    pub fn measure(&self, text: &str, point_size: u32) -> Size {
        self.measure_with_weight(text, point_size, FontWeight::Regular)
    }

    pub fn measure_with_weight(
        &self,
        text: &str,
        point_size: u32,
        weight: FontWeight,
    ) -> Size {
        self.text.measure(text, point_size, weight)
    }

    pub fn text(&mut self, origin: Point, text: &str, color: Color, point_size: u32) {
        self.text_with_weight(origin, text, color, point_size, FontWeight::Regular);
    }

    pub fn text_semibold(&mut self, origin: Point, text: &str, color: Color, point_size: u32) {
        self.text_with_weight(origin, text, color, point_size, FontWeight::Semibold);
    }

    pub fn text_with_weight(
        &mut self,
        origin: Point,
        text: &str,
        color: Color,
        point_size: u32,
        weight: FontWeight,
    ) {
        self.text
            .draw(self.canvas, origin, text, color, point_size, weight);
    }
}

/// Initial window preferences for an [`App`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowConfig {
    pub size: Size,
    pub min_size: Size,
    pub max_size: Option<Size>,
    pub resizable: bool,
    pub style: WindowStyle,
}

impl WindowConfig {
    /// Create a standard non-resizable window.
    pub const fn new(width: u32, height: u32) -> Self {
        let size = Size::new(width, height);
        Self {
            size,
            min_size: size,
            max_size: Some(size),
            resizable: false,
            style: WindowStyle::DEFAULT,
        }
    }

    /// Allow the runtime to resize this window between `min` and `max`.
    pub const fn resizable(mut self, min: Size, max: Option<Size>) -> Self {
        self.min_size = min;
        self.max_size = max;
        self.resizable = true;
        self
    }

    /// Override the standard sevOS window style.
    pub const fn style(mut self, style: WindowStyle) -> Self {
        self.style = style;
        self
    }

    /// Number of pixels required by a tightly packed window backing store,
    /// sized for the *largest* this window can ever be -- `max_size` when
    /// resizable (a plain compile-time value, not `effective_max_size`,
    /// since this has to stay a `const fn` usable where the static backing
    /// array itself is sized), `size` otherwise.
    pub const fn pixel_count(self) -> usize {
        let max = match self.max_size {
            Some(max) => max,
            None => self.size,
        };
        max.width as usize * max.height as usize
    }

    /// The `min_size` counterpart to [`effective_size`](Self::effective_size):
    /// `min_size` is a 1x-doubled reference like everything else here, so it
    /// gets the same rescaling.
    pub fn effective_min_size(&self) -> Size {
        Size::new(
            ui_core::scale::pt(self.min_size.width / 2).max(1),
            ui_core::scale::pt(self.min_size.height / 2).max(1),
        )
    }

    /// The `max_size` counterpart to [`effective_size`](Self::effective_size).
    /// Unlike `size` and `min_size`, this is deliberately *not* rescaled by
    /// content scale: it's the static backing store's actual pixel capacity
    /// (see `pixel_count`), a hard ceiling that stays put regardless of
    /// host density -- rescaling it down on a lower-density host would
    /// leave that host with no room to resize past its (already smaller)
    /// initial size at all.
    pub fn effective_max_size(&self) -> Size {
        self.max_size.unwrap_or(self.size)
    }

    /// `size`/`style` are authored as a fixed 2x-density reference (see the
    /// "twice its real 1x-point value" convention documented on
    /// `ui_window::WindowStyle::DEFAULT`) and also size this app's static
    /// pixel backing store, so they are a hard upper bound. This recomputes
    /// the window actually drawn at runtime, rescaled to the host's real
    /// content scale (`ui_core::scale`, set from the connected host ABI)
    /// instead of assuming that reference density unconditionally, and
    /// clamped within [`effective_min_size`](Self::effective_min_size) and
    /// [`effective_max_size`](Self::effective_max_size) (a plain `.clamp(1,
    /// size)` for a non-resizable window, since `min_size`/`max_size` both
    /// equal `size` there).
    pub fn effective_size(&self) -> Size {
        let min = self.effective_min_size();
        let max = self.effective_max_size();
        Size::new(
            ui_core::scale::pt(self.size.width / 2).clamp(min.width.min(max.width), max.width),
            ui_core::scale::pt(self.size.height / 2).clamp(min.height.min(max.height), max.height),
        )
    }

    /// The `style` counterpart to [`effective_size`](Self::effective_size).
    pub fn effective_style(&self) -> WindowStyle {
        let style = self.style;
        WindowStyle {
            titlebar_height: ui_core::scale::pt(style.titlebar_height / 2).min(style.titlebar_height),
            corner_radius: ui_core::scale::pt(style.corner_radius / 2).min(style.corner_radius),
            ..style
        }
    }

    /// The content area (window minus titlebar) an app actually gets drawn
    /// into at runtime -- what `Frame::size` reports inside `App::draw`.
    /// Apps whose `event` has no `Frame` to ask (see `App::event`) should
    /// call this instead of hardcoding a second, possibly stale copy of the
    /// same arithmetic.
    pub fn effective_content_size(&self) -> Size {
        let size = self.effective_size();
        let titlebar_height = self.effective_style().titlebar_height.min(size.height);
        Size::new(size.width, size.height.saturating_sub(titlebar_height))
    }
}

/// Minimal interface implemented by a sevOS graphical application.
///
/// `draw` receives the **content area only**. UIService draws and manages the
/// title bar/chrome around it. Pointer events delivered to `event` are also
/// translated into content-local coordinates.
/// One entry of a menu in the menu bar: a command the app handles in
/// [`App::menu_command`], or a divider.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuItem {
    Command { title: &'static str, command: u32 },
    Separator,
}

impl MenuItem {
    pub const fn command(title: &'static str, command: u32) -> Self {
        Self::Command { title, command }
    }

    pub const SEPARATOR: Self = Self::Separator;
}

/// A menu-bar title and what drops down under it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Menu {
    pub title: &'static str,
    pub items: &'static [MenuItem],
}

impl Menu {
    pub const fn new(title: &'static str, items: &'static [MenuItem]) -> Self {
        Self { title, items }
    }
}

/// How a menu command shows right now: greyed out, and/or with a check mark.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MenuItemState {
    pub enabled: bool,
    pub checked: bool,
}

impl MenuItemState {
    pub const ENABLED: Self = Self { enabled: true, checked: false };
    pub const DISABLED: Self = Self { enabled: false, checked: false };

    pub const fn enabled(enabled: bool) -> Self {
        Self { enabled, checked: false }
    }

    pub const fn checked(checked: bool) -> Self {
        Self { enabled: true, checked }
    }
}

pub trait App {
    const INFO: AppInfo<'static>;
    const WINDOW: WindowConfig;

    fn draw(&mut self, ui: &mut Frame<'_>);

    /// `content_size` is the content area's *current* size -- the same
    /// thing `draw`'s `Frame::size` reports, kept in sync as the window is
    /// resized. `event` has no `Frame` of its own to ask (there is nothing
    /// to draw into between frames), so the runtime passes it along
    /// explicitly instead of an app hit-testing against a stale, no-longer
    /// resized value.
    fn event(&mut self, _event: Event, _content_size: Size) -> AppAction {
        AppAction::None
    }

    /// Whether an animation is running and the app wants [`tick`](Self::tick)
    /// called every frame. Idle apps return `false` (the default), so the
    /// runtime does no per-frame work for them.
    fn animating(&self) -> bool {
        false
    }

    /// Advance the app's animations to `now_us`, a monotonic microsecond
    /// clock, and say whether that changed what is on screen. Called once per
    /// runtime frame while [`animating`](Self::animating) is true, with the
    /// real time that has passed rather than a fixed step, so a slow frame
    /// makes an animation skip ahead instead of dragging on.
    fn tick(&mut self, _now_us: u64) -> AppAction {
        AppAction::None
    }

    /// What the pointer should show right now, if the app wants anything
    /// other than the plain arrow: queried by the runtime after every event
    /// that moved the pointer, once window chrome (titlebar, drag, resize)
    /// has had its own say and found nothing -- see [`CursorKind`]. `None`
    /// (the default) leaves the arrow showing; an app with no hoverable
    /// controls never needs to override this.
    fn cursor_kind(&self) -> Option<CursorKind> {
        None
    }

    /// The app's own menus, shown in the menu bar after its name while it is
    /// the frontmost app. The system adds the app-name menu (About, Quit) and
    /// the Window menu itself.
    const MENUS: &'static [Menu] = &[];

    /// Whether `command` (from [`App::MENUS`]) can be chosen right now, and
    /// whether it shows a check mark. Asked each time a menu opens.
    fn menu_item_state(&self, _command: u32) -> MenuItemState {
        MenuItemState::ENABLED
    }

    /// The user chose `command` from one of [`App::MENUS`].
    fn menu_command(&mut self, _command: u32) -> AppAction {
        AppAction::None
    }
}
