//! The login screen on NXU: `ui_login::LoginScreen` over the host surface as
//! one fullscreen WindowServer background window, with the passcode checked
//! through the login host table (`UIServiceLoginHostV1`).

use ui_abi::{AuthResult, AuthStatus, HostV5, LoginHostV1, Status};
use ui_core::{Event, Point, PointerButton, Rect, Size};
use ui_login::{Backdrop, LoginScreen, PasscodeChecker};
use ui_platform::InteractiveHostV5;
use ui_render::Surface;

use crate::storage::{StaticCell, TEXT_SCRATCH};
use crate::{cursor, scene, text, windowserver};

unsafe extern "C" {
    /// NXU's monotonic microsecond counter (see runtime.rs).
    fn timer_get_microseconds() -> u64;
}

/// The blurred backdrop is kept at a quarter of the screen's resolution;
/// these hold it for screens up to 1920x1200. A larger screen gets a flat
/// backdrop instead.
const BACKDROP_CAPACITY: usize = (1920 / 4) * (1200 / 4);
static BACKDROP: StaticCell<[u32; BACKDROP_CAPACITY]> = StaticCell::new([0; BACKDROP_CAPACITY]);
static BACKDROP_TEMP: StaticCell<[u32; BACKDROP_CAPACITY]> = StaticCell::new([0; BACKDROP_CAPACITY]);

/// The checker behind the login host table. Every call's answer is decoded
/// with `AuthResult::from_raw`, so anything unexpected is an error, never OK.
struct HostChecker<'a> {
    login: &'a LoginHostV1,
}

impl PasscodeChecker for HostChecker<'_> {
    fn status(&mut self) -> Option<AuthStatus> {
        let function = self.login.auth_status?;
        let mut status = AuthStatus::default();
        let result = unsafe { function(self.login.context, &mut status) };
        (AuthResult::from_raw(result) == AuthResult::Ok).then_some(status)
    }

    fn verify(&mut self, passcode: &[u8]) -> (AuthResult, u32) {
        let Some(function) = self.login.auth_verify else {
            return (AuthResult::Error, 0);
        };
        let mut wait = 0u32;
        let result = unsafe { function(self.login.context, passcode.as_ptr(), passcode.len() as u32, &mut wait) };
        (AuthResult::from_raw(result), wait)
    }

    fn set(&mut self, passcode: &[u8]) -> (AuthResult, u32) {
        let Some(function) = self.login.auth_set else {
            return (AuthResult::Error, 0);
        };
        let mut wait = 0u32;
        let result = unsafe {
            function(self.login.context, core::ptr::null(), 0, passcode.as_ptr(), passcode.len() as u32, &mut wait)
        };
        (AuthResult::from_raw(result), wait)
    }
}

fn now_us() -> u64 {
    unsafe { timer_get_microseconds() }
}

/// Repaint what changed into the host surface and hand it to WindowServer.
unsafe fn paint(
    host: &mut InteractiveHostV5<'_>,
    screen: &mut LoginScreen,
    backdrop: &Backdrop<'_>,
    window: windowserver::WindowId,
    size: Size,
    now: u64,
) -> Result<(), Status> {
    let region = screen.take_repaint_region(size, now);
    let (pixels, width, height, stride) = unsafe {
        host.with_surface(|surface| {
            paint_surface(surface, screen, backdrop, region, now);
            (surface.pixels_mut().as_ptr(), surface.width(), surface.height(), surface.stride())
        })?
    };
    let rendered = windowserver::Render_Window(
        window,
        unsafe { core::slice::from_raw_parts(pixels, height as usize * stride as usize) },
        width,
        height,
        stride,
    );
    if rendered { Ok(()) } else { Err(Status::PresentFailed) }
}

fn paint_surface(surface: &mut Surface<'_>, screen: &LoginScreen, backdrop: &Backdrop<'_>, region: Rect, now: u64) {
    let scratch = unsafe { TEXT_SCRATCH.get_mut() };
    if screen.wants_display_face() && text::has_display() {
        if let Some(mut borel) = text::display(scratch) {
            screen.paint(surface, backdrop, &mut borel, region, now);
        }
    } else {
        let mut inter = text::backend(scratch);
        screen.paint(surface, backdrop, &mut inter, region, now);
    }
}

/// Run the setup/login screen until the checker accepts a passcode.
pub(crate) unsafe fn run(host_abi: &HostV5, login: &LoginHostV1) -> Result<(), Status> {
    login.validate()?;
    let mut host = InteractiveHostV5::connect(host_abi)?;
    let mut checker = HostChecker { login };

    // Blur the wallpaper once: drawn into the host surface, averaged down.
    let low = unsafe { BACKDROP.get_mut() };
    let temp = unsafe { BACKDROP_TEMP.get_mut() };
    let (size, built) = unsafe {
        host.with_surface(|surface| {
            scene::UIDrawWallpaper(surface);
            let size = Size::new(surface.width(), surface.height());
            (size, Backdrop::build(surface, &mut low[..], &mut temp[..]))
        })?
    };
    let backdrop = built.unwrap_or(Backdrop::plain());

    let window = windowserver::Create_Background_Window(Rect::new(0, 0, size.width, size.height))
        .ok_or(Status::NoSurface)?;
    cursor::install_system_cursors();
    let center = Point::new((size.width / 2) as i32, (size.height / 2) as i32);
    let _ = windowserver::Pointer_Move(center.x, center.y);

    let mut screen = LoginScreen::new();
    screen.start(&mut checker, now_us());

    let result = (|| -> Result<(), Status> {
        unsafe { paint(&mut host, &mut screen, &backdrop, window, size, now_us())? };
        let _ = windowserver::Present();

        loop {
            let now = now_us();
            let mut dirty = false;

            while let Some(event) = host.poll_event()? {
                dirty |= match event {
                    Event::PointerDown { position, button: PointerButton::Primary } => screen.click(position, size, now),
                    other => screen.event(other, now),
                };
            }
            dirty |= screen.tick(&mut checker, now);

            if dirty {
                unsafe { paint(&mut host, &mut screen, &backdrop, window, size, now)? };
            }
            let _ = windowserver::Present();

            // The checker call blocks (tepOS runs Argon2id): "Checking..." is
            // on screen before it starts.
            if screen.has_pending() {
                screen.run_pending(&mut checker, now_us());
                unsafe { paint(&mut host, &mut screen, &backdrop, window, size, now_us())? };
                let _ = windowserver::Present();
            }

            if screen.finished() {
                return Ok(());
            }
            core::hint::spin_loop();
        }
    })();

    windowserver::Destroy_Window(window);
    result
}
