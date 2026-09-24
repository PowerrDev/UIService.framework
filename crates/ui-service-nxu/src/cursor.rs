//! Installs a real cursor bitmap on top of WindowServer's small built-in
//! placeholder arrow.

use ui_assets::CursorImage;
use ui_core::Color;

use crate::storage::StaticCell;
use crate::windowserver::{self, CursorKind};

mod generated_cursors {
    include!(concat!(env!("OUT_DIR"), "/ui_cursors.rs"));
}

// The pack's 1x cursor is 32x32; its 2x (Retina) variant is 64x64. 64 is
// only the *backing store's* upper bound here -- the actual variant
// requested is picked at runtime from the host's real content scale (see
// `ui_core::scale`), not hardcoded to always ask for the 2x asset the way
// the rest of this doubling convention (about-sevos/src/panel.rs) does,
// since unlike a redrawable canvas this is a discrete asset pick.
const MAX_DIMENSION: u32 = 64;
const MAX_PIXELS: usize = (MAX_DIMENSION * MAX_DIMENSION) as usize;

// One scratch buffer, reused for each kind in turn at startup (see
// install_system_cursors): nothing here runs again afterward, so a separate
// static per kind would only cost 7 * MAX_PIXELS of wasted space for no
// benefit.
static CURSOR_SCRATCH: StaticCell<[u32; MAX_PIXELS]> = StaticCell::new([0; MAX_PIXELS]);

/// Decode one embedded cursor asset and install it as `kind`'s bitmap in
/// WindowServer. A no-op (that kind keeps its own built-in fallback shape)
/// if no asset was embedded for it at build time, or it fails to decode.
fn install_one(kind: CursorKind, data: &[u8]) {
    if data.is_empty() {
        return;
    }

    let preferred_dimension = if ui_core::scale::permille() >= 1500 { 64 } else { 32 };
    let Ok(cursor) = CursorImage::from_cur(data, preferred_dimension) else {
        return;
    };

    let size = cursor.size();
    if size.width == 0
        || size.height == 0
        || size.width > MAX_DIMENSION
        || size.height > MAX_DIMENSION
    {
        return;
    }

    let scratch = unsafe { CURSOR_SCRATCH.get_mut() };
    for y in 0..size.height {
        for x in 0..size.width {
            let color = cursor.pixel(x, y).unwrap_or(Color::TRANSPARENT);
            scratch[(y * size.width + x) as usize] =
                ((color.alpha as u32) << 24) | color.to_xrgb8888();
        }
    }

    let _ = windowserver::Set_Cursor(
        kind,
        &scratch[..(size.width * size.height) as usize],
        size.width,
        size.height,
        size.width,
        cursor.hotspot(),
    );
}

/// Decode every embedded cursor asset and install each as its own kind's
/// system pointer bitmap -- see `install_one`. `CursorKind::NotAllowed` has
/// no matching asset (no forbidden.cur in tools/UI/Cursors as of writing):
/// it keeps WindowServer's own drawn ring-and-slash fallback.
pub(crate) fn install_system_cursors() {
    install_one(CursorKind::Arrow, generated_cursors::CURSOR_ARROW);
    install_one(CursorKind::Hand, generated_cursors::CURSOR_HAND);
    install_one(CursorKind::Move, generated_cursors::CURSOR_MOVE);
    // The four resize .cur files' actual arrow content is swapped from what
    // their own filenames say (checked by decoding each one directly: e.g.
    // resize-ew.cur draws a vertical double-arrow, resize-ns.cur a
    // horizontal one, and the two diagonals the same way crossed). Not
    // touching the files or their build.rs/Makefile names -- this is the one
    // place that would need to change again if they get re-exported
    // correctly, so it says so.
    install_one(CursorKind::ResizeHorizontal, generated_cursors::CURSOR_RESIZE_NS);
    install_one(CursorKind::ResizeVertical, generated_cursors::CURSOR_RESIZE_EW);
    install_one(CursorKind::ResizeDiagonalNeSw, generated_cursors::CURSOR_RESIZE_NWSE);
    install_one(CursorKind::ResizeDiagonalNwSe, generated_cursors::CURSOR_RESIZE_NESW);
    install_one(CursorKind::Text, generated_cursors::CURSOR_TEXT);
}
