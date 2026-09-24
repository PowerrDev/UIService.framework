//! Host preview of Voyager: the real app, the real Inter text renderer, a
//! small sample filesystem behind the real `ui::core::fs` hook, and the same
//! event -> redraw loop the NXU runtime runs. Writes PPM files (convert with
//! any viewer) for a set of named scenarios, at one or more content scales.
//!
//! ```text
//! cargo run -p voyager-preview -- [OUT_DIR] [SCALE_PERMILLE ...]
//! ```
//!
//! Scenario coordinates are in 1x points, measured from the top-left of the
//! window's content area.

use std::ffi::c_void;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use ui::core::fs;
use ui::core::scale;
use ui::prelude::*;
use ui::render::Surface;
use ui::text::{FontFamily, TextScratch, TtfTextRenderer};
use voyager::VoyagerApp;

const REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
const SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf");

/// Voyager's default window is 430x300 points (`WindowConfig` authors 860x600,
/// a doubled 2x reference); the content area is what is left under the 32 pt
/// title bar.
const FULL: (u32, u32) = (430, 268);
/// The smallest window Voyager allows (240x180 pt).
const SMALLEST: (u32, u32) = (240, 148);
/// The largest: the 860x600 px backing store, which at 1x is 860x568 pt of content.
const LARGEST: (u32, u32) = (860, 568);

/// Mirror of `UIServiceDirEntry` (see `ui::core::fs::DirEntry`, which keeps its
/// fields private): the callback writes through this layout.
#[repr(C)]
#[derive(Clone, Copy)]
struct RawEntry {
    name_length: u32,
    name: [u8; fs::NAME_MAX + 1],
    kind: u32,
    size_bytes: u64,
}

struct Sample {
    path: &'static str,
    entries: &'static [(&'static str, bool, u64)],
}

const SAMPLES: &[Sample] = &[
    Sample { path: "/disk", entries: &[("System", true, 0), ("hello.txt", false, 41)] },
    Sample { path: "/disk/System", entries: &[("Library", true, 0), ("Recovery", true, 0), ("README.txt", false, 1_312)] },
    Sample { path: "/disk/System/Library", entries: &[("BootDaemons", true, 0), ("CoreServices", true, 0), ("Resources", true, 0)] },
    Sample {
        path: "/disk/System/Library/Resources",
        entries: &[
            ("Audio", true, 0),
            ("Cursors", true, 0),
            ("Backgrounds", true, 0),
            ("Boot_Audio.wav", false, 419_800),
            ("Boot_Audio.mp3", false, 42_826),
            ("logo.png", false, 88_204),
            ("wallpaper.jpg", false, 2_310_552),
            ("Release notes.md", false, 9_120),
            ("Sevos Manual.pdf", false, 4_800_120),
            ("An extraordinarily long file name that keeps going and going.txt", false, 12),
        ],
    },
    Sample { path: "/empty", entries: &[] },
];

unsafe extern "C" fn list_sample(
    _context: *mut c_void,
    path: *const u8,
    path_len: u32,
    entries: *mut fs::DirEntry,
    capacity: u32,
    count_out: *mut u32,
    truncated_out: *mut bool,
) -> u32 {
    let path = unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(path, path_len as usize)) };
    let Some(sample) = SAMPLES.iter().find(|sample| sample.path == path) else {
        return 1;
    };

    let entries = entries as *mut RawEntry;
    let count = sample.entries.len().min(capacity as usize);
    for (index, (name, directory, size)) in sample.entries.iter().take(count).enumerate() {
        let mut raw = RawEntry { name_length: 0, name: [0; fs::NAME_MAX + 1], kind: u32::from(*directory), size_bytes: *size };
        let length = name.len().min(fs::NAME_MAX);
        raw.name[..length].copy_from_slice(&name.as_bytes()[..length]);
        raw.name_length = length as u32;
        unsafe { entries.add(index).write(raw) };
    }
    unsafe {
        *count_out = count as u32;
        *truncated_out = sample.entries.len() > count;
    }
    0
}

struct Preview<'a> {
    app: VoyagerApp,
    pixels: Vec<u32>,
    width: u32,
    height: u32,
    permille: i32,
    /// The synthetic monotonic clock `frames` advances.
    clock_us: u64,
    text: TtfTextRenderer<'a, 'a>,
}

impl<'a> Preview<'a> {
    fn new(text: TtfTextRenderer<'a, 'a>, permille: u32, points: (u32, u32)) -> Self {
        scale::set_permille(permille);
        let width = points.0 * permille / 1000;
        let height = points.1 * permille / 1000;
        let mut preview = Self { app: VoyagerApp::new(), pixels: vec![0; (width * height) as usize], width, height, permille: permille as i32, clock_us: 1_000_000, text };
        preview.draw();
        preview
    }

    fn draw(&mut self) {
        let mut surface = Surface::new(&mut self.pixels, self.width, self.height, self.width).expect("surface");
        let mut frame = Frame::new(&mut surface, &mut self.text);
        self.app.draw(&mut frame);
    }

    /// Points to physical pixels.
    fn px(&self, points: i32) -> i32 {
        points * self.permille / 1000
    }

    /// Deliver an event and, like the runtime, redraw when the app asks to.
    fn send(&mut self, event: Event) {
        let size = Size::new(self.width, self.height);
        if self.app.event(event, size) == AppAction::Redraw {
            self.draw();
        }
    }

    fn point(&self, x: i32, y: i32) -> Point {
        Point::new(self.px(x), self.px(y))
    }

    /// Turn the wheel over (x, y): positive `notches` is wheel up.
    fn wheel(&mut self, x: i32, y: i32, notches: i32) {
        let position = self.point(x, y);
        self.send(Event::Scroll { position, delta: notches });
    }

    /// Run the runtime's animation loop for `frames` frames of `frame_us`
    /// each, on a synthetic clock, redrawing when the app asks to.
    fn frames(&mut self, frames: u32, frame_us: u64) {
        for _ in 0..frames {
            if !self.app.animating() {
                break;
            }
            self.clock_us += frame_us;
            if self.app.tick(self.clock_us) == AppAction::Redraw {
                self.draw();
            }
        }
    }

    fn hover(&mut self, x: i32, y: i32) {
        let position = self.point(x, y);
        self.send(Event::PointerMoved { position });
    }

    fn click(&mut self, x: i32, y: i32) {
        let position = self.point(x, y);
        self.send(Event::PointerMoved { position });
        self.send(Event::PointerDown { position, button: PointerButton::Primary });
        self.send(Event::PointerUp { position, button: PointerButton::Primary });
    }

    /// Button down, not yet up.
    fn press(&mut self, x: i32, y: i32) {
        let position = self.point(x, y);
        self.send(Event::PointerMoved { position });
        self.send(Event::PointerDown { position, button: PointerButton::Primary });
    }

    fn save(&self, dir: &Path, name: &str) -> std::io::Result<()> {
        let path = dir.join(format!("{name}.ppm"));
        let mut writer = BufWriter::new(File::create(&path)?);
        write!(writer, "P6\n{} {}\n255\n", self.width, self.height)?;
        for pixel in &self.pixels {
            writer.write_all(&[(pixel >> 16) as u8, (pixel >> 8) as u8, *pixel as u8])?;
        }
        writer.flush()?;
        println!("wrote {}", path.display());
        Ok(())
    }
}

// Where things are, in points, for the default 430x268 window (see `layout.rs`).
const SIDEBAR_SYSTEM: (i32, i32) = (60, 82);
const SIDEBAR_RESOURCES: (i32, i32) = (60, 134);
const TOOLBAR_GRID_BUTTON: (i32, i32) = (301, 20);
const TOOLBAR_LIST_BUTTON: (i32, i32) = (335, 20);
const TOOLBAR_BACK: (i32, i32) = (73, 20);
const TOOLBAR_SIDEBAR: (i32, i32) = (30, 20);
const TOOLBAR_OPEN: (i32, i32) = (390, 20);
/// Centre of list row `n` (0-based) for a visible sidebar.
fn list_row(n: i32) -> (i32, i32) {
    (250, 40 + 24 + 12 + 24 * n)
}
/// Centre of grid tile `n` in the first row of a visible-sidebar window.
fn grid_tile(n: i32) -> (i32, i32) {
    (130 + 12 + 44 + (88 + 4) * n, 40 + 10 + 30)
}

fn scenario<'a>(make: &dyn Fn((u32, u32)) -> Preview<'a>, dir: &Path, tag: &str) -> std::io::Result<()> {
    // The volume root, as it first opens.
    let mut view = make(FULL);
    view.save(dir, &format!("{tag}-01-list"))?;

    view.click(list_row(0).0, list_row(0).1);
    view.save(dir, &format!("{tag}-02-list-selected"))?;

    // Pointer feedback: hover the view control, then press a sidebar row.
    view.hover(TOOLBAR_LIST_BUTTON.0, TOOLBAR_LIST_BUTTON.1);
    view.hover(TOOLBAR_GRID_BUTTON.0, TOOLBAR_GRID_BUTTON.1);
    view.save(dir, &format!("{tag}-03-hover-grid-button"))?;
    view.press(SIDEBAR_SYSTEM.0, SIDEBAR_SYSTEM.1);
    view.save(dir, &format!("{tag}-04-press-sidebar"))?;

    // Icon view of the root.
    let mut grid = make(FULL);
    grid.click(TOOLBAR_GRID_BUTTON.0, TOOLBAR_GRID_BUTTON.1);
    grid.save(dir, &format!("{tag}-05-grid"))?;
    grid.click(grid_tile(0).0, grid_tile(0).1);
    grid.save(dir, &format!("{tag}-06-grid-selected"))?;

    // A folder holding every kind of file, with a name too long to fit.
    let mut many = make(FULL);
    many.click(SIDEBAR_RESOURCES.0, SIDEBAR_RESOURCES.1);
    many.save(dir, &format!("{tag}-07-resources-list"))?;
    many.click(list_row(5).0, list_row(5).1);
    many.save(dir, &format!("{tag}-08-resources-list-selected"))?;
    many.click(TOOLBAR_GRID_BUTTON.0, TOOLBAR_GRID_BUTTON.1);
    many.save(dir, &format!("{tag}-09-resources-grid"))?;

    // Navigating into a folder enables Back, and Open is available on a folder.
    let mut nav = make(FULL);
    nav.click(list_row(0).0, list_row(0).1);
    nav.hover(TOOLBAR_OPEN.0, TOOLBAR_OPEN.1);
    nav.save(dir, &format!("{tag}-10-open-hover"))?;
    nav.click(list_row(0).0, list_row(0).1);
    nav.hover(TOOLBAR_BACK.0, TOOLBAR_BACK.1);
    nav.save(dir, &format!("{tag}-11-inside-system-back-hover"))?;

    // Sidebar hidden.
    let mut hidden = make(FULL);
    hidden.click(TOOLBAR_SIDEBAR.0, TOOLBAR_SIDEBAR.1);
    hidden.save(dir, &format!("{tag}-12-sidebar-hidden"))?;

    // The smallest window: the toolbar sheds Open and the title, controls still fit.
    let mut small = make(SMALLEST);
    small.click(SIDEBAR_RESOURCES.0, SIDEBAR_RESOURCES.1);
    small.save(dir, &format!("{tag}-13-smallest"))?;

    // The largest window (at 1x): Kind returns, the sidebar stops growing.
    let mut large = make(LARGEST);
    large.click(60, 134);
    large.click(500, 40 + 24 + 12 + 24 * 3);
    large.save(dir, &format!("{tag}-14-largest"))?;

    // Scrolling the Resources list (10 rows overflow the default window):
    // a wheel-down notch starts a glide; frames are 16.7 ms apart.
    let mut scrolled = make(FULL);
    scrolled.click(SIDEBAR_RESOURCES.0, SIDEBAR_RESOURCES.1);
    scrolled.wheel(250, 150, -3);
    scrolled.frames(3, 16_667);
    scrolled.save(dir, &format!("{tag}-15-scroll-mid-glide"))?;
    scrolled.frames(20, 16_667);
    scrolled.save(dir, &format!("{tag}-16-scroll-settled-with-indicator"))?;
    // Select a row that was off screen, then let the indicator fade away.
    scrolled.click(250, 150);
    scrolled.frames(200, 16_667);
    scrolled.save(dir, &format!("{tag}-17-scrolled-selected"))?;

    // The icon view scrolls too: the Resources folder in the grid.
    let mut grid_scrolled = make(FULL);
    grid_scrolled.click(SIDEBAR_RESOURCES.0, SIDEBAR_RESOURCES.1);
    grid_scrolled.click(TOOLBAR_GRID_BUTTON.0, TOOLBAR_GRID_BUTTON.1);
    grid_scrolled.wheel(250, 150, -4);
    grid_scrolled.frames(30, 16_667);
    grid_scrolled.save(dir, &format!("{tag}-18-grid-scrolled"))?;
    Ok(())
}

/// Text that costs nothing: what is left of a redraw without any glyphs.
struct NoText;

impl ui::render::TextRenderer for NoText {
    fn measure(&self, text: &str, point_size: u32, _semibold: bool) -> Size {
        Size::new(text.chars().count() as u32 * point_size / 2, point_size + point_size / 4)
    }

    fn draw<C: ui::render::Canvas + ?Sized>(&mut self, _: &mut C, _: Point, _: &str, _: Color, _: u32, _: bool) {}
}

/// Time full redraws of the Resources list (10 rows, every kind of icon).
fn bench(permille: u32) {
    use std::time::Instant;

    scale::set_permille(permille);
    fs::set_backend(core::ptr::null_mut(), Some(list_sample));
    let (width, height) = (FULL.0 * permille / 1000, FULL.1 * permille / 1000);
    let mut app = VoyagerApp::new();
    let size = Size::new(width, height);
    let point = |x: i32, y: i32| Point::new(x * permille as i32 / 1000, y * permille as i32 / 1000);
    let at = SIDEBAR_RESOURCES;
    for event in [
        Event::PointerMoved { position: point(at.0, at.1) },
        Event::PointerDown { position: point(at.0, at.1), button: PointerButton::Primary },
        Event::PointerUp { position: point(at.0, at.1), button: PointerButton::Primary },
    ] {
        app.event(event, size);
    }

    let mut pixels = vec![0u32; (width * height) as usize];
    const RUNS: u32 = 200;

    fn time<T: ui::render::TextRenderer>(app: &mut VoyagerApp, pixels: &mut [u32], width: u32, height: u32, text: &mut T, before: &mut dyn FnMut(&mut T)) -> f64 {
        let mut surface = Surface::new(pixels, width, height, width).expect("surface");
        let mut total = std::time::Duration::ZERO;
        for _ in 0..RUNS {
            before(text);
            let start = Instant::now();
            let mut frame = Frame::new(&mut surface, text);
            app.draw(&mut frame);
            total += start.elapsed();
        }
        total.as_secs_f64() * 1e6 / RUNS as f64
    }

    let scratch: &'static mut TextScratch = Box::leak(Box::new(TextScratch::new()));
    let scratch_ptr = scratch as *mut TextScratch;
    let family = || FontFamily::from_bytes(REGULAR, Some(SEMIBOLD)).expect("Inter");

    let mut warm = TtfTextRenderer::new(family(), unsafe { &mut *scratch_ptr });
    let warm_us = time(&mut app, &mut pixels, width, height, &mut warm, &mut |_| {});
    drop(warm);

    let mut cold = TtfTextRenderer::new(family(), unsafe { &mut *scratch_ptr });
    let cold_us = time(&mut app, &mut pixels, width, height, &mut cold, &mut |_| unsafe { (*scratch_ptr).flush_cache() });
    drop(cold);

    let mut none = NoText;
    let none_us = time(&mut app, &mut pixels, width, height, &mut none, &mut |_| {});

    println!("scale {permille}: full redraw  no text {none_us:8.0} us | text, cache warm {warm_us:8.0} us | text, cache cold {cold_us:8.0} us");
    println!("         text share of a redraw: warm {:.0}%  cold {:.0}%", (warm_us - none_us) / warm_us * 100.0, (cold_us - none_us) / cold_us * 100.0);
}

fn main() -> std::io::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--bench") {
        for permille in [1000, 2000] {
            bench(permille);
        }
        return Ok(());
    }

    let mut args = std::env::args().skip(1);
    let out: PathBuf = args.next().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&out)?;
    let scales: Vec<u32> = args.filter_map(|value| value.parse().ok()).collect();
    let scales = if scales.is_empty() { vec![1000, 2000] } else { scales };

    fs::set_backend(core::ptr::null_mut(), Some(list_sample));

    for permille in scales {
        // The renderer borrows the scratch for as long as the previews live.
        let scratch: &'static mut TextScratch = Box::leak(Box::new(TextScratch::new()));
        let scratch_ptr = scratch as *mut TextScratch;
        let make = |points: (u32, u32)| {
            // SAFETY: previews are built one at a time and dropped before the next.
            let scratch = unsafe { &mut *scratch_ptr };
            let family = FontFamily::from_bytes(REGULAR, Some(SEMIBOLD)).expect("Inter");
            Preview::new(TtfTextRenderer::new(family, scratch), permille, points)
        };
        scenario(&make, &out, &format!("x{permille}"))?;
    }
    Ok(())
}
