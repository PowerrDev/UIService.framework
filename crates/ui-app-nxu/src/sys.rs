//! NXU's system calls, as the userland C library wraps them
//! (frameworks/include/nxu/syscall.h): the app's executable links that
//! library next to this crate's static archive.

use core::ffi::c_char;

use ui_session::{Connect, Message, Submit};

pub const O_READ: u64 = 1 << 0;
pub const MMAP_READ_WRITE: u64 = 0;

pub const DIRENT_REGULAR: u32 = 1;
pub const DIRENT_DIRECTORY: u32 = 2;
pub const STAT_REGULAR: u32 = 1;
pub const STAT_DIRECTORY: u32 = 2;

pub const E_AGAIN: i64 = -9;

/// `nxu_dirent_t`.
#[repr(C)]
pub struct Dirent {
    pub inode: u64,
    pub kind: u32,
    pub name_length: u32,
    pub name: [u8; 256],
}

/// `nxu_stat_t`.
#[repr(C)]
#[derive(Default)]
pub struct Stat {
    pub inode: u64,
    pub size: u64,
    pub kind: u32,
    pub reserved: u32,
}

unsafe extern "C" {
    pub fn nxu_exit(status: u64) -> i64;
    pub fn nxu_write(descriptor: u64, buffer: *const u8, length: u64) -> i64;
    pub fn nxu_open(path: *const c_char, flags: u64) -> i64;
    pub fn nxu_read(descriptor: u64, buffer: *mut u8, length: u64) -> i64;
    pub fn nxu_close(descriptor: u64) -> i64;
    pub fn nxu_readdir(descriptor: u64, entry: *mut Dirent) -> i64;
    pub fn nxu_stat(path: *const c_char, stat: *mut Stat) -> i64;
    pub fn nxu_mmap(size: u64, prot_flags: u64) -> i64;
    pub fn nxu_munmap(address: u64, size: u64) -> i64;
    pub fn nxu_uptime_us() -> i64;
    pub fn nxu_yield() -> i64;
    pub fn nxu_spawn(path: *const c_char, name: *const c_char) -> i64;
    pub fn nxu_waitpid(pid: u64, status: *mut u64) -> i64;
    pub fn nxu_getpid() -> i64;
    pub fn nxu_ipc_port_allocate() -> i64;
    pub fn nxu_ipc_receive_wait(port: u32, buffer: *mut u8, capacity: u64, xfer_name: *mut u32, xfer_type: *mut u32) -> i64;

    pub fn nxu_ui_connect(info: *const Connect) -> i64;
    pub fn nxu_ui_receive(connection: u32, message: *mut Message, wait: u32) -> i64;
    pub fn nxu_ui_submit(connection: u32, submit: *const Submit) -> i64;
    pub fn nxu_ui_control(operation: u32, argument: u64) -> i64;
}

/// Write `text` to the kernel log (standard output).
pub fn log(text: &str) {
    unsafe {
        nxu_write(1, text.as_ptr(), text.len() as u64);
    }
}

/// Log the parts as one line: written with one call, so it never
/// interleaves with another process's output.
pub fn log_line(parts: &[&str]) {
    let mut line = [0u8; 256];
    let mut length = 0;
    for part in parts {
        let take = part.len().min(line.len() - 1 - length);
        line[length..length + take].copy_from_slice(&part.as_bytes()[..take]);
        length += take;
    }
    line[length] = b'\n';
    unsafe {
        nxu_write(1, line.as_ptr(), length as u64 + 1);
    }
}

pub fn exit(status: u64) -> ! {
    unsafe {
        nxu_exit(status);
    }
    loop {
        core::hint::spin_loop();
    }
}

/// Sleep for good, using no CPU: for a service with nothing to do on this
/// system (the Dock on a kernel without a desktop), which bootd would only
/// start again if it exited.
pub fn idle_forever() -> ! {
    let port = unsafe { nxu_ipc_port_allocate() };
    let mut byte = 0u8;
    loop {
        if port > 0 {
            unsafe {
                nxu_ipc_receive_wait(port as u32, &mut byte, 1, core::ptr::null_mut(), core::ptr::null_mut());
            }
        } else {
            unsafe {
                nxu_yield();
            }
        }
    }
}

pub fn uptime_us() -> u64 {
    let now = unsafe { nxu_uptime_us() };
    if now < 0 { 0 } else { now as u64 }
}

/// A NUL-terminated copy of `path` for the C calls; `None` if it does not fit.
pub struct CPath {
    bytes: [u8; 256],
}

impl CPath {
    pub fn new(path: &str) -> Option<Self> {
        if path.len() >= 256 || path.as_bytes().contains(&0) {
            return None;
        }
        let mut bytes = [0u8; 256];
        bytes[..path.len()].copy_from_slice(path.as_bytes());
        Some(Self { bytes })
    }

    pub fn as_ptr(&self) -> *const c_char {
        self.bytes.as_ptr() as *const c_char
    }
}

pub fn stat(path: &str) -> Option<Stat> {
    let path = CPath::new(path)?;
    let mut stat = Stat::default();
    (unsafe { nxu_stat(path.as_ptr(), &mut stat) } >= 0).then_some(stat)
}

/// Private memory for the rest of the process's life (it is never unmapped).
pub fn map(bytes: usize) -> Option<&'static mut [u8]> {
    if bytes == 0 {
        return None;
    }
    let address = unsafe { nxu_mmap(bytes as u64, MMAP_READ_WRITE) };
    if address <= 0 {
        return None;
    }
    Some(unsafe { core::slice::from_raw_parts_mut(address as usize as *mut u8, bytes) })
}

/// Give back memory from [`map`] (the whole of it).
pub fn unmap(memory: &'static mut [u8]) {
    unsafe {
        nxu_munmap(memory.as_mut_ptr() as usize as u64, memory.len() as u64);
    }
}

/// Read the whole file at `path` into fresh memory that stays mapped.
pub fn read_file(path: &str) -> Option<&'static [u8]> {
    let size = stat(path).filter(|stat| stat.kind == STAT_REGULAR)?.size as usize;
    let buffer = map(size.max(1))?;
    let c_path = CPath::new(path)?;
    let descriptor = unsafe { nxu_open(c_path.as_ptr(), O_READ) };
    if descriptor < 0 {
        return None;
    }
    let mut filled = 0usize;
    while filled < size {
        let read = unsafe { nxu_read(descriptor as u64, buffer[filled..].as_mut_ptr(), (size - filled) as u64) };
        if read <= 0 {
            break;
        }
        filled += read as usize;
    }
    unsafe {
        nxu_close(descriptor as u64);
    }
    (filled == size).then_some(&buffer[..size])
}

/// Format `value` in decimal into `buffer`.
pub fn decimal(value: u64, buffer: &mut [u8; 20]) -> &str {
    let mut index = buffer.len();
    let mut value = value;
    loop {
        index -= 1;
        buffer[index] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 || index == 0 {
            break;
        }
    }
    core::str::from_utf8(&buffer[index..]).unwrap_or("?")
}
