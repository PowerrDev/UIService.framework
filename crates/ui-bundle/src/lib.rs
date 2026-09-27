#![no_std]

//! App bundles' metadata and icons, without an allocator: [`plist`] reads
//! `Info.plist` and other XML property lists, [`image`] picks, decodes and
//! scales an app's `.icns` icon.

#[cfg(test)]
extern crate std;

pub mod image;
pub mod plist;
