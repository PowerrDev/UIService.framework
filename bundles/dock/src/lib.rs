#![no_std]

//! Dock.app: the row of apps along the bottom of the screen.
//!
//! bootd starts it at boot (`com.nxu.dock`); it waits for the desktop (the
//! login screen comes first), reads `/System/Library/Preferences/
//! com.nxu.dock.plist` for the apps to keep in it, and each app's
//! `Contents/Info.plist` for its name, executable and icon. Clicking an app
//! starts `Contents/SevOS/<executable>` or, if it runs, brings it forward.
//! The desktop asks the Dock to open apps too (About sevOS from the system
//! menu): the Dock starts every app, so it always knows which run.
//!
//! The Dock draws its icons and dots over transparency; the desktop puts
//! the translucent panel under them and the name bubble over them (see
//! UIService's `ui-service-nxu/src/dockhost.rs`).

use dock::image::{decode_png, decoded_size, icns_best_png, scale_to_argb};
use dock::layout::{Layout, Metrics, Slot, MAX_SLOTS};
use dock::plist::Plist;
use dock::render::{draw, draw_trash, TileView};
use ui_app_nxu::connection::{self, Connection, Received};
use ui_app_nxu::sys;
use ui_session::{self as session, Connect, Submit};

const CONFIG: &str = "/disk/System/Library/Preferences/com.nxu.dock.plist";
/// The system volume is mounted here; paths in plists leave it out.
const VOLUME: &str = "/disk";

const MAX_APPS: usize = 12;
/// Apps that are not kept in the Dock but ran recently, shown after them.
const MAX_RECENTS: usize = 3;
const DEFAULT_TILE_POINTS: u32 = 48;
/// How long the Dock sleeps waiting for input before it checks its apps.
const CHECK_INTERVAL_MS: u32 = 250;

#[derive(Clone, Copy)]
struct Text<const N: usize> {
    bytes: [u8; N],
    length: usize,
}

impl<const N: usize> Text<N> {
    const EMPTY: Self = Self { bytes: [0; N], length: 0 };

    fn from(text: &str) -> Self {
        let mut value = Self::EMPTY;
        value.push(text);
        value
    }

    fn push(&mut self, text: &str) -> bool {
        let end = self.length + text.len();
        if end > N {
            return false;
        }
        self.bytes[self.length..end].copy_from_slice(text.as_bytes());
        self.length = end;
        true
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.length]).unwrap_or("")
    }
}

#[derive(Clone, Copy)]
struct App {
    /// As the user sees it: `/Applications/Voyager.app`.
    bundle: Text<128>,
    name: Text<48>,
    executable: Text<256>,
    icon: Option<&'static [u32]>,
    pid: u32,
    /// In the Dock's configuration (always shown), or a recent one.
    kept: bool,
    /// `LSUIElement`: started by the Dock when asked, never shown in it.
    agent: bool,
    /// When it last started, to order the recent ones.
    started: u64,
}

struct Dock {
    connection: Connection,
    tile: u32,
    show_recents: bool,
    show_trash: bool,
    apps: [Option<App>; MAX_APPS],
    trash: &'static mut [u32],
    frame: &'static mut [u32],
    /// What is where, and which app each tile is.
    layout: Layout,
    tiles: [usize; MAX_SLOTS],
    hovered: Option<usize>,
    pressed: Option<usize>,
    /// Where the pointer last was over the Dock, to find the tile under it
    /// again when the row changes.
    pointer: Option<(i32, i32)>,
    dirty: bool,
}

fn log(parts: &[&str]) {
    let mut line: [&str; 8] = [""; 8];
    line[0] = "dock: ";
    let count = parts.len().min(7);
    line[1..=count].copy_from_slice(&parts[..count]);
    sys::log_line(&line[..=count]);
}

/// `/Applications/X.app` -> `/disk/Applications/X.app`, then `suffix`.
fn on_volume(path: &str, suffix: &str) -> Text<256> {
    let mut full = Text::EMPTY;
    if !path.starts_with("/disk/") {
        full.push(VOLUME);
    }
    full.push(path);
    full.push(suffix);
    full
}

/// `/Applications/Voyager.app` -> `Voyager`.
fn bundle_stem(path: &str) -> &str {
    let name = path.trim_end_matches('/').rsplit('/').next().unwrap_or(path);
    name.strip_suffix(".app").unwrap_or(name)
}

/// An app's name, executable and icon from its bundle.
fn load_app(bundle: &str, tile: u32) -> Option<App> {
    let stem = bundle_stem(bundle);
    let mut app = App {
        bundle: Text::from(bundle),
        name: Text::from(stem),
        executable: on_volume(bundle, "/Contents/SevOS/"),
        icon: None,
        pid: 0,
        kept: false,
        agent: false,
        started: 0,
    };
    let mut executable = Text::<64>::from(stem);
    let mut icon_file = Text::<64>::EMPTY;

    let info_path = on_volume(bundle, "/Contents/Info.plist");
    match sys::read_file(info_path.as_str()).and_then(|bytes| core::str::from_utf8(bytes).ok()) {
        Some(text) => match Plist::parse(text) {
            Ok(plist) => {
                let root = plist.root();
                let mut buffer = [0u8; 64];
                let read = |key: &str, buffer: &mut [u8; 64]| -> Option<Text<64>> {
                    let id = plist.lookup(root, key)?;
                    plist.string_into(id, buffer).map(Text::from)
                };
                if let Some(name) = read("CFBundleName", &mut buffer) {
                    app.name = Text::from(name.as_str());
                }
                if let Some(value) = read("CFBundleExecutable", &mut buffer) {
                    executable = value;
                }
                if let Some(value) = read("CFBundleIconFile", &mut buffer) {
                    icon_file = value;
                }
                app.agent = plist.lookup(root, "LSUIElement").and_then(|id| plist.boolean(id)).unwrap_or(false);
            }
            Err(_) => log(&[info_path.as_str(), ": not a property list"]),
        },
        None => log(&[info_path.as_str(), ": missing"]),
    }
    if !app.executable.push(executable.as_str()) {
        return None;
    }
    if stem.is_empty() {
        return None;
    }

    if icon_file.length != 0 {
        let mut icon_path = on_volume(bundle, "/Contents/Resources/");
        icon_path.push(icon_file.as_str());
        if !icon_file.as_str().contains('.') {
            icon_path.push(".icns");
        }
        app.icon = load_icon(icon_path.as_str(), tile);
        if app.icon.is_none() {
            log(&[icon_path.as_str(), ": no usable icon"]);
        }
    }
    Some(app)
}

/// Decode the best image in an .icns file and scale it to the tile.
fn load_icon(path: &str, tile: u32) -> Option<&'static [u32]> {
    let icns = sys::read_file(path)?;
    let png = icns_best_png(icns, tile).ok()?;
    let (raw_size, rgba_size) = decoded_size(png).ok()?;
    let raw = sys::map(raw_size)?;
    let rgba = sys::map(rgba_size)?;
    let header = decode_png(png, raw, rgba);
    let icon_bytes = sys::map(tile as usize * tile as usize * 4)?;
    let icon = unsafe { core::slice::from_raw_parts_mut(icon_bytes.as_mut_ptr() as *mut u32, tile as usize * tile as usize) };
    if let Ok(header) = header {
        scale_to_argb(rgba, header.width, header.height, tile, icon);
    }
    sys::unmap(raw);
    sys::unmap(rgba);
    header.ok().map(|_| &*icon)
}

impl Dock {
    fn app(&self, index: usize) -> Option<&App> {
        self.apps.get(index).and_then(Option::as_ref)
    }

    /// Rebuild the row: kept apps, then (after a divider) running or recent
    /// ones, then the Trash.
    fn relayout(&mut self) {
        let mut slots = [Slot::Divider; MAX_SLOTS];
        let mut count = 0;
        let mut push = |slot: Slot, count: &mut usize| {
            if *count < MAX_SLOTS {
                slots[*count] = slot;
                *count += 1;
            }
        };

        for (index, app) in self.apps.iter().enumerate() {
            if app.as_ref().is_some_and(|app| app.kept) {
                push(Slot::App(index), &mut count);
            }
        }
        let kept = count;
        for (index, app) in self.apps.iter().enumerate() {
            if app.as_ref().is_some_and(|app| !app.kept && !app.agent && (app.pid != 0 || self.show_recents)) {
                if count == kept && kept > 0 {
                    push(Slot::Divider, &mut count);
                }
                push(Slot::App(index), &mut count);
            }
        }
        if self.show_trash {
            if count > 0 {
                push(Slot::Divider, &mut count);
            }
            push(Slot::Trash, &mut count);
        }

        self.layout = Layout::new(Metrics::new(self.tile), &slots[..count]);
        for (index, &(slot, _)) in self.layout.slots().iter().enumerate() {
            self.tiles[index] = match slot {
                Slot::App(app) => app,
                _ => usize::MAX,
            };
        }
        self.hovered = self.pointer.and_then(|(x, y)| self.hit(x, y));
        self.pressed = None;
        self.dirty = true;
    }

    /// The tile (not a divider) at (x, y).
    fn hit(&self, x: i32, y: i32) -> Option<usize> {
        self.layout.hit(x, y).filter(|&slot| self.layout.slots()[slot].0 != Slot::Divider)
    }

    fn render_and_submit(&mut self) {
        let mut submit = Submit::empty();
        submit.panel_radius = self.layout.metrics.radius;

        if self.dirty {
            self.dirty = false;
            let mut views = [TileView { icon: None, running: false, pressed: false }; MAX_APPS];
            for (index, app) in self.apps.iter().enumerate() {
                if let Some(app) = app {
                    views[index] = TileView { icon: app.icon, running: app.pid != 0, pressed: false };
                }
            }
            if let Some(slot) = self.pressed {
                if let Some(&app) = self.tiles.get(slot) {
                    if app < MAX_APPS {
                        views[app].pressed = true;
                    }
                }
            }
            draw(&self.layout, &views, self.trash, self.frame);
            submit.pixels = self.frame.as_ptr() as usize as u64;
            submit.width = self.layout.width;
            submit.height = self.layout.height;
            submit.stride = self.layout.width;
        }

        if let Some(slot) = self.hovered {
            let label = match self.layout.slots()[slot].0 {
                Slot::App(app) => self.app(app).map_or("", |app| app.name.as_str()),
                Slot::Trash => "Trash",
                Slot::Divider => "",
            };
            session::set_str(&mut submit.label, label);
            submit.label_x = self.layout.center(slot);
        }
        self.connection.submit(&submit);
    }

    /// Start the app in slot `index`, or bring it forward if it runs.
    fn open(&mut self, index: usize) {
        let Some(app) = self.apps[index].as_mut() else { return; };
        if app.pid != 0 {
            connection::activate(app.pid);
            return;
        }

        let executable = app.executable;
        let mut c_path = [0u8; 257];
        c_path[..executable.length].copy_from_slice(&executable.bytes[..executable.length]);
        let process_name = Text::<31>::from(&app.name.as_str()[..app.name.length.min(31)]);
        let mut c_name = [0u8; 32];
        c_name[..process_name.length].copy_from_slice(&process_name.bytes[..process_name.length]);

        let pid = unsafe { sys::nxu_spawn(c_path.as_ptr() as *const _, c_name.as_ptr() as *const _) };
        if pid <= 0 {
            log(&[executable.as_str(), ": could not be started"]);
            return;
        }
        app.pid = pid as u32;
        app.started = sys::uptime_us();
        let mut digits = [0u8; 20];
        log(&["started ", app.name.as_str(), " (pid ", sys::decimal(pid as u64, &mut digits), ")"]);
        self.relayout();
    }

    /// The desktop asked for `bundle` (the system menu's About sevOS): open
    /// it, adding it as a recent app if the Dock does not keep it.
    fn open_bundle(&mut self, bundle: &str) {
        // Paths from the file system (Voyager) carry the volume's mount point.
        let bundle = bundle.strip_prefix(VOLUME).filter(|rest| rest.starts_with('/')).unwrap_or(bundle);
        let existing = self.apps.iter().position(|app| app.as_ref().is_some_and(|app| app.bundle.as_str() == bundle));
        let index = match existing {
            Some(index) => index,
            None => {
                let Some(app) = load_app(bundle, self.tile) else {
                    log(&[bundle, ": not an app"]);
                    return;
                };
                let free = self.apps.iter().position(Option::is_none).or_else(|| self.oldest_recent());
                let Some(index) = free else { return; };
                self.apps[index] = Some(app);
                index
            }
        };
        self.open(index);
    }

    /// The recent app that ran longest ago and is not running.
    fn oldest_recent(&self) -> Option<usize> {
        self.apps
            .iter()
            .enumerate()
            .filter(|(_, app)| app.as_ref().is_some_and(|app| !app.kept && app.pid == 0))
            .min_by_key(|(_, app)| app.as_ref().map_or(0, |app| app.started))
            .map(|(index, _)| index)
    }

    /// Notice apps that quit (the Dock started them, so it reaps them).
    fn check_apps(&mut self) {
        let mut changed = false;
        for app in self.apps.iter_mut().flatten() {
            if app.pid == 0 {
                continue;
            }
            let mut status = 0u64;
            let result = unsafe { sys::nxu_waitpid(app.pid as u64, &mut status) };
            if result != sys::E_AGAIN {
                let mut digits = [0u8; 20];
                let code = if result < 0 { sys::decimal((-result) as u64, &mut digits) } else { sys::decimal(result as u64, &mut digits) };
                log(&[app.name.as_str(), " quit (waitpid ", if result < 0 { "-" } else { "" }, code, ")"]);
                app.pid = 0;
                changed = true;
            }
        }
        // An agent (About sevOS) is not kept once it quits: it never shows.
        for app in self.apps.iter_mut() {
            if app.as_ref().is_some_and(|app| app.agent && app.pid == 0) {
                *app = None;
            }
        }
        // No more than MAX_RECENTS recent apps once they quit.
        loop {
            let recents = self.apps.iter().flatten().filter(|app| !app.kept && app.pid == 0).count();
            if recents <= MAX_RECENTS || !self.show_recents {
                break;
            }
            if let Some(index) = self.oldest_recent() {
                self.apps[index] = None;
                changed = true;
            }
        }
        if !self.show_recents {
            for app in self.apps.iter_mut() {
                if app.as_ref().is_some_and(|app| !app.kept && app.pid == 0) {
                    *app = None;
                    changed = true;
                }
            }
        }
        if changed {
            self.relayout();
        }
    }

    fn pointer(&mut self, message: &session::Message) {
        let hit = self.hit(message.x, message.y);
        self.pointer = (message.event != session::EVENT_POINTER_LEFT).then_some((message.x, message.y));
        match message.event {
            session::EVENT_POINTER_MOVED => {
                if hit != self.hovered {
                    self.hovered = hit;
                }
            }
            session::EVENT_POINTER_LEFT => {
                self.hovered = None;
                if self.pressed.take().is_some() {
                    self.dirty = true;
                }
            }
            session::EVENT_POINTER_DOWN if message.button == session::BUTTON_PRIMARY => {
                self.hovered = hit;
                self.pressed = hit;
                self.dirty = true;
            }
            session::EVENT_POINTER_UP => {
                let pressed = self.pressed.take();
                self.dirty = true;
                if let Some(slot) = pressed.filter(|&slot| Some(slot) == hit) {
                    let app = self.tiles[slot];
                    if app < MAX_APPS {
                        self.open(app);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Read the configuration: the kept apps, the tile size, the options.
fn configure(apps: &mut [Option<App>; MAX_APPS], tile_points: &mut u32, show_recents: &mut bool, show_trash: &mut bool, scale: u32) {
    let Some(text) = sys::read_file(CONFIG).and_then(|bytes| core::str::from_utf8(bytes).ok()) else {
        log(&[CONFIG, ": missing, the Dock is empty"]);
        return;
    };
    let Ok(plist) = Plist::parse(text) else {
        log(&[CONFIG, ": not a property list"]);
        return;
    };
    let root = plist.root();
    if let Some(size) = plist.lookup(root, "tilesize").and_then(|id| plist.integer(id)) {
        *tile_points = size.clamp(16, 128) as u32;
    }
    if let Some(value) = plist.lookup(root, "show-recents").and_then(|id| plist.boolean(id)) {
        *show_recents = value;
    }
    if let Some(value) = plist.lookup(root, "show-trash").and_then(|id| plist.boolean(id)) {
        *show_trash = value;
    }
    let tile = *tile_points * scale / 1000;

    let Some(list) = plist.lookup(root, "persistent-apps") else { return; };
    let mut count = 0;
    for (_, entry) in plist.children(list) {
        // Either a bare path, or a dict with one under "path".
        let path_id = plist.lookup(entry, "path").unwrap_or(entry);
        let mut buffer = [0u8; 128];
        let Some(path) = plist.string_into(path_id, &mut buffer) else { continue; };
        if count == MAX_APPS {
            break;
        }
        match load_app(path, tile) {
            Some(mut app) => {
                app.kept = true;
                apps[count] = Some(app);
                count += 1;
            }
            None => log(&[path, ": not an app"]),
        }
    }
}

fn run() -> i32 {
    let Some(desktop) = connection::wait_for_desktop() else {
        log(&["this system has no desktop; idle"]);
        sys::idle_forever();
    };

    let mut apps = [None; MAX_APPS];
    let mut tile_points = DEFAULT_TILE_POINTS;
    let mut show_recents = true;
    let mut show_trash = true;
    configure(&mut apps, &mut tile_points, &mut show_recents, &mut show_trash, desktop.scale_permille);
    let tile = (tile_points * desktop.scale_permille / 1000).max(16);

    let mut info = Connect::empty(session::KIND_DOCK);
    session::set_str(&mut info.name, "Dock");
    session::set_str(&mut info.bundle_path, "/System/Library/CoreServices/Dock.app");
    let connection = match Connection::open(&info) {
        Ok(connection) => connection,
        Err(_) => {
            log(&["the desktop already has a Dock"]);
            return 2;
        }
    };

    // The widest Dock: every slot a tile (the desktop's buffer is a third of
    // the screen tall, which a Dock never comes near).
    let metrics = Metrics::new(tile);
    let max_width = (metrics.pad_x * 2 + (tile + metrics.gap) * MAX_SLOTS as u32) as usize;
    let frame_pixels = max_width * metrics.height() as usize;
    let (Some(frame), Some(trash)) = (sys::map(frame_pixels * 4), sys::map(tile as usize * tile as usize * 4)) else {
        log(&["no memory"]);
        return 3;
    };
    let frame = unsafe { core::slice::from_raw_parts_mut(frame.as_mut_ptr() as *mut u32, frame_pixels) };
    let trash = unsafe { core::slice::from_raw_parts_mut(trash.as_mut_ptr() as *mut u32, tile as usize * tile as usize) };
    draw_trash(tile, trash);

    let mut dock = Dock {
        connection,
        tile,
        show_recents,
        show_trash,
        apps,
        trash,
        frame,
        layout: Layout::new(metrics, &[]),
        tiles: [usize::MAX; MAX_SLOTS],
        hovered: None,
        pressed: None,
        pointer: None,
        dirty: true,
    };
    dock.relayout();

    let kept = dock.apps.iter().flatten().count();
    let mut digits = [0u8; 20];
    log(&["showing ", sys::decimal(kept as u64, &mut digits), " app(s)"]);

    loop {
        dock.render_and_submit();

        let mut wait = CHECK_INTERVAL_MS;
        loop {
            match dock.connection.receive(wait) {
                Received::Message(message) => {
                    wait = 0;
                    match message.kind {
                        session::MSG_EVENT => dock.pointer(&message),
                        session::MSG_LAUNCH => {
                            let bundle = Text::<128>::from(session::get_str(&message.path));
                            dock.open_bundle(bundle.as_str());
                        }
                        session::MSG_QUIT => return 0,
                        _ => {}
                    }
                }
                Received::Empty => break,
                Received::Closed => return 0,
            }
        }
        dock.check_apps();
    }
}

/// Called by AppKit.framework's `main` (NXU's frameworks/AppKit.framework).
#[unsafe(no_mangle)]
pub extern "C" fn UIApplicationMain() -> i32 {
    run()
}
