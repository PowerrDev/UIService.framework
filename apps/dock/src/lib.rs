#![no_std]

//! The Dock's logic: its configuration and the apps' `Info.plist`s
//! ([`plist`]), their icons ([`image`]) and where each tile goes
//! ([`layout`]). The process that shows it is `bundles/dock`.

#[cfg(test)]
extern crate std;

pub mod layout;
pub mod render;

pub use ui_bundle::{image, plist};
