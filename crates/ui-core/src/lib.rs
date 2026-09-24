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
}
