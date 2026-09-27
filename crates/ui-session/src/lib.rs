#![no_std]

//! The UI session ABI between the NXU desktop and app processes: the
//! `repr(C)` mirror of NXU's `kern/syscall/ui_session_defs.h`.
//!
//! The desktop (in the kernel, `ui-service-nxu`) and the app runtime (in
//! each app process, `ui-app-nxu`) both use these types; the kernel's C side
//! copies them in and out of app memory byte for byte. Every field is fixed
//! width and user pointers travel as `u64`, so the layout is the same on
//! arm64 and i386 -- the size checks at the bottom hold on both.

use core::mem::size_of;

pub const NAME_MAX: usize = 48;
pub const PATH_MAX: usize = 128;
pub const MENU_MAX: usize = 6;
pub const MENU_TITLE_MAX: usize = 24;
pub const MENU_ITEM_MAX: usize = 48;
pub const MENU_ITEM_TITLE_MAX: usize = 40;
pub const LABEL_MAX: usize = 48;

pub const KIND_APP: u32 = 1;
pub const KIND_DOCK: u32 = 2;

pub const MENU_SEPARATOR: u32 = 0xFFFF_FFFF;

pub const MSG_EVENT: u32 = 1;
pub const MSG_REDRAW: u32 = 2;
pub const MSG_MENU_COMMAND: u32 = 3;
pub const MSG_QUIT: u32 = 4;
pub const MSG_FRAME: u32 = 5;
pub const MSG_LAUNCH: u32 = 6;
pub const MSG_ACTIVE: u32 = 7;

pub const EVENT_POINTER_MOVED: u32 = 1;
pub const EVENT_POINTER_DOWN: u32 = 2;
pub const EVENT_POINTER_UP: u32 = 3;
pub const EVENT_POINTER_LEFT: u32 = 4;
pub const EVENT_SCROLL: u32 = 5;
pub const EVENT_KEY_DOWN: u32 = 6;

pub const BUTTON_PRIMARY: u32 = 0;
pub const BUTTON_SECONDARY: u32 = 1;
pub const BUTTON_MIDDLE: u32 = 2;

pub const FLAG_ACTIVE: u32 = 1 << 0;

pub const ACTION_NONE: u32 = 0;
pub const ACTION_CLOSE: u32 = 1;

pub const CURSOR_ARROW: u32 = 0;
pub const CURSOR_HAND: u32 = 1;
pub const CURSOR_NOT_ALLOWED: u32 = 2;

pub const SUBMIT_ANIMATING: u32 = 1 << 0;
pub const SUBMIT_MENU_STATE: u32 = 1 << 1;

pub const MENU_ENABLED: u8 = 1 << 0;
pub const MENU_CHECKED: u8 = 1 << 1;

pub const CONTROL_SESSION: u32 = 1;
pub const CONTROL_ACTIVATE: u32 = 2;
pub const CONTROL_ACTIVITY: u32 = 3;
pub const CONTROL_LAUNCH: u32 = 4;

/// `-NXU_SYS_E_*` values the calls answer with.
pub const E_NOT_FOUND: i64 = -4;
pub const E_AGAIN: i64 = -9;
pub const E_BUSY: i64 = -12;
pub const E_INTERRUPTED: i64 = -14;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MenuItem {
    pub menu: u32,
    pub command: u32,
    pub title: [u8; MENU_ITEM_TITLE_MAX],
}

impl MenuItem {
    pub const EMPTY: Self = Self { menu: 0, command: MENU_SEPARATOR, title: [0; MENU_ITEM_TITLE_MAX] };
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Connect {
    pub struct_size: u32,
    pub kind: u32,
    pub name: [u8; NAME_MAX],
    pub bundle_path: [u8; PATH_MAX],
    pub width: u32,
    pub height: u32,
    pub min_width: u32,
    pub min_height: u32,
    pub max_width: u32,
    pub max_height: u32,
    pub resizable: u32,
    pub titlebar_height: u32,
    pub corner_radius: u32,
    pub menu_count: u32,
    pub item_count: u32,
    pub reserved: u32,
    pub menu_titles: [[u8; MENU_TITLE_MAX]; MENU_MAX],
    pub items: [MenuItem; MENU_ITEM_MAX],
}

impl Connect {
    pub const fn empty(kind: u32) -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            kind,
            name: [0; NAME_MAX],
            bundle_path: [0; PATH_MAX],
            width: 0,
            height: 0,
            min_width: 0,
            min_height: 0,
            max_width: 0,
            max_height: 0,
            resizable: 0,
            titlebar_height: 0,
            corner_radius: 0,
            menu_count: 0,
            item_count: 0,
            reserved: 0,
            menu_titles: [[0; MENU_TITLE_MAX]; MENU_MAX],
            items: [MenuItem::EMPTY; MENU_ITEM_MAX],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Message {
    pub kind: u32,
    pub event: u32,
    pub x: i32,
    pub y: i32,
    pub delta: i32,
    pub button: u32,
    pub character: u32,
    pub command: u32,
    pub width: u32,
    pub height: u32,
    pub flags: u32,
    pub reserved: u32,
    pub time_us: u64,
    pub path: [u8; PATH_MAX],
}

impl Message {
    pub const fn new(kind: u32) -> Self {
        Self {
            kind,
            event: 0,
            x: 0,
            y: 0,
            delta: 0,
            button: 0,
            character: 0,
            command: 0,
            width: 0,
            height: 0,
            flags: 0,
            reserved: 0,
            time_us: 0,
            path: [0; PATH_MAX],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Submit {
    pub pixels: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub action: u32,
    pub cursor: u32,
    pub flags: u32,
    pub menu_state: [u8; MENU_ITEM_MAX],
    pub panel_x: i32,
    pub panel_y: i32,
    pub panel_width: u32,
    pub panel_height: u32,
    pub panel_radius: u32,
    pub label_x: i32,
    pub label: [u8; LABEL_MAX],
}

impl Submit {
    pub const fn empty() -> Self {
        Self {
            pixels: 0,
            width: 0,
            height: 0,
            stride: 0,
            action: ACTION_NONE,
            cursor: CURSOR_ARROW,
            flags: 0,
            menu_state: [0; MENU_ITEM_MAX],
            panel_x: 0,
            panel_y: 0,
            panel_width: 0,
            panel_height: 0,
            panel_radius: 0,
            label_x: 0,
            label: [0; LABEL_MAX],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Session {
    pub scale_permille: u32,
    pub screen_width: u32,
    pub screen_height: u32,
    pub menubar_height: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ActivityRequest {
    pub activity: u64,
    pub processes: u64,
    pub activity_size: u32,
    pub process_size: u32,
    pub capacity: u32,
    pub count: u32,
}

// The C layout, checked at compile time on every target this builds for.
const _: () = assert!(size_of::<MenuItem>() == 48);
const _: () = assert!(size_of::<Connect>() == 2680);
const _: () = assert!(size_of::<Message>() == 56 + 128);
const _: () = assert!(size_of::<Submit>() == 152);
const _: () = assert!(size_of::<Session>() == 16);
const _: () = assert!(size_of::<ActivityRequest>() == 32);

/// Copy `text` into a fixed, NUL-terminated field (cut short on a character
/// boundary if it does not fit).
pub fn set_str(field: &mut [u8], text: &str) {
    let mut length = text.len().min(field.len().saturating_sub(1));
    while length > 0 && !text.is_char_boundary(length) {
        length -= 1;
    }
    field[..length].copy_from_slice(&text.as_bytes()[..length]);
    for byte in &mut field[length..] {
        *byte = 0;
    }
}

/// The text of a fixed, NUL-terminated field.
pub fn get_str(field: &[u8]) -> &str {
    let length = field.iter().position(|&byte| byte == 0).unwrap_or(field.len());
    match core::str::from_utf8(&field[..length]) {
        Ok(text) => text,
        Err(error) => core::str::from_utf8(&field[..error.valid_up_to()]).unwrap_or(""),
    }
}
