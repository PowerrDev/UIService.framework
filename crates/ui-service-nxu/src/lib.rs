#![no_std]

//! NXU adapter for UIService.framework.
//!
//! This crate is not an application API. It packages the stable C host ABI,
//! text bootstrap and the temporary early-boot app runtime into the static
//! library linked by NXU.

use core::panic::PanicInfo;

mod abi_exports;
mod aqua;
mod clock;
mod cursor;
mod demo;
mod login;
mod runtime;
mod scene;
mod storage;
mod surface;
mod text;
mod windowserver;

// Pull the WindowServer NXU backend into this single final static library.
use windowserver_nxu as _;

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
