#![no_std]

//! Rust wrappers around the stable NXU/UIService host ABI.

use core::ffi::c_void;
use core::slice;

use ui_abi::{
    DamageRect, GetSurfaceFn, HostEvent, HostEventType, HostPointerButton, HostV1, HostV2, HostV3, HostV4, HostV5,
    ListDirectoryFn, PresentFn, Status, SurfaceDescriptor, UI_SERVICE_HOST_CAP_DAMAGE, UI_SERVICE_HOST_CAP_FS,
    UI_SERVICE_HOST_CAP_INPUT, UI_SERVICE_HOST_CAP_PRESENT, UI_SERVICE_HOST_CAP_TIME,
};
use ui_core::{Event, Point, PointerButton, Size};
use ui_render::{Canvas, Surface};

pub trait Platform {
    type Canvas<'a>: Canvas
    where
        Self: 'a;

    fn surface_size(&self) -> Size;
    fn begin_frame(&mut self) -> Self::Canvas<'_>;
    fn poll_event(&mut self) -> Option<Event>;
    fn present(&mut self);
}

/*
 * Safe-ish Rust facade over the stable C host table in ui-abi.
 *
 * Host is the ABI-v1 display-only connection. InteractiveHost extends the
 * same ownership model with ABI-v2 input polling, InteractiveHostV3 extends
 * it again with ABI-v3 wall-clock time, InteractiveHostV4 adds the host's
 * real content scale (physical pixels per design point) so UI no longer has
 * to assume every canvas is a fixed 2x-Retina target, and InteractiveHostV5
 * adds an optional read-only filesystem capability (wired into
 * `ui_core::fs`, not exposed directly here, since an app has no host handle
 * of its own to call through). NXU owns the devices, framebuffer, event
 * source, RTC and filesystem; UI only borrows them through callbacks.
 */
pub struct Host<'a> {
    abi: &'a HostV1,
}

impl<'a> Host<'a> {
    pub fn connect(abi: &'a HostV1) -> Result<Self, Status> {
        abi.validate()?;
        Ok(Self { abi })
    }

    pub fn capabilities(&self) -> u64 {
        self.abi.capabilities
    }

    pub unsafe fn with_surface<R>(
        &mut self,
        f: impl FnOnce(&mut Surface<'_>) -> R,
    ) -> Result<R, Status> {
        unsafe { with_surface(self.abi.context, self.abi.get_surface, f) }
    }

    pub fn present(&mut self, damage: Option<DamageRect>) -> Result<(), Status> {
        present(
            self.abi.context,
            self.abi.capabilities,
            self.abi.present,
            damage,
        )
    }
}

pub struct InteractiveHost<'a> {
    abi: &'a HostV2,
}

impl<'a> InteractiveHost<'a> {
    pub fn connect(abi: &'a HostV2) -> Result<Self, Status> {
        abi.validate()?;
        Ok(Self { abi })
    }

    pub fn capabilities(&self) -> u64 {
        self.abi.capabilities
    }

    pub unsafe fn with_surface<R>(
        &mut self,
        f: impl FnOnce(&mut Surface<'_>) -> R,
    ) -> Result<R, Status> {
        unsafe { with_surface(self.abi.context, self.abi.get_surface, f) }
    }

    pub fn present(&mut self, damage: Option<DamageRect>) -> Result<(), Status> {
        present(
            self.abi.context,
            self.abi.capabilities,
            self.abi.present,
            damage,
        )
    }

    pub fn poll_event(&mut self) -> Result<Option<Event>, Status> {
        if self.abi.capabilities & UI_SERVICE_HOST_CAP_INPUT == 0 {
            return Ok(None);
        }

        let poll_event = self.abi.poll_event.ok_or(Status::InvalidArgument)?;
        let mut raw = HostEvent::empty();
        let raw_status = unsafe { poll_event(self.abi.context, &mut raw) };
        let status = Status::from_raw(raw_status).ok_or(Status::Unsupported)?;
        if !status.is_ok() {
            return Err(status);
        }

        if raw.struct_size < core::mem::size_of::<HostEvent>() as u32 {
            return Err(Status::InvalidArgument);
        }

        let event_type = HostEventType::from_raw(raw.event_type).ok_or(Status::Unsupported)?;
        let position = Point::new(raw.x, raw.y);

        match event_type {
            HostEventType::None => Ok(None),
            HostEventType::PointerMoved => Ok(Some(Event::PointerMoved { position })),
            HostEventType::PointerDown => Ok(Some(Event::PointerDown {
                position,
                button: map_button(raw.button)?,
            })),
            HostEventType::PointerUp => Ok(Some(Event::PointerUp {
                position,
                button: map_button(raw.button)?,
            })),
            HostEventType::Scroll => Ok(Some(Event::Scroll {
                position,
                delta: raw.scroll_delta(),
            })),
        }
    }
}

pub struct InteractiveHostV3<'a> {
    abi: &'a HostV3,
}

impl<'a> InteractiveHostV3<'a> {
    pub fn connect(abi: &'a HostV3) -> Result<Self, Status> {
        abi.validate()?;
        Ok(Self { abi })
    }

    pub fn capabilities(&self) -> u64 {
        self.abi.capabilities
    }

    pub unsafe fn with_surface<R>(
        &mut self,
        f: impl FnOnce(&mut Surface<'_>) -> R,
    ) -> Result<R, Status> {
        unsafe { with_surface(self.abi.context, self.abi.get_surface, f) }
    }

    pub fn present(&mut self, damage: Option<DamageRect>) -> Result<(), Status> {
        present(
            self.abi.context,
            self.abi.capabilities,
            self.abi.present,
            damage,
        )
    }

    pub fn poll_event(&mut self) -> Result<Option<Event>, Status> {
        if self.abi.capabilities & UI_SERVICE_HOST_CAP_INPUT == 0 {
            return Ok(None);
        }

        let poll_event = self.abi.poll_event.ok_or(Status::InvalidArgument)?;
        let mut raw = HostEvent::empty();
        let raw_status = unsafe { poll_event(self.abi.context, &mut raw) };
        let status = Status::from_raw(raw_status).ok_or(Status::Unsupported)?;
        if !status.is_ok() {
            return Err(status);
        }

        if raw.struct_size < core::mem::size_of::<HostEvent>() as u32 {
            return Err(Status::InvalidArgument);
        }

        let event_type = HostEventType::from_raw(raw.event_type).ok_or(Status::Unsupported)?;
        let position = Point::new(raw.x, raw.y);

        match event_type {
            HostEventType::None => Ok(None),
            HostEventType::PointerMoved => Ok(Some(Event::PointerMoved { position })),
            HostEventType::PointerDown => Ok(Some(Event::PointerDown {
                position,
                button: map_button(raw.button)?,
            })),
            HostEventType::PointerUp => Ok(Some(Event::PointerUp {
                position,
                button: map_button(raw.button)?,
            })),
            HostEventType::Scroll => Ok(Some(Event::Scroll {
                position,
                delta: raw.scroll_delta(),
            })),
        }
    }

    /// Current wall-clock time as seconds since the Unix epoch (UTC), or
    /// `None` when this host has no RTC to report it.
    pub fn get_time(&mut self) -> Result<Option<u64>, Status> {
        if self.abi.capabilities & UI_SERVICE_HOST_CAP_TIME == 0 {
            return Ok(None);
        }

        let get_time = self.abi.get_time.ok_or(Status::InvalidArgument)?;
        let mut unix_seconds: u64 = 0;
        let raw_status = unsafe { get_time(self.abi.context, &mut unix_seconds) };
        let status = Status::from_raw(raw_status).ok_or(Status::Unsupported)?;
        if !status.is_ok() {
            return Err(status);
        }

        Ok(Some(unix_seconds))
    }
}

pub struct InteractiveHostV4<'a> {
    abi: &'a HostV4,
}

impl<'a> InteractiveHostV4<'a> {
    /// Connects and, on success, records the host's real content scale in
    /// `ui_core::scale` for every subsequent draw call to read.
    pub fn connect(abi: &'a HostV4) -> Result<Self, Status> {
        abi.validate()?;
        ui_core::scale::set_permille(abi.content_scale_permille);
        Ok(Self { abi })
    }

    pub fn capabilities(&self) -> u64 {
        self.abi.capabilities
    }

    pub unsafe fn with_surface<R>(
        &mut self,
        f: impl FnOnce(&mut Surface<'_>) -> R,
    ) -> Result<R, Status> {
        unsafe { with_surface(self.abi.context, self.abi.get_surface, f) }
    }

    pub fn present(&mut self, damage: Option<DamageRect>) -> Result<(), Status> {
        present(
            self.abi.context,
            self.abi.capabilities,
            self.abi.present,
            damage,
        )
    }

    pub fn poll_event(&mut self) -> Result<Option<Event>, Status> {
        if self.abi.capabilities & UI_SERVICE_HOST_CAP_INPUT == 0 {
            return Ok(None);
        }

        let poll_event = self.abi.poll_event.ok_or(Status::InvalidArgument)?;
        let mut raw = HostEvent::empty();
        let raw_status = unsafe { poll_event(self.abi.context, &mut raw) };
        let status = Status::from_raw(raw_status).ok_or(Status::Unsupported)?;
        if !status.is_ok() {
            return Err(status);
        }

        if raw.struct_size < core::mem::size_of::<HostEvent>() as u32 {
            return Err(Status::InvalidArgument);
        }

        let event_type = HostEventType::from_raw(raw.event_type).ok_or(Status::Unsupported)?;
        let position = Point::new(raw.x, raw.y);

        match event_type {
            HostEventType::None => Ok(None),
            HostEventType::PointerMoved => Ok(Some(Event::PointerMoved { position })),
            HostEventType::PointerDown => Ok(Some(Event::PointerDown {
                position,
                button: map_button(raw.button)?,
            })),
            HostEventType::PointerUp => Ok(Some(Event::PointerUp {
                position,
                button: map_button(raw.button)?,
            })),
            HostEventType::Scroll => Ok(Some(Event::Scroll {
                position,
                delta: raw.scroll_delta(),
            })),
        }
    }

    /// Current wall-clock time as seconds since the Unix epoch (UTC), or
    /// `None` when this host has no RTC to report it.
    pub fn get_time(&mut self) -> Result<Option<u64>, Status> {
        if self.abi.capabilities & UI_SERVICE_HOST_CAP_TIME == 0 {
            return Ok(None);
        }

        let get_time = self.abi.get_time.ok_or(Status::InvalidArgument)?;
        let mut unix_seconds: u64 = 0;
        let raw_status = unsafe { get_time(self.abi.context, &mut unix_seconds) };
        let status = Status::from_raw(raw_status).ok_or(Status::Unsupported)?;
        if !status.is_ok() {
            return Err(status);
        }

        Ok(Some(unix_seconds))
    }
}

pub struct InteractiveHostV5<'a> {
    abi: &'a HostV5,
}

impl<'a> InteractiveHostV5<'a> {
    /// Connects and, on success, records the host's real content scale in
    /// `ui_core::scale` and wires up its filesystem callback (if any) in
    /// `ui_core::fs`, for every subsequent draw/app call to read.
    pub fn connect(abi: &'a HostV5) -> Result<Self, Status> {
        abi.validate()?;
        ui_core::scale::set_permille(abi.content_scale_permille);
        let function = if abi.capabilities & UI_SERVICE_HOST_CAP_FS != 0 {
            abi.list_directory
        } else {
            None
        };
        // SAFETY: `ListDirectoryFn` and `ui_core::fs`'s private raw fn type
        // have identical signatures (same parameter/return types, `extern
        // "C"`) -- this only renames the type for `ui_core::fs`'s own use,
        // it does not reinterpret the pointer.
        let function = function.map(|f| unsafe { core::mem::transmute::<ListDirectoryFn, _>(f) });
        ui_core::fs::set_backend(abi.context, function);
        Ok(Self { abi })
    }

    pub fn capabilities(&self) -> u64 {
        self.abi.capabilities
    }

    pub unsafe fn with_surface<R>(
        &mut self,
        f: impl FnOnce(&mut Surface<'_>) -> R,
    ) -> Result<R, Status> {
        unsafe { with_surface(self.abi.context, self.abi.get_surface, f) }
    }

    pub fn present(&mut self, damage: Option<DamageRect>) -> Result<(), Status> {
        present(
            self.abi.context,
            self.abi.capabilities,
            self.abi.present,
            damage,
        )
    }

    pub fn poll_event(&mut self) -> Result<Option<Event>, Status> {
        if self.abi.capabilities & UI_SERVICE_HOST_CAP_INPUT == 0 {
            return Ok(None);
        }

        let poll_event = self.abi.poll_event.ok_or(Status::InvalidArgument)?;
        let mut raw = HostEvent::empty();
        let raw_status = unsafe { poll_event(self.abi.context, &mut raw) };
        let status = Status::from_raw(raw_status).ok_or(Status::Unsupported)?;
        if !status.is_ok() {
            return Err(status);
        }

        if raw.struct_size < core::mem::size_of::<HostEvent>() as u32 {
            return Err(Status::InvalidArgument);
        }

        let event_type = HostEventType::from_raw(raw.event_type).ok_or(Status::Unsupported)?;
        let position = Point::new(raw.x, raw.y);

        match event_type {
            HostEventType::None => Ok(None),
            HostEventType::PointerMoved => Ok(Some(Event::PointerMoved { position })),
            HostEventType::PointerDown => Ok(Some(Event::PointerDown {
                position,
                button: map_button(raw.button)?,
            })),
            HostEventType::PointerUp => Ok(Some(Event::PointerUp {
                position,
                button: map_button(raw.button)?,
            })),
            HostEventType::Scroll => Ok(Some(Event::Scroll {
                position,
                delta: raw.scroll_delta(),
            })),
        }
    }

    /// Current wall-clock time as seconds since the Unix epoch (UTC), or
    /// `None` when this host has no RTC to report it.
    pub fn get_time(&mut self) -> Result<Option<u64>, Status> {
        if self.abi.capabilities & UI_SERVICE_HOST_CAP_TIME == 0 {
            return Ok(None);
        }

        let get_time = self.abi.get_time.ok_or(Status::InvalidArgument)?;
        let mut unix_seconds: u64 = 0;
        let raw_status = unsafe { get_time(self.abi.context, &mut unix_seconds) };
        let status = Status::from_raw(raw_status).ok_or(Status::Unsupported)?;
        if !status.is_ok() {
            return Err(status);
        }

        Ok(Some(unix_seconds))
    }
}

fn map_button(raw: u32) -> Result<PointerButton, Status> {
    match HostPointerButton::from_raw(raw) {
        Some(HostPointerButton::Primary) => Ok(PointerButton::Primary),
        Some(HostPointerButton::Secondary) => Ok(PointerButton::Secondary),
        Some(HostPointerButton::Middle) => Ok(PointerButton::Middle),
        _ => Err(Status::Unsupported),
    }
}

unsafe fn with_surface<R>(
    context: *mut c_void,
    get_surface: Option<GetSurfaceFn>,
    f: impl FnOnce(&mut Surface<'_>) -> R,
) -> Result<R, Status> {
    let mut descriptor = SurfaceDescriptor::empty();
    let get_surface = get_surface.ok_or(Status::InvalidArgument)?;
    let raw_status = unsafe { get_surface(context, &mut descriptor) };
    let status = Status::from_raw(raw_status).ok_or(Status::Unsupported)?;
    if !status.is_ok() {
        return Err(status);
    }

    if !descriptor.is_valid() {
        return Err(Status::NoSurface);
    }

    let pixel_count = (descriptor.stride_pixels as usize)
        .checked_mul(descriptor.height as usize)
        .ok_or(Status::InvalidArgument)?;
    let pixels = unsafe { slice::from_raw_parts_mut(descriptor.pixels, pixel_count) };
    let mut surface = Surface::new(
        pixels,
        descriptor.width,
        descriptor.height,
        descriptor.stride_pixels,
    )
    .ok_or(Status::NoSurface)?;

    Ok(f(&mut surface))
}

fn present(
    context: *mut c_void,
    capabilities: u64,
    present: Option<PresentFn>,
    damage: Option<DamageRect>,
) -> Result<(), Status> {
    if capabilities & UI_SERVICE_HOST_CAP_PRESENT == 0 {
        return Ok(());
    }

    let present = present.ok_or(Status::InvalidArgument)?;
    let damage_ptr = if capabilities & UI_SERVICE_HOST_CAP_DAMAGE != 0 {
        damage
            .as_ref()
            .map_or(core::ptr::null(), |rect| rect as *const DamageRect)
    } else {
        core::ptr::null()
    };
    let raw_status = unsafe { present(context, damage_ptr) };
    let status = Status::from_raw(raw_status).ok_or(Status::Unsupported)?;
    if status.is_ok() {
        Ok(())
    } else {
        Err(status)
    }
}
