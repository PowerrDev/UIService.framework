//! App bundles' own icons: a folder named `<Name>.app` is shown with the
//! app's icon (the `.icns` its `Info.plist` names, loaded through
//! `ui::core::bundle`), not a folder. Decoding an icon is real work (a PNG
//! inflated and scaled), so each is loaded once per size and kept.

use ui::prelude::*;
use ui::render::Canvas;

use crate::storage::StaticCell;

/// The largest icon kept (grid icons at 2x are 128 px or less).
const MAX_SIZE: usize = 128;
const SLOTS: usize = 12;

#[derive(Clone, Copy)]
struct Slot {
    key: u64,
    size: u32,
    loaded: bool,
}

struct Cache {
    slots: [Slot; SLOTS],
    pixels: [[u32; MAX_SIZE * MAX_SIZE]; SLOTS],
    next: usize,
    /// The folder being shown: icons are looked up as `directory/name`.
    directory: [u8; 256],
    directory_len: usize,
}

static CACHE: StaticCell<Cache> = StaticCell::new(Cache {
    slots: [Slot { key: 0, size: 0, loaded: false }; SLOTS],
    pixels: [[0; MAX_SIZE * MAX_SIZE]; SLOTS],
    next: 0,
    directory: [0; 256],
    directory_len: 0,
});

fn cache() -> &'static mut Cache {
    unsafe { CACHE.get_mut() }
}

/// The folder whose entries are about to be drawn (the navigator calls this
/// whenever it lists one).
pub(crate) fn set_directory(path: &str) {
    let cache = cache();
    let length = path.len().min(cache.directory.len());
    cache.directory[..length].copy_from_slice(&path.as_bytes()[..length]);
    cache.directory_len = length;
}

/// `directory/name` into `buffer`.
pub(crate) fn full_path<'b>(name: &str, buffer: &'b mut [u8; 320]) -> Option<&'b str> {
    let cache = cache();
    let directory = &cache.directory[..cache.directory_len];
    let separator = if directory.ends_with(b"/") { 0 } else { 1 };
    let total = directory.len() + separator + name.len();
    if total > buffer.len() {
        return None;
    }
    buffer[..directory.len()].copy_from_slice(directory);
    if separator == 1 {
        buffer[directory.len()] = b'/';
    }
    buffer[directory.len() + separator..total].copy_from_slice(name.as_bytes());
    core::str::from_utf8(&buffer[..total]).ok()
}

fn hash(text: &str, size: u32) -> u64 {
    let mut value: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes().chain(size.to_le_bytes()) {
        value ^= byte as u64;
        value = value.wrapping_mul(0x0000_0100_0000_01b3);
    }
    value
}

/// Draw the icon of the app bundle `name` (in the current folder) into
/// `rect`. False when it has no icon to show (the caller draws a generic one).
pub(crate) fn draw(canvas: &mut dyn Canvas, rect: Rect, name: &str) -> bool {
    let size = rect.size.width.min(rect.size.height).min(MAX_SIZE as u32);
    if size < 4 {
        return false;
    }
    let mut buffer = [0u8; 320];
    let Some(path) = full_path(name, &mut buffer) else { return false; };
    let key = hash(path, size);
    let cache = cache();

    let index = match cache.slots.iter().position(|slot| slot.key == key && slot.size == size) {
        Some(index) => index,
        None => {
            let index = cache.next;
            cache.next = (cache.next + 1) % SLOTS;
            let pixels = &mut cache.pixels[index][..size as usize * size as usize];
            let loaded = ui::core::bundle::app_icon(path, size, pixels);
            cache.slots[index] = Slot { key, size, loaded };
            index
        }
    };
    if !cache.slots[index].loaded {
        return false;
    }

    let pixels = &cache.pixels[index][..size as usize * size as usize];
    let origin = Point::new(rect.origin.x + (rect.size.width - size) as i32 / 2, rect.origin.y + (rect.size.height - size) as i32 / 2);
    for y in 0..size as usize {
        for x in 0..size as usize {
            let pixel = pixels[y * size as usize + x];
            let alpha = (pixel >> 24) as u8;
            if alpha == 0 {
                continue;
            }
            canvas.blend_pixel(
                Point::new(origin.x + x as i32, origin.y + y as i32),
                Color::rgba((pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8, alpha),
            );
        }
    }
    true
}
