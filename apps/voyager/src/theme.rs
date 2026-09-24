//! Voyager's visual foundation: palette, metrics and text helpers.
//!
//! The look follows modern macOS Finder in its light appearance: a light grey
//! toolbar that continues the window's titlebar, a slightly lighter sidebar,
//! white content, hairline dividers, a blue accent for selection, and 13 pt
//! Inter for labels. `Canvas` still has no path or stroke primitive, so icons
//! and glyphs are drawn procedurally with `draw2d` (see `icons`) instead of
//! shipping bitmaps: they stay crisp at any content scale.
//!
//! Every metric is a real 1x-point value converted to physical pixels through
//! `ui_core::scale::pt` for the host's actual content scale, never a
//! hardcoded 2x (see `ui-window::WindowStyle::DEFAULT` and
//! `about-sevos/src/panel.rs` for the same convention).
//!
//! Text, glyph and accent colors are the system's semantic ones
//! (`ui::core::system_color`), so Voyager's contrast is the same the rest of
//! the OS is held to; the tests at the bottom check them against the surfaces
//! only Voyager has (sidebar, stripes, hover and pressed fills).

use ui::prelude::*;

/// The window's own titlebar colour (`ui-window::WindowStyle::DEFAULT`): the
/// toolbar uses it too, so the two read as one band.
pub const TOOLBAR_BACKGROUND: Color = system_color::TITLEBAR;
pub const SIDEBAR_BACKGROUND: Color = Color::rgb(238, 239, 242);
pub const WINDOW_BACKGROUND: Color = system_color::CONTENT_BACKGROUND;
/// Every other list row, a shade off white.
pub const STRIPE: Color = Color::rgb(246, 247, 249);
pub const STATUS_BACKGROUND: Color = Color::rgb(246, 247, 249);
pub const DIVIDER: Color = system_color::SEPARATOR;
/// A row/tile's hairline, lighter than a region divider.
pub const HAIRLINE: Color = Color::rgb(226, 228, 233);

pub const TEXT_PRIMARY: Color = system_color::LABEL;
pub const TEXT_SECONDARY: Color = system_color::SECONDARY_LABEL;
/// Glyphs that work but should recede, e.g. a folder row's disclosure chevron.
pub const TEXT_TERTIARY: Color = system_color::TERTIARY_LABEL;
/// Disabled controls only (a Back button with no history): never for anything
/// that still responds.
pub const TEXT_DISABLED: Color = system_color::DISABLED_LABEL;
/// Section headings are secondary text, not a lighter grey of their own: the
/// old 133,136,145 was 3.1:1 on the sidebar at 11 pt.
pub const SIDEBAR_HEADER: Color = system_color::SECONDARY_LABEL;
/// Toolbar glyphs (chevrons, view modes, sidebar toggle).
pub const GLYPH: Color = Color::rgb(84, 87, 97);

/// The system accent, which `ui-widgets::ButtonStyle::default()` uses too, so a
/// selection matches the system's buttons.
pub const ACCENT: Color = system_color::ACCENT;
pub const ACCENT_PRESSED: Color = system_color::ACCENT_PRESSED;
/// Text on a selected (accent) row or name pill, every column included.
pub const ON_ACCENT: Color = system_color::ON_ACCENT;
/// A row that is being pressed but is not (yet) the selection.
pub const ACCENT_SOFT: Color = Color::rgb(203, 220, 252);

/// A control under the pointer.
pub const HOVER: Color = Color::rgb(210, 212, 219);
/// A control while the button is down over it.
pub const PRESSED: Color = Color::rgb(196, 199, 207);
/// The sidebar's current destination (Finder's grey, not accent, highlight).
pub const SIDEBAR_SELECTION: Color = Color::rgb(216, 218, 225);
pub const SIDEBAR_HOVER: Color = Color::rgb(228, 230, 236);
/// The track of a segmented control and the puck that marks its active segment.
pub const SEGMENT_TRACK: Color = Color::rgb(207, 209, 216);
pub const SEGMENT_PUCK: Color = Color::rgb(255, 255, 255);
pub const SEGMENT_PUCK_BORDER: Color = Color::rgb(196, 198, 205);
/// The plate behind a selected grid icon.
pub const ICON_PLATE: Color = Color::rgb(222, 224, 231);
pub const ICON_PLATE_PRESSED: Color = Color::rgb(206, 209, 217);

// The default window is 430 x 300 pt (860 x 600 is `WindowConfig`'s doubled
// reference: see `WindowConfig::effective_size`), 268 pt of it content, and it
// can be resized from 240 x 180 pt up to the 860 x 600 px backing store. The
// metrics below are sized for the default and degrade towards the smallest.

pub fn toolbar_height() -> u32 {
    scale::pt(40)
}
pub fn status_height() -> u32 {
    scale::pt(22)
}

/// The sidebar takes a third of the window, between what its labels need and
/// what a wide window would waste.
pub fn sidebar_width_for(content_width: u32) -> u32 {
    (content_width * 30 / 100).clamp(scale::pt(104), scale::pt(168))
}

pub fn row_height() -> u32 {
    scale::pt(24)
}
pub fn margin() -> i32 {
    scale::pt_i32(14)
}

pub fn label_point_size() -> u32 {
    scale::pt(13)
}
pub fn caption_point_size() -> u32 {
    scale::pt(11)
}
pub fn title_point_size() -> u32 {
    scale::pt(13)
}
pub fn grid_name_point_size() -> u32 {
    scale::pt(12)
}

/// A hairline: one physical pixel at 1x, two at 2x.
pub fn hairline() -> u32 {
    scale::pt(1).max(1)
}

/// Stroke width for glyphs, in fixed point: 1.5 pt.
pub fn glyph_stroke() -> i32 {
    (scale::permille() as i32 * 96) / 1000
}

/// The height of one line of text at `point_size`.
pub fn line_height(frame: &Frame<'_>, point_size: u32) -> u32 {
    frame.measure("Ag", point_size).height
}

/// Top of a line of text centred vertically in `rect`.
pub fn text_top(frame: &Frame<'_>, rect: Rect, point_size: u32) -> i32 {
    rect.origin.y + (rect.size.height.saturating_sub(line_height(frame, point_size)) / 2) as i32
}

/// Draw `text` centred vertically in `rect` starting at `x`.
pub fn text_in(frame: &mut Frame<'_>, rect: Rect, x: i32, text: &str, color: Color, point_size: u32, weight: FontWeight) {
    let y = text_top(frame, rect, point_size);
    frame.text_with_weight(Point::new(x, y), text, color, point_size, weight);
}

pub const ELLIPSIS: &str = "\u{2026}";

/// `text` cut with a trailing ellipsis so it fits in `max_width`; `scratch` is
/// where the shortened copy goes (there is no allocator here). Returns `text`
/// itself when it already fits.
pub fn fit_text<'a>(frame: &Frame<'_>, text: &'a str, point_size: u32, weight: FontWeight, max_width: u32, scratch: &'a mut [u8]) -> &'a str {
    if frame.measure_with_weight(text, point_size, weight).width <= max_width {
        return text;
    }

    let ellipsis = ELLIPSIS.len();
    let mut end = text.len();
    while end > 0 {
        end -= 1;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        if end + ellipsis > scratch.len() {
            continue;
        }

        scratch[..end].copy_from_slice(&text.as_bytes()[..end]);
        scratch[end..end + ellipsis].copy_from_slice(ELLIPSIS.as_bytes());
        let candidate = core::str::from_utf8(&scratch[..end + ellipsis]).unwrap_or("");
        if frame.measure_with_weight(candidate, point_size, weight).width <= max_width {
            // Re-borrow immutably for the return value.
            let length = end + ellipsis;
            return core::str::from_utf8(&scratch[..length]).unwrap_or("");
        }
    }

    ELLIPSIS
}

/// A file name cut to `max_width` in the middle, so the extension (the part
/// that says what the file is) stays visible: `Boot_Aud…dio.wav`. Names with
/// no extension keep their last few characters instead. Returns `text` itself
/// when it fits.
pub fn fit_name<'a>(frame: &Frame<'_>, text: &'a str, point_size: u32, weight: FontWeight, max_width: u32, scratch: &'a mut [u8]) -> &'a str {
    if frame.measure_with_weight(text, point_size, weight).width <= max_width {
        return text;
    }

    // Where the kept tail starts: the last dot if it looks like an extension,
    // with two characters of the stem before it so the cut does not sit right
    // against the dot (`notes…es.md`, not `notes….md`).
    let extension = match text.rfind('.') {
        Some(dot) if dot > 0 && text.len() - dot <= 8 => Some(dot),
        _ => None,
    };
    let mut tail_start = extension.unwrap_or(text.len().saturating_sub(4));
    if extension.is_some() {
        for _ in 0..2 {
            if tail_start > 0 {
                tail_start -= 1;
                while tail_start > 0 && !text.is_char_boundary(tail_start) {
                    tail_start -= 1;
                }
            }
        }
    }
    while tail_start < text.len() && !text.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    let tail = &text.as_bytes()[tail_start..];
    let ellipsis = ELLIPSIS.as_bytes();

    let mut head_end = tail_start;
    while head_end > 0 {
        head_end -= 1;
        while head_end > 0 && !text.is_char_boundary(head_end) {
            head_end -= 1;
        }

        let length = head_end + ellipsis.len() + tail.len();
        if length > scratch.len() {
            continue;
        }

        scratch[..head_end].copy_from_slice(&text.as_bytes()[..head_end]);
        scratch[head_end..head_end + ellipsis.len()].copy_from_slice(ellipsis);
        scratch[head_end + ellipsis.len()..length].copy_from_slice(tail);

        let candidate = core::str::from_utf8(&scratch[..length]).unwrap_or("");
        if frame.measure_with_weight(candidate, point_size, weight).width <= max_width {
            return core::str::from_utf8(&scratch[..length]).unwrap_or("");
        }
    }

    // Not even the tail and an ellipsis fit: fall back to cutting the end.
    fit_text(frame, text, point_size, weight, max_width, scratch)
}

/// A hairline-thick horizontal rule.
pub fn hline(x: i32, y: i32, width: u32) -> Rect {
    Rect::new(x, y, width, hairline())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui::render::{Canvas, Surface, TextRenderer};

    /// Every character is half the point size wide: easy to reason about.
    struct FixedWidth;

    impl TextRenderer for FixedWidth {
        fn measure(&self, text: &str, point_size: u32, _semibold: bool) -> Size {
            Size::new(text.chars().count() as u32 * point_size / 2, point_size)
        }

        fn draw<C: Canvas + ?Sized>(&mut self, _: &mut C, _: Point, _: &str, _: Color, _: u32, _: bool) {}
    }

    fn with_frame<R>(run: impl FnOnce(&Frame<'_>) -> R) -> R {
        let mut pixels = [0u32; 16];
        let mut surface = Surface::new(&mut pixels, 4, 4, 4).expect("surface");
        let mut text = FixedWidth;
        let frame = Frame::new(&mut surface, &mut text);
        run(&frame)
    }

    fn width(frame: &Frame<'_>, text: &str) -> u32 {
        frame.measure_with_weight(text, 10, FontWeight::Regular).width
    }

    fn assert_contrast(foreground: Color, background: Color, minimum: f32, what: &str) {
        let ratio = foreground.contrast_ratio(background);
        assert!(ratio >= minimum, "{what}: {ratio:.2}:1, below {minimum}:1");
    }

    #[test]
    fn text_is_readable_on_every_voyager_surface() {
        let text = system_color::MIN_TEXT_CONTRAST;
        for (surface, name) in [
            (WINDOW_BACKGROUND, "content"),
            (STRIPE, "stripe"),
            (STATUS_BACKGROUND, "status line"),
            (SIDEBAR_BACKGROUND, "sidebar"),
            (TOOLBAR_BACKGROUND, "toolbar"),
            (SIDEBAR_SELECTION, "sidebar selection"),
            (HOVER, "hover"),
            (PRESSED, "pressed"),
            (ACCENT_SOFT, "pressed row"),
        ] {
            assert_contrast(TEXT_PRIMARY, surface, text, name);
        }
        for (surface, name) in [
            (WINDOW_BACKGROUND, "content"),
            (STRIPE, "stripe"),
            (STATUS_BACKGROUND, "status line"),
            (SIDEBAR_BACKGROUND, "sidebar heading"),
        ] {
            assert_contrast(TEXT_SECONDARY, surface, text, name);
        }
        assert_contrast(ON_ACCENT, ACCENT, text, "selected row");
        assert_contrast(ON_ACCENT, ACCENT_PRESSED, text, "selected row, pressed");
    }

    #[test]
    fn glyphs_are_visible_on_every_surface_they_sit_on() {
        let graphic = system_color::MIN_GRAPHIC_CONTRAST;
        for (surface, name) in [(TOOLBAR_BACKGROUND, "toolbar"), (HOVER, "hover"), (PRESSED, "pressed"), (SEGMENT_TRACK, "segment track")] {
            assert_contrast(GLYPH, surface, graphic, name);
        }
        // The active view segment's glyph, on its puck.
        assert_contrast(ACCENT, SEGMENT_PUCK, graphic, "active segment");
        assert_contrast(ACCENT, PRESSED, graphic, "active segment, pressed");
        // Sidebar destination glyphs, in every row state.
        for (surface, name) in [(SIDEBAR_BACKGROUND, "sidebar"), (SIDEBAR_HOVER, "hover"), (SIDEBAR_SELECTION, "selection"), (PRESSED, "pressed")] {
            assert_contrast(ACCENT, surface, graphic, name);
        }
        // Disclosure chevrons on both row stripes.
        assert_contrast(TEXT_TERTIARY, WINDOW_BACKGROUND, graphic, "chevron");
        assert_contrast(TEXT_TERTIARY, STRIPE, graphic, "chevron on stripe");
    }

    #[test]
    fn a_name_that_fits_is_returned_untouched() {
        with_frame(|frame| {
            let mut scratch = [0u8; 64];
            assert_eq!(fit_name(frame, "notes.txt", 10, FontWeight::Regular, 100, &mut scratch), "notes.txt");
            assert_eq!(fit_text(frame, "notes.txt", 10, FontWeight::Regular, 100, &mut scratch), "notes.txt");
        });
    }

    #[test]
    fn a_long_name_is_cut_in_the_middle_and_keeps_its_extension() {
        with_frame(|frame| {
            let mut scratch = [0u8; 64];
            let name = "An extraordinarily long file name.txt";
            let shown = fit_name(frame, name, 10, FontWeight::Regular, 100, &mut scratch);
            // The extension and two characters of the stem stay visible.
            assert!(shown.ends_with("me.txt"), "{shown}");
            assert!(shown.contains(ELLIPSIS), "{shown}");
            assert!(shown.starts_with("An "), "{shown}");
            assert!(width(frame, shown) <= 100);
            // As much as fits is kept: one more character of the head would not.
            assert!(width(frame, shown) > 100 - 10);
        });
    }

    #[test]
    fn end_truncation_fits_and_marks_the_cut() {
        with_frame(|frame| {
            let mut scratch = [0u8; 64];
            let shown = fit_text(frame, "Resources and more", 10, FontWeight::Regular, 60, &mut scratch);
            assert!(shown.ends_with(ELLIPSIS));
            assert!(width(frame, shown) <= 60);
        });
    }

    #[test]
    fn nothing_fits_degrades_to_just_the_ellipsis_never_a_panic() {
        with_frame(|frame| {
            let mut scratch = [0u8; 64];
            for name in ["file.png", "\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}.png", "noextensionatall"] {
                let shown = fit_name(frame, name, 10, FontWeight::Regular, 6, &mut scratch);
                assert!(shown.chars().count() <= 2, "{shown}");
            }
        });
    }

    #[test]
    fn multibyte_names_are_cut_on_character_boundaries() {
        with_frame(|frame| {
            let mut scratch = [0u8; 96];
            let name = "\u{e9}t\u{e9} \u{e0} la plage avec des amis.jpeg";
            for max in (10..=150).step_by(5) {
                let shown = fit_name(frame, name, 10, FontWeight::Regular, max, &mut scratch);
                assert!(width(frame, shown) <= max || shown == ELLIPSIS);
            }
        });
    }

    #[test]
    fn a_name_longer_than_the_scratch_buffer_still_fits() {
        with_frame(|frame| {
            let mut scratch = [0u8; 12];
            let shown = fit_name(frame, "a-name-much-longer-than-the-scratch.png", 10, FontWeight::Regular, 60, &mut scratch);
            assert!(width(frame, shown) <= 60);
        });
    }
}
