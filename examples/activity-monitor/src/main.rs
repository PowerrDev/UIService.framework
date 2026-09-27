//! Host preview of Activity Monitor: the real app and the real Inter text
//! renderer over a fake `ui::core::activity` host that plays back a
//! plausible sevOS machine (four CPUs, the boot daemons, a busy shell), and
//! the same tick -> redraw loop the desktop runs. Writes PPM files.
//!
//! ```text
//! cargo run -p activity-monitor-preview -- [OUT_DIR] [SCALE_PERMILLE ...]
//! ```

use std::cell::Cell;
use std::ffi::c_void;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use activity_monitor::ActivityMonitorApp;
use ui::core::activity::{self, Activity, ProcessInfo};
use ui::core::scale;
use ui::prelude::*;
use ui::render::Surface;
use ui::text::{FontFamily, TextScratch, TtfTextRenderer};

const REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
const SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf");

/// The default window's content area: 620 x 400 pt less the 32 pt titlebar.
const FULL: (u32, u32) = (620, 368);
/// The smallest window: 460 x 320 pt.
const SMALLEST: (u32, u32) = (460, 288);

thread_local! {
    /// Samples the fake host has answered: its clock.
    static STEP: Cell<u64> = const { Cell::new(0) };
}

/// (name, pid, flags, threads, share of a CPU in percent)
const PROCESSES: &[(&str, u32, u32, u32, u64)] = &[
    ("kernel_task", 0, activity::FLAG_KERNEL, 9, 38),
    ("bootd", 1, activity::FLAG_SYSTEM, 1, 1),
    ("logd", 2, activity::FLAG_SYSTEM, 1, 4),
    ("patchd", 3, activity::FLAG_SYSTEM, 1, 2),
    ("sh", 12, 0, 1, 0),
    ("mandelbrot", 17, 0, 4, 190),
    ("sound_test", 21, 0, 2, 7),
];

/// CPU `cpu`'s load during second `step`, in percent.
fn load(step: u64, cpu: usize) -> u64 {
    let phase = (step + cpu as u64 * 11) % 40;
    let wave = if phase < 20 { phase * 4 } else { (40 - phase) * 4 };
    (15 + wave * (4 - cpu as u64) / 4 + (step * 7 + cpu as u64 * 13) % 9).min(99)
}

unsafe extern "C" fn fake_sample(
    _context: *mut c_void,
    out: *mut Activity,
    processes: *mut ProcessInfo,
    capacity: u32,
    count_out: *mut u32,
) -> u32 {
    let step = STEP.with(|step| {
        step.set(step.get() + 1);
        step.get()
    });
    const HZ: u64 = 100;
    let ticks = step * HZ;
    let mut activity = Activity::empty();
    activity.cpu_count = 4;
    activity.uptime_us = 754 * 1_000_000 + step * 1_000_000;
    activity.page_size = 4096;
    activity.total_pages = 131_072;
    activity.free_pages = 96_512 - (step % 7) * 40;
    activity.heap_pages = 900;
    activity.process_count = PROCESSES.len() as u32;
    activity.thread_count = PROCESSES.iter().map(|p| p.3).sum();
    for cpu in 0..4usize {
        // A wandering load, different per CPU, accumulated tick by tick the
        // way the kernel counts it.
        let busy: u64 = (1..=step).map(|s| load(s, cpu) * HZ / 100).sum();
        activity.cpus[cpu].ticks = ticks;
        activity.cpus[cpu].busy_ticks = busy;
        activity.cpus[cpu].context_switches = step * 900;
        activity.cpus[cpu].online = 1;
    }

    let count = PROCESSES.len().min(capacity as usize);
    for (index, &(name, pid, flags, threads, share)) in PROCESSES.iter().take(count).enumerate() {
        let mut process = ProcessInfo::empty().with_name(name);
        process.uniqueid = u64::from(pid) + 1;
        process.pid = pid;
        process.ppid = if pid == 0 { 0 } else { 1 };
        process.flags = flags;
        process.threads = threads;
        process.cpu_ticks = 4000 * share / 100 + step * share + (step * u64::from(pid) % 3);
        process.state = if share > 50 { activity::STATE_RUNNING } else { activity::STATE_SLEEPING };
        process.mlfq_level = if share > 50 { 3 } else if share > 5 { 1 } else { 0 };
        process.last_cpu = pid % 4;
        unsafe { *processes.add(index) = process };
    }
    unsafe {
        *out = activity;
        *count_out = count as u32;
    }
    0
}

struct Preview<'a> {
    app: ActivityMonitorApp,
    pixels: Vec<u32>,
    width: u32,
    height: u32,
    permille: i32,
    clock_us: u64,
    text: TtfTextRenderer<'a, 'a>,
}

impl<'a> Preview<'a> {
    fn new(text: TtfTextRenderer<'a, 'a>, permille: u32, points: (u32, u32)) -> Self {
        scale::set_permille(permille);
        let width = points.0 * permille / 1000;
        let height = points.1 * permille / 1000;
        let mut preview = Self {
            app: ActivityMonitorApp::new(),
            pixels: vec![0; (width * height) as usize],
            width,
            height,
            permille: permille as i32,
            clock_us: 1_000_000,
            text,
        };
        preview.draw();
        preview
    }

    fn draw(&mut self) {
        let mut surface = Surface::new(&mut self.pixels, self.width, self.height, self.width).expect("surface");
        let mut frame = Frame::new(&mut surface, &mut self.text);
        self.app.draw(&mut frame);
    }

    fn send(&mut self, event: Event) {
        let size = Size::new(self.width, self.height);
        if self.app.event(event, size) == AppAction::Redraw {
            self.draw();
        }
    }

    fn point(&self, x: i32, y: i32) -> Point {
        Point::new(x * self.permille / 1000, y * self.permille / 1000)
    }

    fn click(&mut self, x: i32, y: i32) {
        let position = self.point(x, y);
        self.send(Event::PointerMoved { position });
        self.send(Event::PointerDown { position, button: PointerButton::Primary });
        self.send(Event::PointerUp { position, button: PointerButton::Primary });
    }

    /// `seconds` of the desktop's loop at 60 frames a second.
    fn run(&mut self, seconds: u64) {
        for _ in 0..seconds * 60 {
            self.clock_us += 16_667;
            if self.app.tick(self.clock_us) == AppAction::Redraw {
                self.draw();
            }
        }
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

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let out: PathBuf = args.next().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&out)?;
    let scales: Vec<u32> = args.filter_map(|value| value.parse().ok()).collect();
    let scales = if scales.is_empty() { vec![1000, 2000] } else { scales };

    activity::set_backend(core::ptr::null_mut(), Some(fake_sample));

    for permille in scales {
        let tag = format!("x{permille}");
        let scratch: &'static mut TextScratch = Box::leak(Box::new(TextScratch::new()));
        let scratch_ptr = scratch as *mut TextScratch;
        let make = |points: (u32, u32)| {
            // SAFETY: previews are built one at a time and dropped before the next.
            let scratch = unsafe { &mut *scratch_ptr };
            let family = FontFamily::from_bytes(REGULAR, Some(SEMIBOLD)).expect("Inter");
            Preview::new(TtfTextRenderer::new(family, scratch), permille, points)
        };

        let mut view = make(FULL);
        view.save(&out, &format!("{tag}-01-first-sample"))?;
        view.run(40);
        view.save(&out, &format!("{tag}-02-forty-seconds"))?;
        // Select the third row, sort by name.
        view.click(100, 44 + 22 + 20 * 2 + 10);
        view.click(60, 44 + 11);
        view.save(&out, &format!("{tag}-03-by-name-selected"))?;
        // The System filter.
        view.click(620 - 12 - 29, 22);
        view.save(&out, &format!("{tag}-04-system-only"))?;
        drop(view);

        let mut small = make(SMALLEST);
        small.run(20);
        small.save(&out, &format!("{tag}-05-smallest"))?;
    }
    Ok(())
}
