use ui_core::{Color, Point, Size};
use ui_render::{Canvas, TextRenderer};
use ui_text::{BootstrapText, FontFamily, TextScratch, TtfTextRenderer};

use crate::storage::{StaticCell, TEXT_SCRATCH};

mod generated_fonts {
    include!(concat!(env!("OUT_DIR"), "/ui_fonts.rs"));
}

pub(crate) enum NXUText<'a> {
    Inter(TtfTextRenderer<'static, 'a>),
    Bootstrap(BootstrapText),
}

impl TextRenderer for NXUText<'_> {
    fn measure(&self, text: &str, point_size: u32, semibold: bool) -> Size {
        match self {
            Self::Inter(renderer) => renderer.measure(text, point_size, semibold),
            Self::Bootstrap(renderer) => renderer.measure(text, point_size, semibold),
        }
    }

    fn draw<C: Canvas + ?Sized>(
        &mut self,
        canvas: &mut C,
        origin: Point,
        text: &str,
        color: Color,
        point_size: u32,
        semibold: bool,
    ) {
        match self {
            Self::Inter(renderer) => {
                renderer.draw(canvas, origin, text, color, point_size, semibold)
            }
            Self::Bootstrap(renderer) => {
                renderer.draw(canvas, origin, text, color, point_size, semibold)
            }
        }
    }
}


pub(crate) fn backend(scratch: &mut TextScratch) -> NXUText<'_> {
    if !generated_fonts::INTER_REGULAR.is_empty() {
        if let Ok(family) = FontFamily::from_bytes(
            generated_fonts::INTER_REGULAR,
            if generated_fonts::INTER_SEMIBOLD.is_empty() {
                None
            } else {
                Some(generated_fonts::INTER_SEMIBOLD)
            },
        ) {
            return NXUText::Inter(TtfTextRenderer::new(family, scratch));
        }
    }

    NXUText::Bootstrap(BootstrapText)
}

/// The desktop's one text backend, built on first use and kept: fonts are
/// parsed once, and no caller carries a multi-KiB `NXUText` in its own stack
/// frame (the desktop runs on the arm64 kernel's 16 KiB boot stack, and a
/// few nested frames that each built their own backend overflowed it into
/// the kernel's `.bss`). Owns `TEXT_SCRATCH` from then on: only the login
/// screen, which finishes before the desktop starts, builds its own.
pub(crate) fn shared() -> &'static mut NXUText<'static> {
    static SHARED: StaticCell<Option<NXUText<'static>>> = StaticCell::new(None);

    #[inline(never)]
    fn build(slot: &'static mut Option<NXUText<'static>>) {
        *slot = Some(backend(unsafe { TEXT_SCRATCH.get_mut() }));
    }

    let slot = unsafe { SHARED.get_mut() };
    if slot.is_none() {
        build(unsafe { SHARED.get_mut() });
    }
    match slot {
        Some(text) => text,
        None => unreachable!(),
    }
}

/// Borel, the setup greeting's script face, when it was embedded. Shares
/// the Inter scratch under its own cache tag.
pub(crate) fn display(scratch: &mut TextScratch) -> Option<TtfTextRenderer<'static, '_>> {
    if generated_fonts::BOREL_REGULAR.is_empty() {
        return None;
    }

    FontFamily::from_bytes(generated_fonts::BOREL_REGULAR, None)
        .ok()
        .map(|family| TtfTextRenderer::new(family.with_cache_tag(1), scratch))
}

pub(crate) fn has_display() -> bool {
    !generated_fonts::BOREL_REGULAR.is_empty()
        && FontFamily::from_bytes(generated_fonts::BOREL_REGULAR, None).is_ok()
}

pub(crate) fn has_inter() -> bool {
    if generated_fonts::INTER_REGULAR.is_empty() {
        return false;
    }

    FontFamily::from_bytes(
        generated_fonts::INTER_REGULAR,
        if generated_fonts::INTER_SEMIBOLD.is_empty() {
            None
        } else {
            Some(generated_fonts::INTER_SEMIBOLD)
        },
    )
    .is_ok()
}
