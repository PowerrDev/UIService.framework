#![no_std]

//! Stable repr(C) host ABI shared by UIService and NXU.

use core::ffi::c_void;
use core::mem::size_of;

pub const UI_SERVICE_ABI_VERSION_V1: u32 = 1;
pub const UI_SERVICE_ABI_VERSION_V2: u32 = 2;
pub const UI_SERVICE_ABI_VERSION_V3: u32 = 3;
pub const UI_SERVICE_ABI_VERSION_V4: u32 = 4;
pub const UI_SERVICE_ABI_VERSION_V5: u32 = 5;
pub const UI_SERVICE_ABI_VERSION: u32 = UI_SERVICE_ABI_VERSION_V5;
pub const UI_SERVICE_PIXEL_FORMAT_XRGB8888: u32 = 1;

pub const UI_SERVICE_HOST_CAP_PRESENT: u64 = 1 << 0;
pub const UI_SERVICE_HOST_CAP_DAMAGE: u64 = 1 << 1;
pub const UI_SERVICE_HOST_CAP_INPUT: u64 = 1 << 2;
pub const UI_SERVICE_HOST_CAP_TIME: u64 = 1 << 3;
pub const UI_SERVICE_HOST_CAP_FS: u64 = 1 << 4;
/// The host sends `HostEventType::KeyDown` events. Optional on any host that
/// has input; an app that never looks at keys is unaffected.
pub const UI_SERVICE_HOST_CAP_KEYBOARD: u64 = 1 << 5;
pub const UI_SERVICE_HOST_CAPABILITIES_V1: u64 = UI_SERVICE_HOST_CAP_PRESENT | UI_SERVICE_HOST_CAP_DAMAGE;
pub const UI_SERVICE_HOST_CAPABILITIES_V2: u64 = UI_SERVICE_HOST_CAPABILITIES_V1 | UI_SERVICE_HOST_CAP_INPUT;
pub const UI_SERVICE_HOST_CAPABILITIES_V3: u64 = UI_SERVICE_HOST_CAPABILITIES_V2 | UI_SERVICE_HOST_CAP_TIME;
/// V5's filesystem capability is optional even on a v5 host (About sevOS
/// has no use for it), so it is not folded into a default capability set --
/// a host adds `UI_SERVICE_HOST_CAP_FS` itself when it actually implements
/// `list_directory`.
pub const UI_SERVICE_HOST_CAPABILITIES_V4: u64 = UI_SERVICE_HOST_CAPABILITIES_V3;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Ok = 0,
    InvalidArgument = 1,
    BadVersion = 2,
    Unsupported = 3,
    NoSurface = 4,
    PresentFailed = 5,
}

impl Status {
    pub const fn is_ok(self) -> bool {
        matches!(self, Self::Ok)
    }

    pub const fn from_raw(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Ok),
            1 => Some(Self::InvalidArgument),
            2 => Some(Self::BadVersion),
            3 => Some(Self::Unsupported),
            4 => Some(Self::NoSurface),
            5 => Some(Self::PresentFailed),
            _ => None,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AbiHeader {
    pub struct_size: u32,
    pub abi_version: u32,
}

impl AbiHeader {
    pub const fn new(struct_size: u32, abi_version: u32) -> Self {
        Self {
            struct_size,
            abi_version,
        }
    }

    pub const fn v1<T>() -> Self {
        Self::new(size_of::<T>() as u32, UI_SERVICE_ABI_VERSION_V1)
    }

    pub const fn v2<T>() -> Self {
        Self::new(size_of::<T>() as u32, UI_SERVICE_ABI_VERSION_V2)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DamageRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl DamageRect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SurfaceDescriptor {
    pub pixels: *mut u32,
    pub width: u32,
    pub height: u32,
    pub stride_pixels: u32,
    pub pixel_format: u32,
}

impl SurfaceDescriptor {
    pub const fn empty() -> Self {
        Self {
            pixels: core::ptr::null_mut(),
            width: 0,
            height: 0,
            stride_pixels: 0,
            pixel_format: 0,
        }
    }

    pub const fn is_valid(&self) -> bool {
        !self.pixels.is_null()
            && self.width != 0
            && self.height != 0
            && self.stride_pixels >= self.width
            && self.pixel_format == UI_SERVICE_PIXEL_FORMAT_XRGB8888
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostEventType {
    None = 0,
    PointerMoved = 1,
    PointerDown = 2,
    PointerUp = 3,
    /// The wheel turned. `HostEvent::reserved` carries the signed number of
    /// notches (positive = wheel up) and `x`/`y` the pointer position.
    Scroll = 4,
    /// A key went down (or repeats). `HostEvent::button` carries the evdev
    /// key code (`ui_core::key`), `reserved` the character it types under
    /// the host's layout and modifiers as a Unicode scalar, 0 for none.
    /// Only sent by a host with `UI_SERVICE_HOST_CAP_KEYBOARD`.
    KeyDown = 5,
}

impl HostEventType {
    pub const fn from_raw(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::PointerMoved),
            2 => Some(Self::PointerDown),
            3 => Some(Self::PointerUp),
            4 => Some(Self::Scroll),
            5 => Some(Self::KeyDown),
            _ => None,
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostPointerButton {
    None = 0,
    Primary = 1,
    Secondary = 2,
    Middle = 3,
}

impl HostPointerButton {
    pub const fn from_raw(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::Primary),
            2 => Some(Self::Secondary),
            3 => Some(Self::Middle),
            _ => None,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HostEvent {
    pub event_type: u32,
    pub struct_size: u32,
    pub x: i32,
    pub y: i32,
    pub button: u32,
    /// Zero for every pointer event. For `HostEventType::Scroll` it holds the
    /// signed scroll amount in notches (an `i32` stored as its `u32` bit
    /// pattern), which keeps the struct's size and layout unchanged: a host or
    /// app that predates scrolling never sets or reads it.
    pub reserved: u32,
}

impl HostEvent {
    /// The character a `HostEventType::KeyDown` event types, if any.
    pub fn key_character(&self) -> Option<char> {
        char::from_u32(self.reserved).filter(|&character| character != '\0')
    }

    /// The scroll amount of a `HostEventType::Scroll` event, in notches.
    pub const fn scroll_delta(&self) -> i32 {
        self.reserved as i32
    }

    pub const fn empty() -> Self {
        Self {
            event_type: HostEventType::None as u32,
            struct_size: size_of::<Self>() as u32,
            x: 0,
            y: 0,
            button: HostPointerButton::None as u32,
            reserved: 0,
        }
    }
}

pub type GetSurfaceFn = unsafe extern "C" fn(
    context: *mut c_void,
    surface: *mut SurfaceDescriptor,
) -> u32;

pub type PresentFn = unsafe extern "C" fn(
    context: *mut c_void,
    damage: *const DamageRect,
) -> u32;

pub type PollEventFn = unsafe extern "C" fn(
    context: *mut c_void,
    event: *mut HostEvent,
) -> u32;

/// Seconds since 1970-01-01 00:00:00 UTC, written through `unix_seconds`.
pub type GetTimeFn = unsafe extern "C" fn(
    context: *mut c_void,
    unix_seconds: *mut u64,
) -> u32;

/// List up to `capacity` entries of the directory at `path` (`path_len`
/// bytes, not necessarily NUL-terminated) into `entries`. `*count_out`
/// receives the number actually written; `*truncated_out` is set when the
/// directory held more than `capacity` entries. Returns a
/// `UI_SERVICE_FS_STATUS_*`-shaped code (0 = ok, 1 = not found, 2 = not a
/// directory, other = error) -- see `ui_core::fs::Error`, which decodes it.
pub type ListDirectoryFn = unsafe extern "C" fn(
    context: *mut c_void,
    path: *const u8,
    path_len: u32,
    entries: *mut ui_core::fs::DirEntry,
    capacity: u32,
    count_out: *mut u32,
    truncated_out: *mut bool,
) -> u32;

#[repr(C)]
pub struct HostV1 {
    pub header: AbiHeader,
    pub capabilities: u64,
    pub context: *mut c_void,
    pub get_surface: Option<GetSurfaceFn>,
    pub present: Option<PresentFn>,
}

impl HostV1 {
    pub const fn expected_size() -> u32 {
        size_of::<Self>() as u32
    }

    pub fn validate(&self) -> Result<(), Status> {
        if self.header.abi_version != UI_SERVICE_ABI_VERSION_V1 {
            return Err(Status::BadVersion);
        }

        if self.header.struct_size < Self::expected_size() {
            return Err(Status::InvalidArgument);
        }

        validate_common(self.capabilities, self.get_surface, self.present)
    }
}

#[repr(C)]
pub struct HostV2 {
    pub header: AbiHeader,
    pub capabilities: u64,
    pub context: *mut c_void,
    pub get_surface: Option<GetSurfaceFn>,
    pub present: Option<PresentFn>,
    pub poll_event: Option<PollEventFn>,
}

impl HostV2 {
    pub const fn expected_size() -> u32 {
        size_of::<Self>() as u32
    }

    pub fn validate(&self) -> Result<(), Status> {
        if self.header.abi_version != UI_SERVICE_ABI_VERSION_V2 {
            return Err(Status::BadVersion);
        }

        if self.header.struct_size < Self::expected_size() {
            return Err(Status::InvalidArgument);
        }

        validate_common(self.capabilities, self.get_surface, self.present)?;

        if self.capabilities & UI_SERVICE_HOST_CAP_INPUT != 0 && self.poll_event.is_none() {
            return Err(Status::InvalidArgument);
        }

        Ok(())
    }
}

#[repr(C)]
pub struct HostV3 {
    pub header: AbiHeader,
    pub capabilities: u64,
    pub context: *mut c_void,
    pub get_surface: Option<GetSurfaceFn>,
    pub present: Option<PresentFn>,
    pub poll_event: Option<PollEventFn>,
    pub get_time: Option<GetTimeFn>,
}

impl HostV3 {
    pub const fn expected_size() -> u32 {
        size_of::<Self>() as u32
    }

    pub fn validate(&self) -> Result<(), Status> {
        if self.header.abi_version != UI_SERVICE_ABI_VERSION_V3 {
            return Err(Status::BadVersion);
        }

        if self.header.struct_size < Self::expected_size() {
            return Err(Status::InvalidArgument);
        }

        validate_common(self.capabilities, self.get_surface, self.present)?;

        if self.capabilities & UI_SERVICE_HOST_CAP_INPUT != 0 && self.poll_event.is_none() {
            return Err(Status::InvalidArgument);
        }

        if self.capabilities & UI_SERVICE_HOST_CAP_TIME != 0 && self.get_time.is_none() {
            return Err(Status::InvalidArgument);
        }

        Ok(())
    }
}

#[repr(C)]
pub struct HostV4 {
    pub header: AbiHeader,
    pub capabilities: u64,
    pub context: *mut c_void,
    pub get_surface: Option<GetSurfaceFn>,
    pub present: Option<PresentFn>,
    pub poll_event: Option<PollEventFn>,
    pub get_time: Option<GetTimeFn>,
    /// Physical pixels per design point, as thousandths (e.g. `2000` for a
    /// genuine 2x-Retina host, `1000` for a 1x/non-Retina one). `0` means
    /// "unknown"; UIService keeps its historical 2x default in that case.
    pub content_scale_permille: u32,
}

impl HostV4 {
    pub const fn expected_size() -> u32 {
        size_of::<Self>() as u32
    }

    pub fn validate(&self) -> Result<(), Status> {
        if self.header.abi_version != UI_SERVICE_ABI_VERSION_V4 {
            return Err(Status::BadVersion);
        }

        if self.header.struct_size < Self::expected_size() {
            return Err(Status::InvalidArgument);
        }

        validate_common(self.capabilities, self.get_surface, self.present)?;

        if self.capabilities & UI_SERVICE_HOST_CAP_INPUT != 0 && self.poll_event.is_none() {
            return Err(Status::InvalidArgument);
        }

        if self.capabilities & UI_SERVICE_HOST_CAP_TIME != 0 && self.get_time.is_none() {
            return Err(Status::InvalidArgument);
        }

        Ok(())
    }
}

#[repr(C)]
pub struct HostV5 {
    pub header: AbiHeader,
    pub capabilities: u64,
    pub context: *mut c_void,
    pub get_surface: Option<GetSurfaceFn>,
    pub present: Option<PresentFn>,
    pub poll_event: Option<PollEventFn>,
    pub get_time: Option<GetTimeFn>,
    pub content_scale_permille: u32,
    /// `None`, with `UI_SERVICE_HOST_CAP_FS` unset, on a host with no
    /// filesystem to offer -- an app that needs this treats that as
    /// "unavailable", not an error.
    pub list_directory: Option<ListDirectoryFn>,
}

impl HostV5 {
    pub const fn expected_size() -> u32 {
        size_of::<Self>() as u32
    }

    pub fn validate(&self) -> Result<(), Status> {
        if self.header.abi_version != UI_SERVICE_ABI_VERSION_V5 {
            return Err(Status::BadVersion);
        }

        if self.header.struct_size < Self::expected_size() {
            return Err(Status::InvalidArgument);
        }

        validate_common(self.capabilities, self.get_surface, self.present)?;

        if self.capabilities & UI_SERVICE_HOST_CAP_INPUT != 0 && self.poll_event.is_none() {
            return Err(Status::InvalidArgument);
        }

        if self.capabilities & UI_SERVICE_HOST_CAP_TIME != 0 && self.get_time.is_none() {
            return Err(Status::InvalidArgument);
        }

        if self.capabilities & UI_SERVICE_HOST_CAP_FS != 0 && self.list_directory.is_none() {
            return Err(Status::InvalidArgument);
        }

        Ok(())
    }
}

pub const UI_SERVICE_LOGIN_ABI_VERSION_V1: u32 = 1;
pub const UI_SERVICE_PASSCODE_MIN: u32 = 4;
pub const UI_SERVICE_PASSCODE_MAX: u32 = 64;

/// Answers of the login host's passcode callbacks (`UI_SERVICE_AUTH_*`).
#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthResult {
    Ok = 0,
    Denied = 1,
    RetryLater = 2,
    Locked = 3,
    Unavailable = 4,
    NotSet = 5,
    Invalid = 6,
    Error = 7,
}

impl AuthResult {
    /// Unknown values count as `Error`: never as a success.
    pub const fn from_raw(value: u32) -> Self {
        match value {
            0 => Self::Ok,
            1 => Self::Denied,
            2 => Self::RetryLater,
            3 => Self::Locked,
            4 => Self::Unavailable,
            5 => Self::NotSet,
            6 => Self::Invalid,
            _ => Self::Error,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AuthStatus {
    pub passcode_set: u32,
    pub failures: u32,
    pub locked: u32,
    pub wait_seconds: u32,
}

pub type AuthStatusFn = unsafe extern "C" fn(context: *mut c_void, status: *mut AuthStatus) -> u32;
pub type AuthVerifyFn = unsafe extern "C" fn(
    context: *mut c_void,
    passcode: *const u8,
    length: u32,
    wait_seconds: *mut u32,
) -> u32;
pub type AuthSetFn = unsafe extern "C" fn(
    context: *mut c_void,
    old_passcode: *const u8,
    old_length: u32,
    passcode: *const u8,
    length: u32,
    wait_seconds: *mut u32,
) -> u32;

/// The login screen's host table (`UIServiceLoginHostV1`): separate from the
/// app host, so only the login screen can reach the passcode checker.
#[repr(C)]
pub struct LoginHostV1 {
    pub header: AbiHeader,
    pub context: *mut c_void,
    pub auth_status: Option<AuthStatusFn>,
    pub auth_verify: Option<AuthVerifyFn>,
    pub auth_set: Option<AuthSetFn>,
}

impl LoginHostV1 {
    pub const fn expected_size() -> u32 {
        size_of::<Self>() as u32
    }

    pub fn validate(&self) -> Result<(), Status> {
        if self.header.abi_version != UI_SERVICE_LOGIN_ABI_VERSION_V1 {
            return Err(Status::BadVersion);
        }
        if self.header.struct_size < Self::expected_size() {
            return Err(Status::InvalidArgument);
        }
        if self.auth_status.is_none() || self.auth_verify.is_none() || self.auth_set.is_none() {
            return Err(Status::InvalidArgument);
        }
        Ok(())
    }
}

fn validate_common(
    capabilities: u64,
    get_surface: Option<GetSurfaceFn>,
    present: Option<PresentFn>,
) -> Result<(), Status> {
    if get_surface.is_none() {
        return Err(Status::InvalidArgument);
    }

    if capabilities & UI_SERVICE_HOST_CAP_PRESENT != 0 && present.is_none() {
        return Err(Status::InvalidArgument);
    }

    Ok(())
}

const _: [(); 8] = [(); size_of::<AbiHeader>()];
const _: [(); 16] = [(); size_of::<AuthStatus>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 40] = [(); size_of::<LoginHostV1>()];
const _: [(); 16] = [(); size_of::<DamageRect>()];
const _: [(); 24] = [(); size_of::<HostEvent>()];

#[cfg(target_pointer_width = "64")]
const _: [(); 24] = [(); size_of::<SurfaceDescriptor>()];

#[cfg(target_pointer_width = "64")]
const _: [(); 40] = [(); size_of::<HostV1>()];

#[cfg(target_pointer_width = "64")]
const _: [(); 48] = [(); size_of::<HostV2>()];

#[cfg(target_pointer_width = "64")]
const _: [(); 56] = [(); size_of::<HostV3>()];

#[cfg(target_pointer_width = "64")]
const _: [(); 64] = [(); size_of::<HostV4>()];

#[cfg(target_pointer_width = "64")]
const _: [(); 72] = [(); size_of::<HostV5>()];
