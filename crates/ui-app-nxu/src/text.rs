//! Text for an app process: Inter, read from the system's font folder when
//! the app starts (not embedded in every app), or the bootstrap face if the
//! fonts are missing.

use core::cell::UnsafeCell;

use ui_core::{Color, Point, Size};
use ui_render::{Canvas, TextRenderer};
use ui_text::{BootstrapText, FontFamily, TextScratch, TtfTextRenderer};

use crate::sys;

pub const INTER_REGULAR: &str = "/disk/System/Library/Fonts/Inter-Regular.ttf";
pub const INTER_SEMIBOLD: &str = "/disk/System/Library/Fonts/Inter-SemiBold.ttf";

struct Scratch(UnsafeCell<TextScratch>);

// One thread per app runtime.
unsafe impl Sync for Scratch {}

/// The glyph cache and outline buffers (~300 KiB): a static, never a local.
static SCRATCH: Scratch = Scratch(UnsafeCell::new(TextScratch::new()));

pub enum AppText<'a> {
    Inter(TtfTextRenderer<'static, 'a>),
    Bootstrap(BootstrapText),
}

impl TextRenderer for AppText<'_> {
    fn measure(&self, text: &str, point_size: u32, semibold: bool) -> Size {
        match self {
            Self::Inter(renderer) => renderer.measure(text, point_size, semibold),
            Self::Bootstrap(renderer) => renderer.measure(text, point_size, semibold),
        }
    }

    fn draw<C: Canvas + ?Sized>(&mut self, canvas: &mut C, origin: Point, text: &str, color: Color, point_size: u32, semibold: bool) {
        match self {
            Self::Inter(renderer) => renderer.draw(canvas, origin, text, color, point_size, semibold),
            Self::Bootstrap(renderer) => renderer.draw(canvas, origin, text, color, point_size, semibold),
        }
    }
}

/// Load the fonts and build the renderer; call once.
pub fn load() -> AppText<'static> {
    let regular = sys::read_file(INTER_REGULAR);
    let semibold = sys::read_file(INTER_SEMIBOLD);
    if let Some(regular) = regular {
        if let Ok(family) = FontFamily::from_bytes(regular, semibold) {
            let scratch = unsafe { &mut *SCRATCH.0.get() };
            return AppText::Inter(TtfTextRenderer::new(family, scratch));
        }
    }
    sys::log_line(&["ui-app-nxu: ", INTER_REGULAR, " unavailable, using the bootstrap face"]);
    AppText::Bootstrap(BootstrapText)
}
