//! What an app asks the system for outside its window, wired into
//! `ui_core`'s backends the way the in-kernel host used to: directory
//! listings (Voyager) straight from the file system calls, and the
//! process table (Activity Monitor) through the UI session.

use core::ffi::c_void;

use ui_core::activity::{Activity, ProcessInfo};
use ui_session as session;

use crate::sys::{self, CPath, Dirent};

const NAME_MAX: usize = ui_core::fs::NAME_MAX;

const KIND_REGULAR: u32 = 0;
const KIND_DIRECTORY: u32 = 1;
const KIND_OTHER: u32 = 2;

const STATUS_OK: u32 = 0;
const STATUS_NOT_FOUND: u32 = 1;
const STATUS_NOT_A_DIRECTORY: u32 = 2;
const STATUS_ERROR: u32 = 3;

/// `UIServiceDirEntry`'s layout (`ui_core::fs::DirEntry` keeps its fields
/// private; this writes the same bytes).
#[repr(C)]
struct RawDirEntry {
    name_length: u32,
    name: [u8; NAME_MAX + 1],
    kind: u32,
    size_bytes: u64,
}

const _: () = assert!(core::mem::size_of::<RawDirEntry>() == core::mem::size_of::<ui_core::fs::DirEntry>());

/// `path` joined with `name`, as a C string.
fn child_path(directory: &str, name: &[u8]) -> Option<CPath> {
    let mut joined = [0u8; 255];
    let mut length = 0;
    let mut push = |bytes: &[u8]| -> Option<()> {
        let end = length + bytes.len();
        if end >= joined.len() {
            return None;
        }
        joined[length..end].copy_from_slice(bytes);
        length = end;
        Some(())
    };
    push(directory.as_bytes())?;
    if !directory.ends_with('/') {
        push(b"/")?;
    }
    push(name)?;
    CPath::new(core::str::from_utf8(&joined[..length]).ok()?)
}

unsafe extern "C" fn list_directory(
    _context: *mut c_void,
    path: *const u8,
    path_len: u32,
    entries: *mut ui_core::fs::DirEntry,
    capacity: u32,
    count_out: *mut u32,
    truncated_out: *mut bool,
) -> u32 {
    if path.is_null() || count_out.is_null() || truncated_out.is_null() || (entries.is_null() && capacity != 0) {
        return STATUS_ERROR;
    }
    unsafe {
        *count_out = 0;
        *truncated_out = false;
    }
    let bytes = unsafe { core::slice::from_raw_parts(path, path_len as usize) };
    let Ok(directory) = core::str::from_utf8(bytes) else { return STATUS_ERROR; };

    match sys::stat(directory) {
        None => return STATUS_NOT_FOUND,
        Some(stat) if stat.kind != sys::STAT_DIRECTORY => return STATUS_NOT_A_DIRECTORY,
        Some(_) => {}
    }
    let Some(c_path) = CPath::new(directory) else { return STATUS_ERROR; };
    let descriptor = unsafe { sys::nxu_open(c_path.as_ptr(), sys::O_READ) };
    if descriptor < 0 {
        return STATUS_ERROR;
    }

    let entries = entries as *mut RawDirEntry;
    let mut count = 0u32;
    let mut status = STATUS_OK;
    let mut dirent = Dirent { inode: 0, kind: 0, name_length: 0, name: [0; 256] };
    loop {
        let result = unsafe { sys::nxu_readdir(descriptor as u64, &mut dirent) };
        if result == 0 {
            break;
        }
        if result < 0 {
            status = STATUS_ERROR;
            break;
        }
        let name = &dirent.name[..(dirent.name_length as usize).min(255)];
        // A browser shows what is inside a folder, not the folder's own links.
        if name == b"." || name == b".." {
            continue;
        }
        if count >= capacity {
            unsafe { *truncated_out = true };
            break;
        }

        let entry = unsafe { &mut *entries.add(count as usize) };
        let length = name.len().min(NAME_MAX);
        *entry = RawDirEntry { name_length: dirent.name_length, name: [0; NAME_MAX + 1], kind: KIND_OTHER, size_bytes: 0 };
        entry.name[..length].copy_from_slice(&name[..length]);
        if dirent.kind == sys::DIRENT_DIRECTORY {
            entry.kind = KIND_DIRECTORY;
        } else if dirent.kind == sys::DIRENT_REGULAR {
            entry.kind = KIND_REGULAR;
            let mut stat = sys::Stat::default();
            if let Some(child) = child_path(directory, name) {
                if unsafe { sys::nxu_stat(child.as_ptr(), &mut stat) } >= 0 {
                    entry.size_bytes = stat.size;
                }
            }
        }
        count += 1;
    }

    unsafe {
        sys::nxu_close(descriptor as u64);
        *count_out = count;
    }
    status
}

unsafe extern "C" fn get_activity(
    _context: *mut c_void,
    activity: *mut Activity,
    processes: *mut ProcessInfo,
    capacity: u32,
    count_out: *mut u32,
) -> u32 {
    if activity.is_null() || count_out.is_null() {
        return 1;
    }
    let mut request = session::ActivityRequest {
        activity: activity as usize as u64,
        processes: processes as usize as u64,
        activity_size: core::mem::size_of::<Activity>() as u32,
        process_size: core::mem::size_of::<ProcessInfo>() as u32,
        capacity,
        count: 0,
    };
    let result = unsafe { sys::nxu_ui_control(session::CONTROL_ACTIVITY, &mut request as *mut _ as usize as u64) };
    if result < 0 {
        return 1;
    }
    unsafe { *count_out = request.count };
    0
}

/// A string value of `key` in the property list at `path`, into `out`.
fn plist_string<'o>(path: &str, key: &str, out: &'o mut [u8; 128]) -> Option<&'o str> {
    let bytes = sys::read_file(path)?;
    let text = core::str::from_utf8(bytes).ok()?;
    let plist = ui_bundle::plist::Plist::parse(text).ok()?;
    let id = plist.lookup(plist.root(), key)?;
    plist.string_into(id, out)
}

/// `a` + `b` + `c` as one path, into `out`.
fn join<'o>(parts: &[&str], out: &'o mut [u8; 320]) -> Option<&'o str> {
    let mut length = 0;
    for part in parts {
        let end = length + part.len();
        if end > out.len() {
            return None;
        }
        out[length..end].copy_from_slice(part.as_bytes());
        length = end;
    }
    core::str::from_utf8(&out[..length]).ok()
}

/// The icon of the bundle at `path` (as the file system names it, `/disk/...`),
/// from the `.icns` its Info.plist names, scaled to `size`.
fn app_icon(path: &str, size: u32, pixels: &mut [u32]) -> bool {
    let mut info_path = [0u8; 320];
    let Some(info) = join(&[path, "/Contents/Info.plist"], &mut info_path) else { return false; };
    let mut icon_name = [0u8; 128];
    let Some(icon) = plist_string(info, "CFBundleIconFile", &mut icon_name) else { return false; };
    let extension = if icon.contains('.') { "" } else { ".icns" };
    let mut icon_path = [0u8; 320];
    let Some(icon_path) = join(&[path, "/Contents/Resources/", icon, extension], &mut icon_path) else { return false; };

    let Some(icns) = sys::read_file(icon_path) else { return false; };
    let Ok(png) = ui_bundle::image::icns_best_png(icns, size) else { return false; };
    let Ok((raw_size, rgba_size)) = ui_bundle::image::decoded_size(png) else { return false; };
    let (Some(raw), Some(rgba)) = (sys::map(raw_size), sys::map(rgba_size)) else { return false; };
    let decoded = ui_bundle::image::decode_png(png, raw, rgba);
    if let Ok(header) = decoded {
        ui_bundle::image::scale_to_argb(rgba, header.width, header.height, size, pixels);
    }
    sys::unmap(raw);
    sys::unmap(rgba);
    decoded.is_ok()
}

/// Ask the system to open the app bundle at `path` (the Dock starts it).
fn open(path: &str) -> bool {
    let Some(c_path) = CPath::new(path) else { return false; };
    unsafe { sys::nxu_ui_control(session::CONTROL_LAUNCH, c_path.as_ptr() as usize as u64) >= 0 }
}

/// Point `ui_core`'s `fs`, `activity` and `bundle` at this process's calls.
pub fn install() {
    ui_core::fs::set_backend(core::ptr::null_mut(), Some(list_directory));
    ui_core::activity::set_backend(core::ptr::null_mut(), Some(get_activity));
    ui_core::bundle::set_backend(Some(app_icon), Some(open));
}
