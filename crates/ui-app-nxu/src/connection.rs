//! One UI session connection: what an app (or the Dock) talks to the
//! desktop through. See NXU's kern/syscall/ui_session_defs.h.

use ui_session::{self as session, Connect, Message, Session, Submit};

use crate::sys;

/// Wait until the desktop is up (after the login screen) and learn its
/// scale and screen size.
pub fn wait_for_desktop() -> Option<Session> {
    let mut info = Session::default();
    let result = unsafe { sys::nxu_ui_control(session::CONTROL_SESSION, &mut info as *mut Session as usize as u64) };
    if result < 0 {
        let mut digits = [0u8; 20];
        sys::log_line(&["ui-app-nxu: the desktop session is unavailable (error -", sys::decimal((-result) as u64, &mut digits), ")"]);
        return None;
    }
    Some(info)
}

/// `Connection::receive`'s "until a message comes".
pub const WAIT_FOREVER: u32 = u32::MAX;

pub struct Connection(u32);

/// What receiving found.
pub enum Received {
    Message(Message),
    /// Nothing waiting (only when not asked to wait).
    Empty,
    /// The desktop let go of this connection: the app should exit.
    Closed,
}

impl Connection {
    pub fn open(info: &Connect) -> Result<Self, i64> {
        let result = unsafe { sys::nxu_ui_connect(info) };
        if result > 0 { Ok(Self(result as u32)) } else { Err(result) }
    }

    /// The next message, waiting up to `wait_ms` milliseconds for one
    /// ([`WAIT_FOREVER`]: until there is one; 0: not at all).
    pub fn receive(&self, wait_ms: u32) -> Received {
        let mut message = Message::new(0);
        match unsafe { sys::nxu_ui_receive(self.0, &mut message, wait_ms) } {
            1 => Received::Message(message),
            0 => Received::Empty,
            session::E_INTERRUPTED => Received::Empty,
            _ => Received::Closed,
        }
    }

    pub fn submit(&self, submit: &Submit) -> bool {
        unsafe { sys::nxu_ui_submit(self.0, submit) >= 0 }
    }
}

/// Ask the desktop to bring the app with this pid forward (the Dock only).
pub fn activate(pid: u32) -> bool {
    unsafe { sys::nxu_ui_control(session::CONTROL_ACTIVATE, pid as u64) >= 0 }
}
