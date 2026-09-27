#![no_std]

//! The runtime of a sevOS app on NXU: what turns a `ui_app::App` into a
//! process in `/Applications/<Name>.app/Contents/SevOS/<Name>`.
//!
//! An app's bundle crate is a static library exporting `UIApplicationMain`,
//! which calls [`run`]; NXU links it with the userland C library and
//! AppKit.framework's `main`. The desktop itself stays in the kernel
//! (UIService's `ui-service-nxu`): this crate talks to it through the UI
//! session system calls, and draws nothing but the app's content.

pub mod backends;
pub mod connection;
mod runtime;
pub mod sys;
pub mod text;

pub use runtime::run;

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    sys::log("ui-app-nxu: panic");
    if let Some(location) = info.location() {
        let mut line = [0u8; 20];
        sys::log_line(&[" at ", location.file(), ":", sys::decimal(location.line() as u64, &mut line)]);
    } else {
        sys::log("\n");
    }
    sys::exit(101)
}
