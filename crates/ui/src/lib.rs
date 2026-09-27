#![no_std]

//! Application-facing umbrella crate for UIService.framework.
//!
//! Most apps only need:
//!
//! ```ignore
//! use ui::prelude::*;
//! ```
//!
//! Low-level NXU host ABI details remain available through [`abi`] and
//! [`platform`], but are intentionally not part of the normal app prelude.

pub use ui_abi as abi;
pub use ui_app as application;
pub use ui_assets as assets;
pub use ui_core as core;
pub use ui_platform as platform;
pub use ui_render as render;
pub use ui_support as support;
pub use ui_text as text;
pub use ui_widgets as widgets;
pub use ui_window as window;

/// Common imports for application code.
pub mod prelude {
    pub use ui_app::{App, AppAction, CursorKind, FontWeight, Frame, Menu, MenuItem, MenuItemState, WindowConfig};
    pub use ui_assets::{AssetError, CursorImage};
    pub use ui_core::scale;
    pub use ui_core::system_color;
    pub use ui_core::{Color, Event, Point, PointerButton, Rect, Size};
    pub use ui_support::{AppInfo, ApplicationInfo, SystemInfo};
    pub use ui_widgets::{Button, ButtonState, ButtonStyle};
    pub use ui_window::WindowStyle;
}
