//! The NXU desktop: the wallpaper, the menu bar, the Dock and the windows of
//! every app process, with none of the apps linked in.
//!
//! Apps live in `/Applications/<Name>.app` and run as processes of their
//! own; they connect through the kernel's UI session bridge (see
//! `bridge.rs`) and the desktop keeps a `RemoteApp` per connection: the
//! window's chrome and backing store, where it is, whether it is key. The
//! Dock is a process too (`dockhost.rs`) and launches the apps, including
//! the ones chosen from the system menu. The desktop keeps the stacking
//! order, decides which window an event belongs to, and owns the menu bar
//! (see `menubar.rs`), whose titles follow the frontmost app.
//!
//! Everything big lives in statics or on the kernel heap, not on this stack:
//! the arm64 boot stack this runs on is 16 KiB (see `storage.rs`).

use ui_abi::{HostV5, Status};
use ui_core::{scale::pt, Event, Point, Size};
use ui_platform::InteractiveHostV5;
use ui_render::Canvas;
use ui_session as session;

use crate::bridge;
use crate::dockhost::DockHost;
use crate::menubar::{Action, Item, Items, MenuBar, TitleKind};
use crate::remote::RemoteApp;
use crate::storage::StaticCell;
use crate::windowserver::{self, CursorKind, WindowId};
use crate::{clock, scene, text};

unsafe extern "C" {
    /// NXU's monotonic microsecond counter: the same kernel symbol
    /// WindowServer's own NXU bridge uses for frame pacing and its cursor
    /// animation (one final kernel image, so same-build coupling).
    fn timer_get_microseconds() -> u64;
}

pub(crate) fn now_us() -> u64 {
    unsafe { timer_get_microseconds() }
}

/// How many app windows the desktop keeps (the bridge has 8 connections,
/// one of them the Dock's).
pub(crate) const MAX_APPS: usize = 8;

/// The system menu's About item opens this bundle.
const ABOUT_BUNDLE: &str = "/System/Library/CoreServices/About sevOS.app";

/// Animation frames are asked for at most this often.
const FRAME_INTERVAL_US: u64 = 16_667;

static MENUBAR: StaticCell<MenuBar> = StaticCell::new(MenuBar::new());
static APPS: StaticCell<[Option<RemoteApp>; MAX_APPS]> = StaticCell::new([const { None }; MAX_APPS]);
static DOCK: StaticCell<Option<DockHost>> = StaticCell::new(None);

struct Desktop<'h> {
    host: InteractiveHostV5<'h>,
    apps: &'static mut [Option<RemoteApp>; MAX_APPS],
    dock: &'static mut Option<DockHost>,
    surface: Size,
    desktop_id: WindowId,
    /// Visible windows, back to front: the last one is the key window.
    order: [usize; MAX_APPS],
    order_len: usize,
    /// The window that took the last press, until the release: a drag keeps
    /// going to it wherever the pointer wanders.
    captured: Option<usize>,
    /// The window the pointer was last over, to tell it when it leaves.
    hovered: Option<usize>,
    /// The pointer is over the content of `hovered` (its cursor shows).
    over_content: bool,
    menubar: &'static mut MenuBar,
    /// The app whose name and menus the menu bar shows.
    menubar_app: Option<usize>,
    clock_minute: Option<u64>,
    last_frame_us: u64,
    last_liveness_us: u64,
}

impl Desktop<'_> {
    fn app(&self, slot: usize) -> Option<&RemoteApp> {
        self.apps.get(slot).and_then(Option::as_ref)
    }

    fn app_mut(&mut self, slot: usize) -> Option<&mut RemoteApp> {
        self.apps.get_mut(slot).and_then(Option::as_mut)
    }

    fn key_window(&self) -> Option<usize> {
        (self.order_len > 0).then(|| self.order[self.order_len - 1])
    }

    /// The frontmost visible window whose frame holds `point`.
    fn window_at(&self, point: Point) -> Option<usize> {
        self.order[..self.order_len]
            .iter()
            .rev()
            .copied()
            .find(|&slot| self.app(slot).and_then(RemoteApp::window).is_some_and(|window| window.frame().contains(point)))
    }

    fn remove_from_order(&mut self, slot: usize) {
        if let Some(position) = self.order[..self.order_len].iter().position(|&entry| entry == slot) {
            self.order.copy_within(position + 1..self.order_len, position);
            self.order_len -= 1;
        }
    }

    /// Make `slot` the key window: raise it, turn the old key window's
    /// titlebar grey and this one's back to colour, and give it the menu bar.
    fn focus(&mut self, slot: usize) {
        let previous = self.key_window();
        if previous != Some(slot) {
            self.remove_from_order(slot);
            self.order[self.order_len] = slot;
            self.order_len += 1;
            if let Some(previous) = previous.and_then(|previous| self.app_mut(previous)) {
                previous.set_active(false);
            }
        }
        if let Some(id) = self.app(slot).and_then(RemoteApp::window_id) {
            windowserver::Focus_Window(id);
        }
        self.raise_dock();
        if let Some(app) = self.app_mut(slot) {
            app.set_active(true);
        }
        self.sync_menubar();
    }

    fn raise_dock(&mut self) {
        if let Some(dock) = self.dock.as_ref() {
            dock.raise();
        }
    }

    /// Put WindowServer's stacking back to ours, bottom to top: after a
    /// click that the open menu swallowed but WindowServer (which hit-tests
    /// the same click on its own) already raised a window for.
    fn restack(&mut self) {
        for index in 0..self.order_len {
            if let Some(id) = self.app(self.order[index]).and_then(RemoteApp::window_id) {
                windowserver::Focus_Window(id);
            }
        }
        self.raise_dock();
    }

    /// Show `slot` and make it the key window. A window shown for the first
    /// time cascades down-right from the upper left of the desktop.
    fn show(&mut self, slot: usize) {
        let visible = self.order_len as i32;
        let top = MenuBar::height() as i32;
        let size = self.surface;
        let origin = Point::new(
            size.width as i32 / 6 + visible * pt(28) as i32,
            top + (size.height as i32 - top) / 10 + visible * pt(28) as i32,
        );
        let Some(app) = self.app_mut(slot) else { return; };
        let shown = app.show(size, origin);
        if shown {
            self.focus(slot);
        }
    }

    fn hide(&mut self, slot: usize) {
        if let Some(app) = self.app_mut(slot) {
            app.hide();
        }
        self.window_gone(slot);
    }

    fn quit(&mut self, slot: usize) {
        let now = now_us();
        if let Some(app) = self.app_mut(slot) {
            app.quit(now);
        }
        self.window_gone(slot);
    }

    fn window_gone(&mut self, slot: usize) {
        self.remove_from_order(slot);
        if self.captured == Some(slot) {
            self.captured = None;
        }
        if self.hovered == Some(slot) {
            self.hovered = None;
        }
        match self.key_window() {
            Some(next) => self.focus(next),
            None => self.sync_menubar(),
        }
    }

    /// The app's process is gone: drop everything of it. The menu bar lets go
    /// of its names first (they live in the slot).
    fn drop_app(&mut self, slot: usize) {
        if self.menubar.is_open() {
            self.close_menu();
        }
        self.remove_from_order(slot);
        if self.menubar_app == Some(slot) {
            self.menubar_app = None;
            self.menubar.set_app("sevOS", &[]);
        }
        self.apps[slot] = None;
        self.window_gone(slot);
    }

    fn apply_outcome(&mut self, slot: usize, outcome: crate::remote::Outcome) {
        if outcome.quit {
            self.quit(slot);
        } else if outcome.hide {
            self.hide(slot);
        }
    }

    /// Point the menu bar at the key window's app (or the system's own
    /// titles when nothing is on screen) and repaint it.
    fn sync_menubar(&mut self) {
        let app = self.key_window();
        if app != self.menubar_app || app.is_none() {
            self.menubar_app = app;
            let mut titles = [""; session::MENU_MAX];
            let (name, count) = match app.and_then(|slot| self.app(slot)) {
                Some(app) => {
                    let count = app.menu_count();
                    for (index, title) in titles.iter_mut().enumerate().take(count) {
                        *title = app.menu_title(index);
                    }
                    (app.name(), count)
                }
                None => ("sevOS", 0),
            };
            self.menubar.set_app(name, &titles[..count]);
        }
        self.redraw_menubar();
    }

    /// Repaint the bar into the desktop layer and send the layer over:
    /// WindowServer recomposites only the pixels that changed (the bar).
    fn redraw_menubar(&mut self) {
        let mut clock_buffer = [0u8; 32];
        let clock_text: &str = match self.host.get_time() {
            Ok(Some(unix_seconds)) => clock::format(unix_seconds, &mut clock_buffer),
            _ => "",
        };

        let text = text::shared();
        let menubar = &mut self.menubar;
        let layer = unsafe {
            self.host.with_surface(|surface| {
                menubar.draw(surface, text, clock_text);
                (surface.pixels_mut().as_ptr(), surface.width(), surface.height(), surface.stride())
            })
        };
        if let Ok((pixels, width, height, stride)) = layer {
            let pixels = unsafe { core::slice::from_raw_parts(pixels, height as usize * stride as usize) };
            windowserver::Render_Window(self.desktop_id, pixels, width, height, stride);
        }
    }

    fn running(&self) -> impl Iterator<Item = (usize, &RemoteApp)> {
        self.apps.iter().enumerate().filter_map(|(slot, app)| app.as_ref().filter(|app| !app.is_quitting()).map(|app| (slot, app)))
    }

    /// What drops down under title `index`, decided now (enabled states and
    /// check marks are current as of the moment the menu opens).
    fn menu_items(&self, index: usize) -> Items {
        let mut items = Items::new();
        let key = self.menubar_app;
        let any_hidden = self.running().any(|(_, app)| !app.is_visible());

        match self.menubar.title_kind(index) {
            Some(TitleKind::System) => {
                items.push(Item::new("About sevOS", Action::About).enabled(self.dock.is_some()));
            }
            Some(TitleKind::Application) => match key.and_then(|slot| self.app(slot).map(|app| (slot, app))) {
                Some((slot, app)) => {
                    let name = app.name();
                    let others_visible = self.order[..self.order_len].iter().any(|&other| other != slot);
                    items.push(Item::joined("Hide ", name, Action::Hide(slot)));
                    items.push(Item::new("Hide Others", Action::HideOthers).enabled(others_visible));
                    items.push(Item::new("Show All", Action::ShowAll).enabled(any_hidden));
                    items.push(Item::SEPARATOR);
                    items.push(Item::joined("Quit ", name, Action::Quit(slot)));
                }
                None => {
                    items.push(Item::new("About sevOS", Action::About).enabled(self.dock.is_some()));
                    items.push(Item::SEPARATOR);
                    items.push(Item::new("Show All", Action::ShowAll).enabled(any_hidden));
                }
            },
            Some(TitleKind::AppMenu(menu)) => {
                if let Some((slot, app)) = key.and_then(|slot| self.app(slot).map(|app| (slot, app))) {
                    for (item, command, title) in app.menu_items(menu) {
                        if command == session::MENU_SEPARATOR {
                            items.push(Item::SEPARATOR);
                        } else {
                            let (enabled, checked) = app.menu_item_state(item);
                            items.push(Item::new(title, Action::Command(slot, command)).enabled(enabled).checked(checked));
                        }
                    }
                }
            }
            Some(TitleKind::Window) => {
                match key {
                    Some(slot) => {
                        items.push(Item::new("Minimize", Action::Hide(slot)));
                        items.push(Item::new("Close", Action::Quit(slot)));
                    }
                    None => {
                        items.push(Item::new("Minimize", Action::HideOthers).enabled(false));
                        items.push(Item::new("Close", Action::HideOthers).enabled(false));
                    }
                }
                items.push(Item::new("Bring All to Front", Action::ShowAll).enabled(any_hidden || self.order_len > 1));
                items.push(Item::SEPARATOR);
                for (slot, app) in self.running() {
                    items.push(Item::new(app.name(), Action::Focus(slot)).checked(key == Some(slot)));
                }
            }
            None => {}
        }
        items
    }

    fn open_menu(&mut self, index: usize) {
        let items = self.menu_items(index);
        let text = text::shared();
        self.menubar.open(index, items, self.surface, text);
        self.redraw_menubar();
    }

    fn close_menu(&mut self) {
        if self.menubar.is_open() {
            self.menubar.close();
            self.restack();
            self.redraw_menubar();
        }
    }

    fn perform(&mut self, action: Action) {
        match action {
            Action::About => {
                if let Some(dock) = self.dock.as_ref() {
                    dock.launch(ABOUT_BUNDLE);
                }
            }
            Action::Focus(slot) => self.show(slot),
            Action::Hide(slot) => self.hide(slot),
            Action::HideOthers => {
                if let Some(key) = self.key_window() {
                    for slot in 0..MAX_APPS {
                        if slot != key && self.app(slot).is_some_and(RemoteApp::is_visible) {
                            if let Some(app) = self.app_mut(slot) {
                                app.hide();
                            }
                            self.remove_from_order(slot);
                        }
                    }
                    self.focus(key);
                }
            }
            Action::ShowAll => {
                let key = self.key_window();
                for slot in 0..MAX_APPS {
                    if self.app(slot).is_some_and(|app| !app.is_quitting()) && Some(slot) != key {
                        self.show(slot);
                    }
                }
                if let Some(key) = key {
                    self.focus(key);
                }
            }
            Action::Quit(slot) => self.quit(slot),
            Action::Command(slot, command) => {
                if let Some(app) = self.app_mut(slot) {
                    app.menu_command(command);
                }
            }
        }
    }

    /// An event while a menu is open: it all goes to the menu bar.
    fn menu_event(&mut self, event: Event) {
        match event {
            Event::PointerMoved { position } => {
                windowserver::Set_Cursor_Kind(CursorKind::Arrow);
                // Sliding along the bar switches menus, as on macOS.
                if let Some(title) = self.menubar.title_at(position) {
                    if Some(title) != self.menubar.open_title() {
                        self.open_menu(title);
                    }
                    return;
                }
                self.menubar.hover(position, text::shared());
            }
            Event::PointerDown { position, .. } => {
                if let Some(title) = self.menubar.title_at(position) {
                    if Some(title) == self.menubar.open_title() {
                        self.close_menu();
                    } else {
                        self.open_menu(title);
                    }
                } else if !self.menubar.menu_contains(position) {
                    self.close_menu();
                }
            }
            Event::PointerUp { position, .. } => {
                // Press on a title, drag down, release on an item: chosen.
                // A release anywhere else leaves the menu open.
                if self.menubar.menu_contains(position) {
                    self.menubar.hover(position, text::shared());
                    if let Some(action) = self.menubar.highlighted_action() {
                        self.close_menu();
                        self.perform(action);
                    }
                }
            }
            Event::KeyDown { code, .. } => match code {
                ui_core::key::ESCAPE => self.close_menu(),
                ui_core::key::UP => self.menubar.step(false, text::shared()),
                ui_core::key::DOWN => self.menubar.step(true, text::shared()),
                ui_core::key::LEFT | ui_core::key::RIGHT => {
                    if let Some(open) = self.menubar.open_title() {
                        let count = (0..crate::menubar::MAX_TITLES).take_while(|&index| self.menubar.title_kind(index).is_some()).count();
                        let next = if code == ui_core::key::RIGHT { (open + 1) % count } else { (open + count - 1) % count };
                        self.open_menu(next);
                    }
                }
                ui_core::key::ENTER => {
                    if let Some(action) = self.menubar.highlighted_action() {
                        self.close_menu();
                        self.perform(action);
                    }
                }
                _ => {}
            },
            Event::PointerLeft | Event::Scroll { .. } => {}
        }
    }

    /// Hand `event` to window `slot` and act on what its chrome says.
    fn deliver(&mut self, slot: usize, event: Event) {
        let surface = self.surface;
        let Some(app) = self.app_mut(slot) else { return; };
        let outcome = app.event(event, surface, MenuBar::height());
        self.over_content = outcome.over_content;
        self.apply_outcome(slot, outcome);
    }

    fn handle(&mut self, event: Event) {
        if self.menubar.is_open() {
            self.menu_event(event);
            return;
        }

        let captured = self.captured.is_some();
        if let Some(dock) = self.dock.as_mut() {
            if dock.event(event, captured) {
                if let (Event::PointerMoved { .. }, Some(previous)) = (event, self.hovered.take()) {
                    self.deliver(previous, Event::PointerLeft);
                }
                return;
            }
        }

        match event {
            Event::PointerDown { position, .. } => {
                if let Some(title) = self.menubar.title_at(position) {
                    self.open_menu(title);
                    return;
                }
                match self.window_at(position) {
                    Some(slot) => {
                        if self.key_window() != Some(slot) {
                            self.focus(slot);
                        }
                        self.captured = Some(slot);
                        self.deliver(slot, event);
                    }
                    None => {
                        windowserver::Set_Cursor_Kind(CursorKind::Arrow);
                    }
                }
            }
            Event::PointerMoved { position } => {
                if let Some(slot) = self.captured {
                    self.deliver(slot, event);
                    return;
                }
                let under = self.window_at(position);
                if self.hovered != under {
                    if let Some(previous) = self.hovered {
                        self.deliver(previous, Event::PointerLeft);
                    }
                    self.hovered = under;
                }
                match under {
                    Some(slot) => self.deliver(slot, event),
                    None => {
                        self.over_content = false;
                        windowserver::Set_Cursor_Kind(CursorKind::Arrow);
                    }
                }
            }
            Event::PointerUp { position, .. } => {
                let target = self.captured.take().or_else(|| self.window_at(position));
                if let Some(slot) = target {
                    self.deliver(slot, event);
                }
            }
            Event::Scroll { position, .. } => {
                if let Some(slot) = self.window_at(position) {
                    self.deliver(slot, event);
                }
            }
            Event::KeyDown { .. } => {
                if let Some(slot) = self.key_window() {
                    self.deliver(slot, event);
                }
            }
            Event::PointerLeft => {
                if let Some(slot) = self.hovered.take() {
                    self.deliver(slot, event);
                }
            }
        }
    }

    /// New connections: an app's window comes up in front; a Dock takes
    /// its place along the bottom.
    fn accept(&mut self) {
        while let Some(connection) = bridge::accept() {
            let Some(info) = bridge::info(connection) else {
                bridge::release(connection);
                continue;
            };

            if info.kind == session::KIND_DOCK {
                *self.dock = DockHost::new(connection, self.surface);
                if self.dock.is_none() {
                    bridge::release(connection);
                }
                continue;
            }

            let Some(slot) = self.apps.iter().position(Option::is_none) else {
                bridge::release(connection);
                continue;
            };
            match RemoteApp::new(connection) {
                Some(app) => {
                    self.apps[slot] = Some(app);
                    self.show(slot);
                }
                None => bridge::release(connection),
            }
        }

        // An app asked to open a bundle (Voyager opening an app): the Dock
        // starts every app, so it goes there.
        let mut path = [0u8; session::PATH_MAX];
        while bridge::take_launch(&mut path) {
            if let Some(dock) = self.dock.as_ref() {
                dock.launch(session::get_str(&path));
            }
        }

        // The Dock asked for an app to come forward (its icon was clicked).
        while let Some(pid) = bridge::take_activation() {
            let slot = self.running().find(|(_, app)| app.pid() == pid).map(|(slot, _)| slot);
            if let Some(slot) = slot {
                self.show(slot);
                self.focus(slot);
            }
        }
    }

    fn tick(&mut self) {
        let now = now_us();

        let minute = self.host.get_time().ok().flatten().map(|seconds| seconds / 60);
        if minute != self.clock_minute {
            self.clock_minute = minute;
            self.redraw_menubar();
        }

        self.accept();

        // Apps that exited (or crashed) leave; checking the process table is
        // not free, so it is done a few times a second, not every pass.
        if now.saturating_sub(self.last_liveness_us) > 250_000 {
            self.last_liveness_us = now;
            for slot in 0..MAX_APPS {
                if self.app_mut(slot).is_some_and(|app| app.finished(now)) {
                    self.drop_app(slot);
                }
            }
            if self.dock.as_ref().is_some_and(|dock| !dock.alive()) {
                *self.dock = None;
            }
        }

        let frame_due = now.saturating_sub(self.last_frame_us) >= FRAME_INTERVAL_US;
        if frame_due {
            self.last_frame_us = now;
        }
        for slot in 0..MAX_APPS {
            let over_content = self.over_content && self.hovered == Some(slot);
            let Some(app) = self.app_mut(slot) else { continue; };
            if app.update(over_content, frame_due) {
                self.quit(slot);
            }
        }

        if self.dock.is_some() {
            let surface = self.surface;
            let dock = &mut self.dock;
            let _ = unsafe {
                self.host.with_surface(|layer| {
                    let stride = layer.stride();
                    if let Some(dock) = dock.as_mut() {
                        dock.update(surface, layer.pixels(), stride);
                    }
                })
            };
        }
    }
}

/// Run the desktop. Returns only on a failure to set it up: apps come and go
/// as processes, and with none running the desktop, its menu bar and the
/// Dock stay.
pub(crate) unsafe fn run(host_abi: &HostV5) -> Result<(), Status> {
    let mut host = InteractiveHostV5::connect(host_abi)?;
    let surface = unsafe { host.with_surface(|surface| surface.size()) }?;
    windowserver::Set_Shadow_Scale(ui_core::scale::permille());

    let desktop_id = windowserver::Create_Background_Window(ui_core::Rect::new(0, 0, surface.width, surface.height))
        .ok_or(Status::NoSurface)?;

    // The wallpaper is painted once: from here on only the menu bar strip
    // of this layer ever changes.
    unsafe { host.with_surface(|surface| scene::UIDrawWallpaper(surface)) }?;

    let mut desktop = Desktop {
        host,
        apps: unsafe { APPS.get_mut() },
        dock: unsafe { DOCK.get_mut() },
        surface,
        desktop_id,
        order: [0; MAX_APPS],
        order_len: 0,
        captured: None,
        hovered: None,
        over_content: false,
        // A static, like everything big here (see the module comment).
        menubar: unsafe { MENUBAR.get_mut() },
        menubar_app: None,
        clock_minute: None,
        last_frame_us: 0,
        last_liveness_us: 0,
    };

    desktop.sync_menubar();

    crate::cursor::install_system_cursors();
    let pointer = Point::new((surface.width / 2) as i32, (surface.height / 2) as i32);
    windowserver::Pointer_Move(pointer.x, pointer.y);
    windowserver::Present();

    // Only now can apps (and the Dock) connect: their windows have a desktop.
    bridge::session_begin(ui_core::scale::permille(), surface.width, surface.height, MenuBar::height());

    loop {
        // Moves are coalesced: only where the pointer ended up by the time
        // the queue is empty is handled (a move is still handled before any
        // other event, so a press lands where the pointer was). A mouse
        // reports ~125 times a second and every move of a drag or a live
        // resize redraws a window; handling each one before presenting
        // anything left a resize seconds behind the pointer. The cursor
        // itself follows every packet: the host moves it as they arrive.
        let mut pending_move = None;
        while let Some(event) = desktop.host.poll_event()? {
            if let Event::PointerMoved { .. } = event {
                pending_move = Some(event);
                continue;
            }
            if let Some(moved) = pending_move.take() {
                desktop.handle(moved);
            }
            desktop.handle(event);
        }
        if let Some(moved) = pending_move {
            desktop.handle(moved);
        }

        desktop.tick();

        // Always present: the cursor's own animation advances only here.
        // `false` is not a failure worth ending the session over: it is
        // what WindowServer answers when nothing was damaged.
        let _ = windowserver::Present();

        core::hint::spin_loop();
    }
}
