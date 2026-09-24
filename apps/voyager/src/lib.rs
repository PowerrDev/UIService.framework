//! Voyager — sevOS's Finder-equivalent: a folder browser with a sidebar, a
//! toolbar, list and icon views and a status line, styled after macOS Finder.
//!
//! armOS's Ignite (`frameworks/CoreServices/Ignite.app/main.cpp`) is the
//! inspiration, not the foundation: this is a from-scratch app against
//! UIService's Rust `ui` crate. That platform has no path or stroke drawing,
//! so `draw2d` adds anti-aliased strokes, polygons and rounded rects on top of
//! `Canvas::blend_pixel`, and `icons` draws every icon and glyph procedurally
//! (crisp at any content scale, no bitmaps). Input is the pointer and the
//! scroll wheel; there is no keyboard event, so no typing yet. Scrolling is
//! smooth: the wheel moves a target and `App::tick` eases the view towards it.
//!
//! Module map: `theme` is the palette and text helpers; `draw2d` and `icons`
//! are the drawing toolkit; `model` is the file/folder model and the sidebar
//! destinations; `nav` is the path stack and live listing; `scroll` is the
//! smooth-scrolling state; `layout` is the one geometry pass `draw` and
//! `event` both call; `toolbar`, `sidebar`, `content` and `status` each own one
//! region's drawing and hit testing.

// `not(test)`, not a bare `no_std`: unit tests below (in `nav`, `layout` and
// `content`) exercise navigation and hit-testing math on the host, where
// they can run under `cargo test` without the freestanding target. This has
// no effect on the real `aarch64-unknown-none-softfloat` build, which never
// sets `cfg(test)`.
#![cfg_attr(not(test), no_std)]

mod content;
mod draw2d;
mod icons;
mod layout;
mod model;
mod nav;
mod scroll;
mod sidebar;
mod status;
mod storage;
mod theme;
mod toolbar;

use ui::prelude::*;

use content::ViewMode;
use layout::Layout;
use model::DESTINATIONS;
use nav::Navigator;
use scroll::Scroller;
use toolbar::{ToolbarHit, ToolbarState};

const WINDOW_WIDTH: u32 = 940;
const WINDOW_HEIGHT: u32 = 680;

// A Finder-like window is one of the few sevOS windows that actually
// benefits from resizing (unlike, say, About sevOS's fixed panel), so
// Voyager opts in. `None` for max defaults to `WINDOW_WIDTH`/`HEIGHT`
// themselves -- the static backing store's actual capacity, see
// `WindowConfig::pixel_count` -- rather than a separately authored ceiling
// that could drift out of sync with it.
const MIN_WINDOW_WIDTH: u32 = 480;
const MIN_WINDOW_HEIGHT: u32 = 360;

/// How far one wheel notch scrolls, in points: a little under a row of the
/// list, so a single notch is a clear step and a trackpad's stream is smooth.
const SCROLL_NOTCH_POINTS: u32 = 22;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Hit {
    Toolbar(ToolbarHit),
    Sidebar(usize),
    Content(usize),
}

/// Whether pointing at `hit` changes anything on screen. The toolbar and the
/// sidebar light up under the pointer; list rows and icons do not (Finder's do
/// not either), so moving over them needs no repaint at all.
fn hover_is_visible(hit: Option<Hit>) -> bool {
    matches!(hit, Some(Hit::Toolbar(_)) | Some(Hit::Sidebar(_)))
}

/// Up to how many individual targets (hover *and* press each flip an old and a
/// new target) `PendingRedraw::Partial` tracks before giving up and falling
/// back to a full repaint.
const MAX_PARTIAL_TARGETS: usize = 6;

/// What the next `draw` call needs to do, decided in `event` so `draw`
/// itself never has to re-derive "what changed" from scratch.
///
/// Redrawing Voyager's entire window content (toolbar + sidebar + full file
/// list, each row individually measured and drawn) on every single
/// pointer-moved event is the dominant cost of just scrubbing the mouse
/// across the window under software rendering -- nothing about the list
/// actually changed, only which control is hovered (or pressed). `Partial`
/// captures exactly that case: the handful of targets whose hover/press state
/// actually flipped, so `draw` can repaint just those instead of the whole
/// window.
enum PendingRedraw {
    /// Nothing pending (or already handled).
    None,
    /// Only hover and/or press state changed since the last draw -- each
    /// entry is one row, tile or control that needs repainting with whatever
    /// its *current* state now is (see `VoyagerApp::redraw_target`).
    Partial { targets: [Option<Hit>; MAX_PARTIAL_TARGETS], count: usize },
    /// Anything else -- selection, navigation, view mode or sidebar
    /// visibility -- needs the normal full repaint.
    Full,
}

impl PendingRedraw {
    /// Record one target (a row, tile, sidebar row or toolbar control that was
    /// hovered or pressed a moment ago, or is now) as needing a repaint.
    /// Several of these can arrive in one batch (a hover change and a press
    /// change touch up to four targets between them, and more pointer-moved
    /// events can arrive before the next draw) -- they just accumulate, since
    /// nothing in between was ever actually painted. Once there's no room
    /// left, this escalates to `Full` instead of dropping one silently.
    fn note_target_change(&mut self, target: Option<Hit>) {
        let Some(hit) = target else { return; };

        match self {
            PendingRedraw::Full => {}
            PendingRedraw::None => {
                let mut targets = [None; MAX_PARTIAL_TARGETS];
                targets[0] = Some(hit);
                *self = PendingRedraw::Partial { targets, count: 1 };
            }
            PendingRedraw::Partial { targets, count } => {
                if targets[..*count].contains(&Some(hit)) {
                    // Already queued this batch (e.g. hover bounced back to
                    // a target it just left) -- no need for a second slot.
                } else if *count < MAX_PARTIAL_TARGETS {
                    targets[*count] = Some(hit);
                    *count += 1;
                } else {
                    *self = PendingRedraw::Full;
                }
            }
        }
    }

    fn note_full_change(&mut self) {
        *self = PendingRedraw::Full;
    }
}

fn hit_test(layout: &Layout, entry_count: usize, mode: ViewMode, scroll: i32, point: Point) -> Option<Hit> {
    if let Some(hit) = toolbar::hit_test(layout, point) {
        return Some(Hit::Toolbar(hit));
    }
    if let Some(index) = sidebar::hit_test(layout, point) {
        return Some(Hit::Sidebar(index));
    }
    if let Some(index) = content::hit_test(layout.content, entry_count, mode, scroll, point) {
        return Some(Hit::Content(index));
    }
    None
}

pub struct VoyagerApp {
    nav: Navigator,
    /// The sidebar entry whose own folder is showing, if any.
    current_destination: Option<usize>,
    selection: Option<usize>,
    view_mode: ViewMode,
    sidebar_visible: bool,
    /// Where the listing is scrolled to, and where it is easing to.
    scroller: Scroller,
    hovered: Option<Hit>,
    pressed: Option<Hit>,
    pending_redraw: PendingRedraw,
    /// The content size `draw` last actually ran a full pass against --
    /// compared on entry to catch a resize even though nothing in `self`
    /// otherwise records one. See `draw`.
    last_size: Size,
}

impl VoyagerApp {
    pub fn new() -> Self {
        let mut app = Self {
            nav: Navigator::new(),
            current_destination: None,
            selection: None,
            view_mode: ViewMode::List,
            sidebar_visible: true,
            scroller: Scroller::new(),
            hovered: None,
            pressed: None,
            pending_redraw: PendingRedraw::None,
            last_size: Size::new(0, 0),
        };
        app.sync_destination();
        app
    }

    /// The furthest the current listing can scroll in a content area of `size`.
    fn max_scroll(&self, size: Size) -> i32 {
        let layout = Layout::compute(size, self.sidebar_visible);
        content::max_scroll(layout.content, self.nav.entries().len(), self.view_mode)
    }

    /// Point the sidebar's highlight at whichever entry is the folder now
    /// showing (none, if it is not one of them). Called after any navigation.
    fn sync_destination(&mut self) {
        self.current_destination = model::destination_for_path(self.nav.path());
    }

    /// Repaint whatever single target `hit` refers to, using the *current*
    /// hover/press/selection state -- so calling this with a target that was
    /// just left draws it back to normal, and with one just entered (or
    /// pressed) draws it with that feedback. A toolbar target repaints the
    /// whole (cheap) toolbar band. See `PendingRedraw::Partial`.
    fn redraw_target(&self, ui: &mut Frame<'_>, layout: &Layout, hit: Option<Hit>) {
        match hit {
            Some(Hit::Content(index)) => {
                let pressed = matches!(self.pressed, Some(Hit::Content(i)) if i == index);
                content::redraw_row(ui, layout.content, self.nav.entries(), self.view_mode, index, self.selection, pressed);
            }
            Some(Hit::Sidebar(index)) => {
                let hovered = matches!(self.hovered, Some(Hit::Sidebar(i)) if i == index);
                let pressed = matches!(self.pressed, Some(Hit::Sidebar(i)) if i == index);
                sidebar::redraw_row(ui, layout, index, self.current_destination, hovered, pressed);
            }
            Some(Hit::Toolbar(_)) => self.draw_toolbar(ui, layout),
            None => {}
        }
    }

    fn draw_toolbar(&self, ui: &mut Frame<'_>, layout: &Layout) {
        let hovered = match self.hovered {
            Some(Hit::Toolbar(hit)) => Some(hit),
            _ => None,
        };
        let pressed = match self.pressed {
            Some(Hit::Toolbar(hit)) => Some(hit),
            _ => None,
        };
        toolbar::draw(
            ui,
            layout,
            &ToolbarState {
                can_go_back: self.nav.can_go_back(),
                can_go_forward: self.nav.can_go_forward(),
                view_mode: self.view_mode,
                open_enabled: self.open_enabled(),
                hovered,
                pressed,
                title: self.nav.title(),
            },
        );
    }

    fn open_enabled(&self) -> bool {
        self.selection
            .and_then(|index| self.nav.entries().get(index))
            .is_some_and(|entry| entry.is_folder())
    }

    /// Apply a completed click. Returns whether anything visible changed.
    fn activate(&mut self, hit: Hit) -> bool {
        match hit {
            Hit::Toolbar(ToolbarHit::SidebarToggle) => {
                self.sidebar_visible = !self.sidebar_visible;
                true
            }
            Hit::Toolbar(ToolbarHit::Back) => {
                let moved = self.nav.go_back();
                self.selection = None;
                self.scroller.reset();
                self.sync_destination();
                moved
            }
            Hit::Toolbar(ToolbarHit::Forward) => {
                let moved = self.nav.go_forward();
                self.selection = None;
                self.scroller.reset();
                self.sync_destination();
                moved
            }
            Hit::Toolbar(ToolbarHit::ViewList) => {
                let changed = self.view_mode != ViewMode::List;
                self.view_mode = ViewMode::List;
                if changed {
                    self.scroller.reset();
                }
                changed
            }
            Hit::Toolbar(ToolbarHit::ViewGrid) => {
                let changed = self.view_mode != ViewMode::Grid;
                self.view_mode = ViewMode::Grid;
                if changed {
                    self.scroller.reset();
                }
                changed
            }
            Hit::Toolbar(ToolbarHit::Open) => self.open_selection(),
            Hit::Sidebar(index) => {
                if self.current_destination == Some(index) {
                    return false;
                }
                self.nav.go_to_destination(&DESTINATIONS[index]);
                self.sync_destination();
                self.selection = None;
                self.scroller.reset();
                true
            }
            Hit::Content(index) => {
                if self.selection == Some(index) {
                    self.open_selection()
                } else {
                    self.selection = Some(index);
                    true
                }
            }
        }
    }

    /// Enter the selected entry if it is a folder. No-op for a file: there
    /// is no other app here to hand it off to.
    fn open_selection(&mut self) -> bool {
        let Some(index) = self.selection else {
            return false;
        };
        let Some(entry) = self.nav.entries().get(index).copied() else {
            return false;
        };
        if self.nav.enter(&entry) {
            self.selection = None;
            self.scroller.reset();
            self.sync_destination();
            true
        } else {
            false
        }
    }
}

impl App for VoyagerApp {
    const INFO: AppInfo<'static> = AppInfo::new("Voyager", "com.butterscotch.voyager", "0.1.0");
    const WINDOW: WindowConfig = WindowConfig::new(WINDOW_WIDTH, WINDOW_HEIGHT)
        .resizable(Size::new(MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT), None);

    fn draw(&mut self, ui: &mut Frame<'_>) {
        let size = ui.size();

        // A resize lands here as a plain, unannounced difference in what
        // `ui.size()` reports -- `event` is suppressed while a resize drag
        // is in progress (see `runtime.rs`), so nothing in `self` marks it
        // directly. Left unchecked, a `Partial` left pending from just
        // before the drag started would repaint only its stale targets
        // onto a freshly resized (and, on the runtime side, freshly
        // recleared) canvas, leaving the rest blank.
        if size != self.last_size {
            self.last_size = size;
            self.pending_redraw = PendingRedraw::Full;
            // A different size is a different range: keep the view inside it.
            let max = self.max_scroll(size);
            self.scroller.clamp(max);
        }

        // A scrolled listing has rows partly under the column header and the
        // toolbar, which a single-row repaint would overwrite: once scrolled,
        // everything repaints in full (and only what changed is recomposited).
        let pending = core::mem::replace(&mut self.pending_redraw, PendingRedraw::None);
        if let PendingRedraw::Partial { targets, count } = pending {
            if self.scroller.offset_px() == 0 {
                let layout = Layout::compute(size, self.sidebar_visible);
                let mut toolbar_drawn = false;
                for target in &targets[..count] {
                    // Every toolbar target repaints the same band: once is enough.
                    if matches!(target, Some(Hit::Toolbar(_))) {
                        if toolbar_drawn {
                            continue;
                        }
                        toolbar_drawn = true;
                    }
                    self.redraw_target(ui, &layout, *target);
                }
                return;
            }
        }

        ui.fill(theme::WINDOW_BACKGROUND);
        // Smaller than any window Voyager allows (its minimum is 240 x 148 pt
        // of content): nothing sensible fits.
        if size.width < scale::pt(200) || size.height < scale::pt(100) {
            return;
        }

        let layout = Layout::compute(size, self.sidebar_visible);
        let entries = self.nav.entries();

        // The listing first: a scrolled row or tile spills over the content
        // area's edges, and the toolbar and status line, drawn after it, cover
        // the spill.
        let pressed_content = match self.pressed {
            Some(Hit::Content(index)) => Some(index),
            _ => None,
        };
        content::draw(
            ui,
            layout.content,
            entries,
            self.view_mode,
            self.selection,
            pressed_content,
            self.scroller.offset_px(),
            self.scroller.indicator_alpha(),
        );

        self.draw_toolbar(ui, &layout);

        let hovered_sidebar = match self.hovered {
            Some(Hit::Sidebar(index)) => Some(index),
            _ => None,
        };
        let pressed_sidebar = match self.pressed {
            Some(Hit::Sidebar(index)) => Some(index),
            _ => None,
        };
        sidebar::draw(ui, &layout, self.current_destination, hovered_sidebar, pressed_sidebar);

        status::draw(ui, &layout, entries, self.selection, self.nav.truncated());
    }

    fn animating(&self) -> bool {
        self.scroller.animating()
    }

    fn tick(&mut self, now_us: u64) -> AppAction {
        let max = self.max_scroll(self.last_size);
        if self.scroller.tick(now_us, max) {
            // Scrolling moves the whole listing: a full repaint (the window
            // server recomposites only the pixels that actually changed).
            self.pending_redraw.note_full_change();
            AppAction::Redraw
        } else {
            AppAction::None
        }
    }

    /// A hand over anything that responds to a click (a sidebar destination,
    /// a file/folder in the listing -- Voyager's own "links"; a toolbar
    /// button); the "not allowed" ring over one of the toolbar buttons that
    /// currently does nothing (back/forward past either end of history, Open
    /// with nothing selected). The runtime only asks this for a hover that
    /// isn't already claimed by window chrome (the titlebar, a resize edge).
    fn cursor_kind(&self) -> Option<CursorKind> {
        let toolbar_hit = match self.hovered? {
            Hit::Sidebar(_) | Hit::Content(_) => return Some(CursorKind::Hand),
            Hit::Toolbar(hit) => hit,
        };

        let enabled = match toolbar_hit {
            ToolbarHit::Back => self.nav.can_go_back(),
            ToolbarHit::Forward => self.nav.can_go_forward(),
            ToolbarHit::Open => self.open_enabled(),
            ToolbarHit::SidebarToggle | ToolbarHit::ViewGrid | ToolbarHit::ViewList => true,
        };

        Some(if enabled { CursorKind::Hand } else { CursorKind::NotAllowed })
    }

    fn event(&mut self, event: Event, content_size: Size) -> AppAction {
        let layout = Layout::compute(content_size, self.sidebar_visible);
        let entry_count = self.nav.entries().len();
        let mode = self.view_mode;
        let scroll = self.scroller.offset_px();

        let changed = match event {
            Event::PointerMoved { position } => {
                let hit = hit_test(&layout, entry_count, mode, scroll, position);
                let old = self.hovered;
                if hit == old {
                    false
                } else {
                    self.hovered = hit;
                    // Only what visibly reacts to the pointer needs repainting.
                    if hover_is_visible(old) {
                        self.pending_redraw.note_target_change(old);
                    }
                    if hover_is_visible(hit) {
                        self.pending_redraw.note_target_change(hit);
                    }
                    hover_is_visible(old) || hover_is_visible(hit)
                }
            }
            Event::PointerDown { position, button: PointerButton::Primary } => {
                let hit = hit_test(&layout, entry_count, mode, scroll, position);
                let old = self.pressed;
                let changed = hit != old && hit.is_some();
                if changed {
                    self.pressed = hit;
                    self.pending_redraw.note_target_change(old);
                    self.pending_redraw.note_target_change(hit);
                }
                changed
            }
            Event::PointerUp { position, button: PointerButton::Primary } => {
                let released = hit_test(&layout, entry_count, mode, scroll, position);
                let pressed = self.pressed.take();
                let activated = pressed.is_some() && pressed == released && released.is_some_and(|hit| self.activate(hit));
                if activated {
                    // `activate` can change selection, navigation, view
                    // mode or sidebar visibility -- all wider than the one
                    // row that was pressed (notably, whichever row/tile
                    // held the *previous* selection also needs to lose its
                    // highlight, and nothing here tracks that on its own).
                    self.pending_redraw.note_full_change();
                } else if pressed.is_some() {
                    self.pending_redraw.note_target_change(pressed);
                }
                activated || pressed.is_some()
            }
            Event::PointerLeft => {
                let old_hovered = self.hovered;
                let old_pressed = self.pressed.take();
                self.hovered = None;
                if hover_is_visible(old_hovered) {
                    self.pending_redraw.note_target_change(old_hovered);
                }
                self.pending_redraw.note_target_change(old_pressed);
                hover_is_visible(old_hovered) || old_pressed.is_some()
            }
            Event::Scroll { position, delta } => {
                // The wheel scrolls the listing while the pointer is over it.
                // It only moves the target: the animation (`tick`) does the
                // drawing, a frame at a time, so nothing to repaint yet.
                if layout.content.contains(position) {
                    let max = content::max_scroll(layout.content, entry_count, mode);
                    self.scroller.scroll_by(delta, scale::pt_i32(SCROLL_NOTCH_POINTS as i32), max);
                }
                false
            }
            _ => false,
        };

        if changed {
            AppAction::Redraw
        } else {
            AppAction::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `VoyagerApp` is a stack local on the kernel's 16 KiB boot stack, and the
    /// memory just below that stack is the kernel's own state: growing the app
    /// (an embedded path stack was once 6.4 KiB of it) overflows into it and
    /// crashes the kernel at start-up rather than failing anywhere near here.
    /// Big buffers belong in `storage::StaticCell` statics.
    #[test]
    fn the_app_is_small_enough_to_live_on_the_boot_stack() {
        let app = core::mem::size_of::<VoyagerApp>();
        let navigator = core::mem::size_of::<Navigator>();
        assert!(app <= 1024, "VoyagerApp is {app} bytes");
        assert!(navigator <= 512, "Navigator is {navigator} bytes");
    }
}
