#![no_std]

//! Lightweight metadata types used by applications and system UI.

/// Stable metadata describing an application.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppInfo<'a> {
    pub name: &'a str,
    pub identifier: &'a str,
    pub version: &'a str,
}

impl<'a> AppInfo<'a> {
    pub const fn new(name: &'a str, identifier: &'a str, version: &'a str) -> Self {
        Self {
            name,
            identifier,
            version,
        }
    }
}

/// Compatibility alias for code written against UI 0.3.
pub type ApplicationInfo<'a> = AppInfo<'a>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SystemInfo<'a> {
    pub product_name: &'a str,
    pub version: &'a str,
    pub build: &'a str,
    pub kernel: &'a str,
    pub copyright: &'a str,
}

impl<'a> SystemInfo<'a> {
    pub const fn new(
        product_name: &'a str,
        version: &'a str,
        build: &'a str,
        kernel: &'a str,
        copyright: &'a str,
    ) -> Self {
        Self {
            product_name,
            version,
            build,
            kernel,
            copyright,
        }
    }
}
