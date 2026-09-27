//! The kernel's UI session bridge (NXU's drivers/video/ui_service_bridge.c),
//! seen from the desktop: the connections app processes opened with
//! `NXU_SYS_UI_CONNECT`, their messages and frames.
//!
//! Called directly by symbol, like `timer_get_microseconds`: the desktop and
//! the bridge link into the same kernel image. The types are `ui-session`'s
//! mirrors of the C structures.

use ui_session::{Connect, Message, Submit};

unsafe extern "C" {
    fn ui_bridge_session_begin(scale_permille: u32, screen_width: u32, screen_height: u32, menubar_height: u32);
    fn ui_bridge_accept() -> i32;
    fn ui_bridge_info(connection: u32) -> *const Connect;
    fn ui_bridge_pid(connection: u32) -> u32;
    fn ui_bridge_alive(connection: u32) -> bool;
    fn ui_bridge_post(connection: u32, message: *const Message) -> bool;
    fn ui_bridge_state(connection: u32, state: *mut Submit, sequence: *mut u64) -> bool;
    fn ui_bridge_take_frame(
        connection: u32,
        destination: *mut u32,
        destination_stride: u32,
        max_width: u32,
        max_height: u32,
        width_out: *mut u32,
        height_out: *mut u32,
    ) -> bool;
    fn ui_bridge_take_activation() -> u32;
    fn ui_bridge_take_launch(path: *mut u8) -> bool;
    fn ui_bridge_terminate(connection: u32);
    fn ui_bridge_release(connection: u32);

    fn kmalloc(size: usize) -> *mut u8;
    fn kfree(address: *mut u8) -> bool;
}

/// Let apps connect: the desktop is on screen.
pub(crate) fn session_begin(scale_permille: u32, screen_width: u32, screen_height: u32, menubar_height: u32) {
    unsafe { ui_bridge_session_begin(scale_permille, screen_width, screen_height, menubar_height) }
}

/// A newly connected app, taken once.
pub(crate) fn accept() -> Option<u32> {
    let connection = unsafe { ui_bridge_accept() };
    (connection >= 0).then_some(connection as u32)
}

/// What the app sent when it connected. Copied out: the bridge's copy goes
/// away with the connection.
pub(crate) fn info(connection: u32) -> Option<Connect> {
    let info = unsafe { ui_bridge_info(connection) };
    (!info.is_null()).then(|| unsafe { *info })
}

pub(crate) fn pid(connection: u32) -> u32 {
    unsafe { ui_bridge_pid(connection) }
}

pub(crate) fn alive(connection: u32) -> bool {
    unsafe { ui_bridge_alive(connection) }
}

pub(crate) fn post(connection: u32, message: &Message) -> bool {
    unsafe { ui_bridge_post(connection, message) }
}

/// The app's latest submit, when it is newer than `sequence`.
pub(crate) fn state(connection: u32, sequence: &mut u64) -> Option<Submit> {
    let mut state = Submit::empty();
    let newer = unsafe { ui_bridge_state(connection, &mut state, sequence) };
    newer.then_some(state)
}

/// Copy the app's new frame, if there is one, into `destination` (a
/// `max_width` x `max_height` area of a buffer `stride` pixels wide). Returns
/// the frame's own size.
pub(crate) fn take_frame(connection: u32, destination: &mut [u32], stride: u32, max_width: u32, max_height: u32) -> Option<(u32, u32)> {
    if max_height != 0 && destination.len() < (max_height as usize - 1) * stride as usize + max_width as usize {
        return None;
    }
    let (mut width, mut height) = (0, 0);
    let taken = unsafe {
        ui_bridge_take_frame(connection, destination.as_mut_ptr(), stride, max_width, max_height, &mut width, &mut height)
    };
    taken.then_some((width, height))
}

/// A bundle an app asked to open (Voyager opening an app), into `path`.
pub(crate) fn take_launch(path: &mut [u8; ui_session::PATH_MAX]) -> bool {
    unsafe { ui_bridge_take_launch(path.as_mut_ptr()) }
}

/// The pid of an app the Dock asked to bring forward.
pub(crate) fn take_activation() -> Option<u32> {
    let pid = unsafe { ui_bridge_take_activation() };
    (pid != 0).then_some(pid)
}

pub(crate) fn terminate(connection: u32) {
    unsafe { ui_bridge_terminate(connection) }
}

pub(crate) fn release(connection: u32) {
    unsafe { ui_bridge_release(connection) }
}

/// A kernel heap buffer of `u32` pixels, freed on drop. The desktop keeps
/// app windows' backing stores here: how many apps run, and how big their
/// windows are, is only known at run time.
pub(crate) struct Pixels {
    pointer: *mut u32,
    length: usize,
}

impl Pixels {
    pub(crate) fn new(length: usize) -> Option<Self> {
        if length == 0 {
            return None;
        }
        let pointer = unsafe { kmalloc(length * core::mem::size_of::<u32>()) } as *mut u32;
        (!pointer.is_null()).then_some(Self { pointer, length })
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [u32] {
        unsafe { core::slice::from_raw_parts_mut(self.pointer, self.length) }
    }

    pub(crate) fn len(&self) -> usize {
        self.length
    }
}

impl Drop for Pixels {
    fn drop(&mut self) {
        unsafe {
            kfree(self.pointer as *mut u8);
        }
    }
}
