#![no_std]

//! Geometry, color and normalized input primitives shared by UIService.

pub mod system_color;

/// The host's content scale (physical pixels per design point), set once at
/// boot from the actual QEMU/host density instead of assuming a fixed 2x
/// "Retina" canvas. Every hardcoded "this canvas is 2x density" pixel budget
/// across UIService should be expressed as a 1x point value converted
/// through [`scale::pt`] so it lands at the right on-screen size on any host,
/// not just one whose backingScaleFactor happens to be exactly 2.
pub mod scale {
    use core::sync::atomic::{AtomicU32, Ordering};

    /// Physical pixels per design point, expressed as thousandths (e.g.
    /// `2000` == 2.0x). Defaults to `2000` so anything drawn before a host
    /// reports its real scale (or a host too old to report one at all)
    /// keeps today's historical, hardcoded-2x appearance.
    static PERMILLE: AtomicU32 = AtomicU32::new(2000);

    /// Record the host's real content scale. Ignores `0`, which would make
    /// [`pt`] collapse every dimension to nothing.
    pub fn set_permille(permille: u32) {
        if permille != 0 {
            PERMILLE.store(permille, Ordering::Relaxed);
        }
    }

    pub fn permille() -> u32 {
        PERMILLE.load(Ordering::Relaxed)
    }

    pub fn factor() -> f32 {
        permille() as f32 / 1000.0
    }

    /// Convert a 1x design-point dimension into physical pixels at the
    /// current host content scale.
    pub fn pt(points: u32) -> u32 {
        ((points as u64 * permille() as u64) / 1000) as u32
    }

    pub fn pt_i32(points: i32) -> i32 {
        ((points as i64 * permille() as i64) / 1000) as i32
    }
}

/// Read-only directory listing, provided by the host when it reports
/// `UI_SERVICE_HOST_CAP_FS` (see `ui_platform::InteractiveHostV5`). Mirrors
/// `scale`'s pattern: a small global wired up once at boot from the
/// connected host ABI, since an app has no host handle of its own to call
/// through (`ui_app::App`'s `draw`/`event` only ever see a `Frame`/`Event`)
/// -- this lets one call `fs::list_directory` directly instead of a
/// capability being threaded through every layer in between.
pub mod fs {
    use core::ffi::c_void;
    use core::sync::atomic::{AtomicUsize, Ordering};

    pub const NAME_MAX: usize = 63;

    /// Matches `UIServiceDirEntry`'s C layout exactly (see UIService.h) so
    /// the host callback can write directly into a caller-provided buffer
    /// of these -- no intermediate copy or separate raw type.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct DirEntry {
        name_length: u32,
        name: [u8; NAME_MAX + 1],
        kind: u32,
        pub size_bytes: u64,
    }

    impl DirEntry {
        pub const fn empty() -> Self {
            Self { name_length: 0, name: [0; NAME_MAX + 1], kind: 0, size_bytes: 0 }
        }

        pub fn name(&self) -> &str {
            let length = (self.name_length as usize).min(NAME_MAX);
            core::str::from_utf8(&self.name[..length]).unwrap_or("")
        }

        pub fn is_directory(&self) -> bool {
            self.kind == 1
        }
    }

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    pub enum Error {
        /// No host is connected yet, or it never reported the filesystem
        /// capability at all.
        Unavailable,
        NotFound,
        NotADirectory,
        Other,
    }

    pub struct Listing {
        pub count: usize,
        /// `true` when the directory held more entries than the caller's
        /// buffer could hold -- the first `count` are still valid, just not
        /// the whole directory.
        pub truncated: bool,
    }

    /// Matches `UIServiceListDirectoryFn` in UIService.h.
    type RawListDirectoryFn = unsafe extern "C" fn(
        context: *mut c_void,
        path: *const u8,
        path_len: u32,
        entries: *mut DirEntry,
        capacity: u32,
        count_out: *mut u32,
        truncated_out: *mut bool,
    ) -> u32;

    static CONTEXT: AtomicUsize = AtomicUsize::new(0);
    static FUNCTION: AtomicUsize = AtomicUsize::new(0);

    /// Wire up the host's real filesystem callback. Called once, from
    /// `ui_platform::InteractiveHostV5::connect`; never called by
    /// application code.
    pub fn set_backend(context: *mut c_void, function: Option<RawListDirectoryFn>) {
        CONTEXT.store(context as usize, Ordering::Relaxed);
        FUNCTION.store(function.map_or(0, |f| f as usize), Ordering::Relaxed);
    }

    /// List up to `entries.len()` entries of the directory at `path` (an
    /// absolute path, not necessarily NUL-terminated -- the raw call gets
    /// an explicit length).
    pub fn list_directory(path: &str, entries: &mut [DirEntry]) -> Result<Listing, Error> {
        let function_addr = FUNCTION.load(Ordering::Relaxed);
        if function_addr == 0 {
            return Err(Error::Unavailable);
        }

        // SAFETY: `FUNCTION` only ever holds an address `set_backend` was
        // given as a real `RawListDirectoryFn`, or 0 (handled above).
        let function: RawListDirectoryFn = unsafe { core::mem::transmute(function_addr) };
        let context = CONTEXT.load(Ordering::Relaxed) as *mut c_void;

        let mut count: u32 = 0;
        let mut truncated = false;
        let status = unsafe {
            function(
                context,
                path.as_ptr(),
                path.len() as u32,
                entries.as_mut_ptr(),
                entries.len() as u32,
                &mut count,
                &mut truncated,
            )
        };

        match status {
            0 => Ok(Listing { count: (count as usize).min(entries.len()), truncated }),
            1 => Err(Error::NotFound),
            2 => Err(Error::NotADirectory),
            _ => Err(Error::Other),
        }
    }
}

/// Processes, CPUs and memory, provided by the host when it reports
/// `UI_SERVICE_HOST_CAP_ACTIVITY` -- what Activity Monitor shows. Wired up
/// once at connect time, like [`fs`].
///
/// CPU time is in scheduler ticks: a process's share over an interval is how
/// far its `cpu_ticks` moved against one CPU's `ticks` (100 = one whole CPU),
/// so nothing here needs to know the host's tick rate.
pub mod activity {
    use core::ffi::c_void;
    use core::sync::atomic::{AtomicUsize, Ordering};

    pub const NAME_MAX: usize = 31;
    pub const CPU_MAX: usize = 8;

    pub const STATE_RUNNING: u32 = 0;
    pub const STATE_RUNNABLE: u32 = 1;
    pub const STATE_SLEEPING: u32 = 2;
    pub const STATE_STOPPED: u32 = 3;
    pub const STATE_OTHER: u32 = 4;

    pub const FLAG_KERNEL: u32 = 1 << 0;
    pub const FLAG_SYSTEM: u32 = 1 << 1;

    /// `UIServiceProcessInfo` (UIService.h), bit for bit.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct ProcessInfo {
        pub uniqueid: u64,
        pub cpu_ticks: u64,
        pub pid: u32,
        pub ppid: u32,
        pub state: u32,
        pub flags: u32,
        pub threads: u32,
        pub mlfq_level: u32,
        pub last_cpu: u32,
        name_length: u32,
        name: [u8; NAME_MAX + 1],
    }

    impl ProcessInfo {
        pub const fn empty() -> Self {
            Self {
                uniqueid: 0,
                cpu_ticks: 0,
                pid: 0,
                ppid: 0,
                state: STATE_OTHER,
                flags: 0,
                threads: 0,
                mlfq_level: 0,
                last_cpu: 0,
                name_length: 0,
                name: [0; NAME_MAX + 1],
            }
        }

        /// For hosts written in Rust (previews, tests).
        pub fn with_name(mut self, name: &str) -> Self {
            let length = name.len().min(NAME_MAX);
            self.name[..length].copy_from_slice(&name.as_bytes()[..length]);
            self.name[length] = 0;
            self.name_length = length as u32;
            self
        }

        pub fn name(&self) -> &str {
            let length = (self.name_length as usize).min(NAME_MAX);
            match core::str::from_utf8(&self.name[..length]) {
                Ok(name) => name,
                // Cut in the middle of a character: keep the whole ones.
                Err(error) => core::str::from_utf8(&self.name[..error.valid_up_to()]).unwrap_or(""),
            }
        }

        pub const fn is_kernel(&self) -> bool {
            self.flags & FLAG_KERNEL != 0
        }
    }

    /// `UIServiceCpuInfo`.
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct CpuInfo {
        pub ticks: u64,
        pub busy_ticks: u64,
        pub context_switches: u64,
        pub online: u32,
        pub reserved: u32,
    }

    /// `UIServiceActivity`.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Activity {
        pub struct_size: u32,
        pub cpu_count: u32,
        pub uptime_us: u64,
        pub page_size: u64,
        pub total_pages: u64,
        pub free_pages: u64,
        pub heap_pages: u64,
        pub process_count: u32,
        pub thread_count: u32,
        pub cpus: [CpuInfo; CPU_MAX],
    }

    impl Activity {
        pub const fn empty() -> Self {
            Self {
                struct_size: core::mem::size_of::<Self>() as u32,
                cpu_count: 0,
                uptime_us: 0,
                page_size: 0,
                total_pages: 0,
                free_pages: 0,
                heap_pages: 0,
                process_count: 0,
                thread_count: 0,
                cpus: [CpuInfo { ticks: 0, busy_ticks: 0, context_switches: 0, online: 0, reserved: 0 }; CPU_MAX],
            }
        }
    }

    // The C side has the same numbers (see UIService.h's layout note).
    const _: () = assert!(core::mem::size_of::<ProcessInfo>() == 80);
    const _: () = assert!(core::mem::size_of::<CpuInfo>() == 32);
    const _: () = assert!(core::mem::size_of::<Activity>() == 56 + 32 * CPU_MAX);

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    pub enum Error {
        /// No host, or one without `UI_SERVICE_HOST_CAP_ACTIVITY`.
        Unavailable,
        Failed,
    }

    /// Matches `UIServiceGetActivityFn` in UIService.h.
    pub type RawGetActivityFn = unsafe extern "C" fn(
        context: *mut c_void,
        activity: *mut Activity,
        processes: *mut ProcessInfo,
        capacity: u32,
        count_out: *mut u32,
    ) -> u32;

    static CONTEXT: AtomicUsize = AtomicUsize::new(0);
    static FUNCTION: AtomicUsize = AtomicUsize::new(0);

    /// Wire up the host's callback. Called from
    /// `ui_platform::InteractiveHostV5::connect` (or a preview's own fake).
    pub fn set_backend(context: *mut c_void, function: Option<RawGetActivityFn>) {
        CONTEXT.store(context as usize, Ordering::Relaxed);
        FUNCTION.store(function.map_or(0, |f| f as usize), Ordering::Relaxed);
    }

    pub fn available() -> bool {
        FUNCTION.load(Ordering::Relaxed) != 0
    }

    /// Take a sample: fills `activity` and the front of `processes`, and
    /// returns how many processes were written.
    pub fn sample(activity: &mut Activity, processes: &mut [ProcessInfo]) -> Result<usize, Error> {
        let function_addr = FUNCTION.load(Ordering::Relaxed);
        if function_addr == 0 {
            return Err(Error::Unavailable);
        }

        // SAFETY: only `set_backend` stores here, always a real function.
        let function: RawGetActivityFn = unsafe { core::mem::transmute(function_addr) };
        let context = CONTEXT.load(Ordering::Relaxed) as *mut c_void;

        activity.struct_size = core::mem::size_of::<Activity>() as u32;
        let mut count: u32 = 0;
        let status = unsafe {
            function(context, activity, processes.as_mut_ptr(), processes.len() as u32, &mut count)
        };

        if status != 0 {
            return Err(Error::Failed);
        }
        Ok((count as usize).min(processes.len()))
    }
}

/// App bundles (`/Applications/<Name>.app`), for apps that show or open
/// them (Voyager): an app's icon, and asking the system to open one. Wired
/// up once by the app's runtime, like [`fs`]; unavailable (every call answers
/// `false`) where nothing was wired.
pub mod bundle {
    use core::sync::atomic::{AtomicUsize, Ordering};

    /// Fill `pixels` (`size` x `size`, `0xAARRGGBB`, straight alpha) with the
    /// icon of the bundle at `path`. False when it has none or it will not load.
    pub type RawAppIconFn = fn(path: &str, size: u32, pixels: &mut [u32]) -> bool;
    /// Open (launch, or bring forward) the app bundle at `path`.
    pub type RawOpenFn = fn(path: &str) -> bool;

    static ICON: AtomicUsize = AtomicUsize::new(0);
    static OPEN: AtomicUsize = AtomicUsize::new(0);

    pub fn set_backend(icon: Option<RawAppIconFn>, open: Option<RawOpenFn>) {
        ICON.store(icon.map_or(0, |f| f as usize), Ordering::Relaxed);
        OPEN.store(open.map_or(0, |f| f as usize), Ordering::Relaxed);
    }

    /// Whether `name` is an app bundle's (a folder whose name ends in `.app`).
    pub fn is_app_name(name: &str) -> bool {
        name.len() > 4 && name[name.len() - 4..].eq_ignore_ascii_case(".app")
    }

    pub fn app_icon(path: &str, size: u32, pixels: &mut [u32]) -> bool {
        let function = ICON.load(Ordering::Relaxed);
        if function == 0 || pixels.len() < size as usize * size as usize {
            return false;
        }
        // SAFETY: only `set_backend` stores here, always a real function.
        let function: RawAppIconFn = unsafe { core::mem::transmute(function) };
        function(path, size, pixels)
    }

    pub fn open(path: &str) -> bool {
        let function = OPEN.load(Ordering::Relaxed);
        if function == 0 {
            return false;
        }
        // SAFETY: as above.
        let function: RawOpenFn = unsafe { core::mem::transmute(function) };
        function(path)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    pub const fn offset(self, dx: i32, dy: i32) -> Self {
        Self::new(self.x + dx, self.y + dy)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

impl Size {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Rect {
    pub origin: Point,
    pub size: Size,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            origin: Point::new(x, y),
            size: Size::new(width, height),
        }
    }

    pub fn contains(self, point: Point) -> bool {
        if point.x < self.origin.x || point.y < self.origin.y {
            return false;
        }

        let local_x = (point.x - self.origin.x) as u32;
        let local_y = (point.y - self.origin.y) as u32;
        local_x < self.size.width && local_y < self.size.height
    }

    pub const fn translated(self, dx: i32, dy: i32) -> Self {
        Self::new(
            self.origin.x + dx,
            self.origin.y + dy,
            self.size.width,
            self.size.height,
        )
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl Color {
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);

    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha: 255,
        }
    }

    pub const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }

    pub const fn with_alpha(self, alpha: u8) -> Self {
        Self {
            red: self.red,
            green: self.green,
            blue: self.blue,
            alpha,
        }
    }

    pub const fn from_xrgb8888(value: u32) -> Self {
        Self::rgb(
            ((value >> 16) & 0xFF) as u8,
            ((value >> 8) & 0xFF) as u8,
            (value & 0xFF) as u8,
        )
    }

    pub const fn to_xrgb8888(self) -> u32 {
        ((self.red as u32) << 16) | ((self.green as u32) << 8) | self.blue as u32
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Event {
    PointerMoved {
        position: Point,
    },
    PointerDown {
        position: Point,
        button: PointerButton,
    },
    PointerUp {
        position: Point,
        button: PointerButton,
    },
    PointerLeft,
    /// The scroll wheel (or a trackpad's two-finger scroll) turned while the
    /// pointer was at `position`. `delta` counts wheel notches: positive is
    /// "wheel up" (the content moves down, towards its start), negative is
    /// "wheel down". A trackpad delivers many small deltas rather than a few
    /// large ones, so an app should animate towards where they add up to
    /// instead of jumping (see `App::tick`).
    Scroll {
        position: Point,
        delta: i32,
    },
    /// A key went down or auto-repeats. `code` is the evdev key code (see
    /// [`key`]), `character` what it types under the host's layout and
    /// modifiers, if anything. Only hosts with a keyboard send these.
    KeyDown {
        code: u32,
        character: Option<char>,
    },
}

/// evdev key codes an app is likely to act on (the host passes every code
/// through; these are only names for the common ones).
pub mod key {
    pub const ESCAPE: u32 = 1;
    pub const BACKSPACE: u32 = 14;
    pub const TAB: u32 = 15;
    pub const ENTER: u32 = 28;
    pub const LEFT: u32 = 105;
    pub const RIGHT: u32 = 106;
    pub const UP: u32 = 103;
    pub const DOWN: u32 = 108;
}
