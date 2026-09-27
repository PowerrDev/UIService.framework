//! An app process's window, as the desktop keeps it.
//!
//! The app (`/Applications/<Name>.app`, running `ui-app-nxu`) draws its
//! content in its own address space and hands frames to the kernel's UI
//! session bridge; `RemoteApp` owns the WindowServer window around it: the
//! chrome, the backing store the frame is copied into, where the window is,
//! whether it is key or hidden. Everything the app has to know -- input in
//! its content's coordinates, its content size, menu commands, when to quit
//! -- it is sent as a message through the bridge, and nothing the app does
//! can stall the desktop: a frame that has not arrived yet simply is not
//! drawn yet.

use ui_core::{scale::pt, Color, Event, Point, PointerButton, Rect, Size};
use ui_render::{Canvas, Surface};
use ui_session as session;
use ui_window::{Window, WindowStyle};

use crate::bridge::{self, Pixels};
use crate::windowserver::{self, CursorKind, WindowId};
use crate::{aqua, scene, text};

/// How long an app has to quit when asked before it is ended.
const QUIT_GRACE_US: u64 = 2_000_000;

/// How long a live resize waits for the app's next frame before the window
/// takes the pointer's size anyway, without content. Long: it is for an app
/// that stopped answering, and a big window can take a few hundred
/// milliseconds to draw under emulation -- blank content flashing through a
/// resize looks worse than the window trailing the pointer.
const RESIZE_WAIT_US: u64 = 500_000;

/// What the app sent when it connected, kept by value: the bridge's copy
/// goes away with the connection, and the menu bar holds on to these names.
pub(crate) struct RemoteApp {
    connection: u32,
    pid: u32,
    info: session::Connect,
    window: Window,
    style: WindowStyle,
    id: Option<WindowId>,
    active: bool,
    backing: Pixels,
    /// The app's latest submit, and the bridge's count for it.
    state: session::Submit,
    sequence: u64,
    /// The content size the app was last told about.
    told: Size,
    /// A frame the app has not been asked for yet: the next pass asks.
    needs_redraw: bool,
    /// When the window's frame took a size WindowServer's window does not
    /// have yet (a live resize), waiting for the app's frame at that size.
    resize_since: Option<u64>,
    /// When the app was asked to quit, if it was.
    quitting_since: Option<u64>,
    /// The window has been on screen (so it has a place of its own).
    placed: bool,
}

impl RemoteApp {
    /// Take a new connection. `None` for a connection that is not an app
    /// (the Dock has its own host) or one whose window cannot be backed.
    pub(crate) fn new(connection: u32) -> Option<Self> {
        let info = bridge::info(connection)?;
        if info.kind != session::KIND_APP {
            return None;
        }

        let style = WindowStyle {
            titlebar_height: info.titlebar_height,
            corner_radius: info.corner_radius,
            ..WindowStyle::DEFAULT
        };
        let backing = Pixels::new(info.max_width as usize * info.max_height as usize)?;
        let mut window = Window::new(Rect::new(0, 0, info.width, info.height), style);
        if info.resizable != 0 {
            window = window.with_resize_limits(
                Size::new(info.min_width, info.min_height),
                Size::new(info.max_width, info.max_height),
            );
        }

        Some(Self {
            connection,
            pid: bridge::pid(connection),
            info,
            window,
            style,
            id: None,
            active: false,
            backing,
            state: session::Submit::empty(),
            sequence: 0,
            told: Size::new(0, 0),
            needs_redraw: true,
            resize_since: None,
            quitting_since: None,
            placed: false,
        })
    }

    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }

    /// The app's name, for the menu bar and the Window menu. `'static`
    /// because the desktop keeps every `RemoteApp` in a static and clears the
    /// menu bar before it lets go of one.
    pub(crate) fn name(&self) -> &'static str {
        static_str(session::get_str(&self.info.name))
    }

    pub(crate) fn menu_count(&self) -> usize {
        (self.info.menu_count as usize).min(session::MENU_MAX)
    }

    pub(crate) fn menu_title(&self, menu: usize) -> &'static str {
        static_str(session::get_str(&self.info.menu_titles[menu.min(session::MENU_MAX - 1)]))
    }

    /// The items of `menu`: (item index, command or `MENU_SEPARATOR`, title).
    pub(crate) fn menu_items(&self, menu: usize) -> impl Iterator<Item = (usize, u32, &'static str)> + '_ {
        let count = (self.info.item_count as usize).min(session::MENU_ITEM_MAX);
        self.info.items[..count]
            .iter()
            .enumerate()
            .filter(move |(_, item)| item.menu as usize == menu)
            .map(|(index, item)| (index, item.command, static_str(session::get_str(&item.title))))
    }

    /// (enabled, checked) of item `index`, as the app last reported them.
    pub(crate) fn menu_item_state(&self, index: usize) -> (bool, bool) {
        if self.state.flags & session::SUBMIT_MENU_STATE == 0 {
            return (true, false);
        }
        let state = self.state.menu_state.get(index).copied().unwrap_or(0);
        (state & session::MENU_ENABLED != 0, state & session::MENU_CHECKED != 0)
    }

    pub(crate) fn is_visible(&self) -> bool {
        self.id.is_some()
    }

    pub(crate) fn is_quitting(&self) -> bool {
        self.quitting_since.is_some()
    }

    pub(crate) fn window(&self) -> Option<Window> {
        self.id.map(|_| self.window)
    }

    pub(crate) fn window_id(&self) -> Option<WindowId> {
        self.id
    }

    /// Put the window on screen: at `origin` the first time, where it was
    /// when it was hidden after that.
    pub(crate) fn show(&mut self, screen: Size, origin: Point) -> bool {
        if self.id.is_some() || self.is_quitting() {
            return self.id.is_some();
        }
        if !self.placed {
            self.placed = true;
            self.window.set_origin(origin);
            self.window.clamp_to(screen);
        }

        let Some(id) = windowserver::Create_Window(self.window.frame(), false, self.window.corner_radius()) else {
            return false;
        };
        if self.info.resizable != 0 {
            windowserver::Set_Size_Limits(
                id,
                Size::new(self.info.min_width, self.info.min_height),
                Size::new(self.info.max_width, self.info.max_height),
            );
        }
        self.id = Some(id);
        windowserver::Set_Window_Opaque_Interior(id);
        windowserver::Set_Window_Key(id, self.active);
        self.render_full();
        true
    }

    /// Take the window off screen; the app keeps running.
    pub(crate) fn hide(&mut self) {
        if let Some(id) = self.id.take() {
            windowserver::Destroy_Window(id);
        }
        self.resize_since = None;
        self.active = false;
    }

    /// Ask the app to quit and take its window down at once. It gets
    /// `QUIT_GRACE_US` to exit before it is ended.
    pub(crate) fn quit(&mut self, now_us: u64) {
        self.hide();
        if self.quitting_since.is_none() {
            self.quitting_since = Some(now_us);
            bridge::post(self.connection, &session::Message::new(session::MSG_QUIT));
        }
    }

    /// Whether the app's process is gone (or was ended for not quitting):
    /// the desktop then drops it.
    pub(crate) fn finished(&mut self, now_us: u64) -> bool {
        if !bridge::alive(self.connection) {
            return true;
        }
        if let Some(since) = self.quitting_since {
            if now_us.saturating_sub(since) > QUIT_GRACE_US {
                bridge::terminate(self.connection);
            }
        }
        false
    }

    pub(crate) fn set_active(&mut self, active: bool) {
        if self.active == active {
            return;
        }
        self.active = active;
        if let Some(id) = self.id {
            windowserver::Set_Window_Key(id, active);
        }
        self.render_titlebar();
        let mut message = session::Message::new(session::MSG_ACTIVE);
        message.flags = if active { session::FLAG_ACTIVE } else { 0 };
        self.post(message);
    }

    fn post(&mut self, mut message: session::Message) {
        let content = self.window.local_content_rect().size;
        message.width = content.width;
        message.height = content.height;
        bridge::post(self.connection, &message);
    }

    pub(crate) fn menu_command(&mut self, command: u32) {
        let mut message = session::Message::new(session::MSG_MENU_COMMAND);
        message.command = command;
        self.post(message);
    }

    /// Chrome and all: the window's first paint, and after a resize. The
    /// content keeps the app's last frame (clipped or padded) until the app
    /// sends one for the new size.
    fn render_full(&mut self) {
        let Some(id) = self.id else { return; };
        let size = self.window.frame().size;
        let needed = size.width as usize * size.height as usize;
        if needed == 0 || needed > self.backing.len() {
            return;
        }
        let local = Window::new(Rect::new(0, 0, size.width, size.height), self.style);
        let name = self.name();
        let active = self.active;
        {
            let pixels = &mut self.backing.as_mut_slice()[..needed];
            let Some(mut surface) = Surface::new(pixels, size.width, size.height, size.width) else { return; };
            surface.fill(Color::from_xrgb8888(scene::TRANSPARENT_KEY));
            aqua::UIDrawWindow(&mut surface, text::shared(), &local, name, active);
            let content = local.local_content_rect();
            surface.fill_rect(content, self.style.background);
            local.draw_content_mask(&mut surface, Color::from_xrgb8888(scene::TRANSPARENT_KEY), true);
        }
        windowserver::Render_Window(id, &self.backing.as_mut_slice()[..needed], size.width, size.height, size.width);
        self.needs_redraw = true;
    }

    fn render_titlebar(&mut self) {
        let Some(id) = self.id else { return; };
        let size = self.window.frame().size;
        let needed = size.width as usize * size.height as usize;
        if needed == 0 || needed > self.backing.len() {
            return;
        }
        let local = Window::new(Rect::new(0, 0, size.width, size.height), self.style);
        let name = self.name();
        let active = self.active;
        {
            let pixels = &mut self.backing.as_mut_slice()[..needed];
            let Some(mut surface) = Surface::new(pixels, size.width, size.height, size.width) else { return; };
            aqua::UIDrawTitlebar(&mut surface, text::shared(), &local, name, active);
        }
        windowserver::Render_Window(id, &self.backing.as_mut_slice()[..needed], size.width, size.height, size.width);
    }

    /// Where the window goes to show content `width` x `height` during a
    /// live resize: the edges not being dragged stay where they are.
    fn frame_for_content(&self, width: u32, height: u32) -> Rect {
        let target = self.window.frame();
        let height = height + self.style.titlebar_height;
        let edges = self.window.resizing_edges();
        let x = if edges.left { target.origin.x + target.size.width as i32 - width as i32 } else { target.origin.x };
        let y = if edges.top { target.origin.y + target.size.height as i32 - height as i32 } else { target.origin.y };
        Rect::new(x, y, width, height)
    }

    /// A live resize step on screen: WindowServer's window takes `frame`
    /// together with the app's frame for it, which `update` just put in the
    /// backing store, so only the chrome around that is drawn: the titlebar
    /// band and the content's corners and edges.
    fn show_resized(&mut self, frame: Rect) {
        let done = frame.size == self.window.frame().size;
        self.resize_since = if done { None } else { Some(crate::desktop::now_us()) };
        let Some(id) = self.id else { return; };
        let size = frame.size;
        let needed = size.width as usize * size.height as usize;
        if needed == 0 || needed > self.backing.len() {
            return;
        }
        let local = Window::new(Rect::new(0, 0, size.width, size.height), self.style);
        let name = self.name();
        let active = self.active;
        {
            let pixels = &mut self.backing.as_mut_slice()[..needed];
            let Some(mut surface) = Surface::new(pixels, size.width, size.height, size.width) else { return; };
            let titlebar = Rect::new(0, 0, size.width, self.style.titlebar_height.min(size.height));
            surface.fill_rect(titlebar, Color::from_xrgb8888(scene::TRANSPARENT_KEY));
            aqua::UIDrawTitlebar(&mut surface, text::shared(), &local, name, active);
            local.draw_content_mask(&mut surface, Color::from_xrgb8888(scene::TRANSPARENT_KEY), true);
        }
        // Refused (no memory for the bigger window): it stays as it was on
        // screen; a smaller size later can still be shown.
        if !windowserver::Resize_Window(id, frame) {
            return;
        }
        windowserver::Render_Window(id, &self.backing.as_mut_slice()[..needed], size.width, size.height, size.width);
    }

    /// One desktop pass: pick up what the app sent (a frame, its pointer,
    /// whether it animates or wants to quit), ask for a redraw after a
    /// resize and for the next animation frame. Returns whether the app
    /// asked to close.
    pub(crate) fn update(&mut self, over_content: bool, frame_due: bool) -> bool {
        if self.is_quitting() {
            return false;
        }

        let mut close = false;
        if let Some(state) = bridge::state(self.connection, &mut self.sequence) {
            self.state = state;
            close = state.action == session::ACTION_CLOSE;
            if over_content {
                windowserver::Set_Cursor_Kind(cursor_kind(state.cursor));
            }
        }

        if let Some(id) = self.id {
            // During a live resize a frame shows at the size the app drew it
            // for (the latest submit's), a step behind the pointer.
            let resizing = self.resize_since.is_some();
            let frame = if resizing { self.frame_for_content(self.state.width, self.state.height) } else { self.window.frame() };
            let size = frame.size;
            let content = Window::new(Rect::new(0, 0, size.width, size.height), self.style).local_content_rect();
            let needed = size.width as usize * size.height as usize;
            if needed <= self.backing.len() && content.size.width > 0 && content.size.height > 0 {
                let offset = content.origin.y as usize * size.width as usize;
                let taken = bridge::take_frame(
                    self.connection,
                    &mut self.backing.as_mut_slice()[offset..needed],
                    size.width,
                    content.size.width,
                    content.size.height,
                );
                if let Some((width, height)) = taken.filter(|_| resizing) {
                    if width == content.size.width && height == content.size.height {
                        self.show_resized(frame);
                    } else {
                        // A newer submit raced the state read: ask again.
                        self.needs_redraw = true;
                    }
                } else if taken.is_some() {
                    let local = Window::new(Rect::new(0, 0, size.width, size.height), self.style);
                    {
                        let pixels = &mut self.backing.as_mut_slice()[..needed];
                        if let Some(mut surface) = Surface::new(pixels, size.width, size.height, size.width) {
                            local.draw_content_mask(&mut surface, Color::from_xrgb8888(scene::TRANSPARENT_KEY), true);
                        }
                    }
                    windowserver::Render_Window(id, &self.backing.as_mut_slice()[..needed], size.width, size.height, size.width);
                }
            }

            let target = self.window.local_content_rect().size;
            if self.needs_redraw || self.told != target {
                self.needs_redraw = false;
                self.told = target;
                self.post(session::Message::new(session::MSG_REDRAW));
            }

            // The app is slow (or will not draw this size): the window
            // follows the pointer anyway, with its last content.
            if self.resize_since.is_some_and(|since| crate::desktop::now_us().saturating_sub(since) > RESIZE_WAIT_US) {
                self.resize_since = None;
                if windowserver::Resize_Window(id, self.window.frame()) {
                    self.render_full();
                }
            }
        }

        if frame_due && self.state.flags & session::SUBMIT_ANIMATING != 0 && self.id.is_some() {
            self.post(session::Message::new(session::MSG_FRAME));
        }
        close
    }

    /// An input event for this window (screen coordinates): the chrome takes
    /// what is its (drag, resize, the traffic lights), the rest goes to the
    /// app in content coordinates. Returns what the traffic lights asked for.
    pub(crate) fn event(&mut self, event: Event, screen: Size, menubar_height: u32) -> Outcome {
        let mut outcome = Outcome::default();
        if self.id.is_none() {
            return outcome;
        }

        if let Event::PointerDown { position, button: PointerButton::Primary } = event {
            match traffic_light_at(&self.window, position) {
                Some(0) => {
                    outcome.quit = true;
                    return outcome;
                }
                Some(1) => {
                    outcome.hide = true;
                    return outcome;
                }
                _ => {}
            }
        }

        let old = self.window.frame();
        let suppress = self.window.is_dragging() || self.window.is_resizing();
        self.window.handle_event(event, screen);
        // Never under the menu bar.
        let frame = self.window.frame();
        if frame.origin.y < menubar_height as i32 {
            self.window.set_origin(Point::new(frame.origin.x, menubar_height as i32));
        }

        let chrome = chrome_cursor_kind(&self.window, event);
        let over_content = chrome == Some(CursorKind::Arrow);
        if let Some(kind) = chrome {
            windowserver::Set_Cursor_Kind(if over_content { cursor_kind(self.state.cursor) } else { kind });
        }
        outcome.over_content = over_content;

        let frame = self.window.frame();
        if frame != old {
            if let Some(id) = self.id {
                if frame.size != old.size {
                    // A live resize step: the app is asked for its content
                    // at the new size now, and the window takes that size on
                    // screen when the frame comes back (`update`), content
                    // and all. Resizing first showed an empty window for a
                    // frame each step, and composited every step twice.
                    let content = self.window.local_content_rect().size;
                    if self.told != content {
                        self.told = content;
                        self.post(session::Message::new(session::MSG_REDRAW));
                    }
                    if self.resize_since.is_none() {
                        self.resize_since = Some(crate::desktop::now_us());
                    }
                } else {
                    windowserver::Move_Window(id, frame);
                }
            }
        }

        if let Some(message) = content_message(event, self.window, suppress) {
            self.post(message);
        }
        outcome
    }
}

impl Drop for RemoteApp {
    fn drop(&mut self) {
        self.hide();
        bridge::release(self.connection);
    }
}

/// What a window's chrome did with an event.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Outcome {
    /// Its close button: quit the app.
    pub quit: bool,
    /// Its minimize button.
    pub hide: bool,
    /// The pointer is over the content (the app's cursor shows).
    pub over_content: bool,
}

/// A name inside a `RemoteApp`, as `'static`. Sound because every
/// `RemoteApp` lives in the desktop's static slot table, and the desktop
/// points the menu bar elsewhere before a slot is dropped.
fn static_str(text: &str) -> &'static str {
    unsafe { &*(text as *const str) }
}

fn cursor_kind(raw: u32) -> CursorKind {
    match raw {
        session::CURSOR_HAND => CursorKind::Hand,
        session::CURSOR_NOT_ALLOWED => CursorKind::NotAllowed,
        _ => CursorKind::Arrow,
    }
}

/// Which traffic light, if any, is under `position` (screen coordinates):
/// 0 close, 1 minimize, 2 zoom.
fn traffic_light_at(window: &Window, position: Point) -> Option<usize> {
    let frame = window.frame();
    let local = Point::new(position.x - frame.origin.x, position.y - frame.origin.y);
    let center_y = (window.style().titlebar_height / 2) as i32;
    let reach = pt(aqua::TRAFFIC_LIGHT_RADIUS + 3) as i32;

    aqua::TRAFFIC_LIGHT_X.iter().position(|&x| {
        let dx = local.x - pt(x) as i32;
        let dy = local.y - center_y;
        dx * dx + dy * dy <= reach * reach
    })
}

fn resize_cursor_kind(edges: ui_window::ResizeEdges) -> Option<CursorKind> {
    if edges.is_none() {
        return None;
    }
    let horizontal = edges.left || edges.right;
    let vertical = edges.top || edges.bottom;
    Some(if horizontal && vertical {
        let ne_sw = (edges.top && edges.right) || (edges.bottom && edges.left);
        if ne_sw { CursorKind::ResizeDiagonalNeSw } else { CursorKind::ResizeDiagonalNwSe }
    } else if horizontal {
        CursorKind::ResizeHorizontal
    } else {
        CursorKind::ResizeVertical
    })
}

/// What the chrome wants the pointer to show. Plain `Arrow` means "over
/// content": the app's own cursor then gets the last word.
fn chrome_cursor_kind(window: &Window, event: Event) -> Option<CursorKind> {
    if window.is_dragging() {
        return Some(CursorKind::Move);
    }
    if let Some(kind) = resize_cursor_kind(window.resizing_edges()) {
        return Some(kind);
    }
    let Event::PointerMoved { position } = event else { return None; };
    if window.titlebar_rect().contains(position) {
        Some(CursorKind::Hand)
    } else if let Some(kind) = resize_cursor_kind(window.resize_edges_at(position)) {
        Some(kind)
    } else {
        Some(CursorKind::Arrow)
    }
}

fn button_code(button: PointerButton) -> u32 {
    match button {
        PointerButton::Primary => session::BUTTON_PRIMARY,
        PointerButton::Secondary => session::BUTTON_SECONDARY,
        PointerButton::Middle => session::BUTTON_MIDDLE,
    }
}

/// `event` as a message for the app, in content coordinates, or `None` when
/// it is the chrome's (titlebar, edges) or must not reach the app (a drag
/// in progress, a wheel turned outside the content).
fn content_message(event: Event, window: Window, suppress: bool) -> Option<session::Message> {
    let content = window.content_rect();
    let mut message = session::Message::new(session::MSG_EVENT);

    let (kind, position) = match event {
        Event::PointerDown { position, .. } if window.titlebar_rect().contains(position) => return None,
        Event::PointerDown { position, .. } if !window.resize_edges_at(position).is_none() => return None,
        Event::PointerMoved { .. } | Event::PointerUp { .. } if suppress => return None,
        Event::PointerMoved { position } => (session::EVENT_POINTER_MOVED, Some(position)),
        Event::PointerDown { position, button } => {
            message.button = button_code(button);
            (session::EVENT_POINTER_DOWN, Some(position))
        }
        Event::PointerUp { position, button } => {
            message.button = button_code(button);
            (session::EVENT_POINTER_UP, Some(position))
        }
        Event::PointerLeft => (session::EVENT_POINTER_LEFT, None),
        Event::KeyDown { code, character } => {
            message.button = code;
            message.character = character.map_or(0, |character| character as u32);
            (session::EVENT_KEY_DOWN, None)
        }
        Event::Scroll { position, delta } => {
            if suppress || !content.contains(position) {
                return None;
            }
            message.delta = delta;
            (session::EVENT_SCROLL, Some(position))
        }
    };
    if let Some(position) = position {
        message.x = position.x - content.origin.x;
        message.y = position.y - content.origin.y;
    }
    message.event = kind;
    Some(message)
}
