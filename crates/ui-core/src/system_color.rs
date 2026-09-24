//! Semantic system colors and the contrast rules they are held to.
//!
//! Apple's Human Interface Guidelines ask apps to use *semantic* colors
//! (`labelColor`, `secondaryLabelColor`, `controlAccentColor`, ...) instead
//! of hand-picked values, so every app and the system chrome agree on what
//! "primary text" or "the accent" looks like. These are sevOS's equivalents.
//! App-specific surfaces (a sidebar, a stripe) can still be their own
//! colors, but text and glyphs drawn on them should come from here.
//!
//! Every foreground below is checked (see the tests) against the surfaces it
//! is meant for, using the WCAG 2.x contrast ratio that the HIG's own
//! accessibility guidance points to:
//!
//! - text: at least 4.5:1 (WCAG 1.4.3, AA);
//! - glyphs and other non-text a person needs to see: at least 3:1
//!   (WCAG 1.4.11);
//! - disabled content is exempt from both (WCAG 1.4.3's "inactive user
//!   interface component" exception), which is exactly why
//!   [`DISABLED_LABEL`] must never be used for anything that still works.
//!
//! Only a light appearance exists so far; a dark one would be a second set of
//! these same names, not new names.

use crate::Color;

/// Minimum contrast for text (WCAG 1.4.3 AA).
pub const MIN_TEXT_CONTRAST: f32 = 4.5;
/// Minimum contrast for meaningful non-text: glyphs, focus/selection
/// indicators (WCAG 1.4.11).
pub const MIN_GRAPHIC_CONTRAST: f32 = 3.0;

// ---- surfaces --------------------------------------------------------------

/// A window's content background.
pub const WINDOW_BACKGROUND: Color = Color::rgb(250, 250, 251);
/// A document/list background: plain white.
pub const CONTENT_BACKGROUND: Color = Color::WHITE;
/// Titlebars and toolbars.
pub const TITLEBAR: Color = Color::rgb(225, 226, 230);
/// The system menu bar.
pub const MENUBAR: Color = Color::rgb(238, 238, 234);
/// A hairline between regions. Decorative: not held to a contrast minimum.
pub const SEPARATOR: Color = Color::rgb(208, 210, 216);

// ---- text and glyphs -------------------------------------------------------

/// Primary text: titles, labels, file names, menu titles.
pub const LABEL: Color = Color::rgb(28, 29, 32);
/// Secondary text: column headings, captions, sizes, the status line, section
/// headings in a sidebar. Still has to be readable: 4.5:1 on every surface
/// above, including the titlebar.
pub const SECONDARY_LABEL: Color = Color::rgb(97, 99, 107);
/// Glyphs that carry meaning but are not text (a disclosure chevron, a
/// toolbar icon's lighter parts): 3:1 on the light surfaces. Not for text.
pub const TERTIARY_LABEL: Color = Color::rgb(138, 140, 147);
/// Content that is *disabled* -- a greyed-out Back button, an unavailable
/// menu item. Deliberately below the minimums, since that is what tells a
/// person it does not work right now. Never use it for something enabled.
pub const DISABLED_LABEL: Color = Color::rgb(170, 173, 181);

// ---- accent ----------------------------------------------------------------

/// The system accent: default buttons, selection, sidebar glyphs. Dark enough
/// that white text on it clears 4.5:1 (the old 55,115,245 only reached 4.26:1).
pub const ACCENT: Color = Color::rgb(49, 102, 218);
/// The accent under the pointer: a step lighter, still 4.5:1 under white text.
pub const ACCENT_HOVER: Color = Color::rgb(56, 110, 228);
/// The accent while pressed or while a selection is being clicked again.
pub const ACCENT_PRESSED: Color = Color::rgb(42, 87, 186);
/// Text and glyphs on any accent fill. Plain white, secondary columns
/// included: no tint of white reaches 4.5:1 on [`ACCENT`].
pub const ON_ACCENT: Color = Color::WHITE;

/// sRGB channel value to linear light, scaled to 0..=65535: IEC 61966-2-1's
/// transfer function, precomputed because `no_std` has no `powf`.
const LINEAR: [u16; 256] = [
    0, 20, 40, 60, 80, 99, 119, 139, 159, 179, 199, 219, 241, 264, 288, 313,
        340, 367, 396, 427, 458, 491, 526, 562, 599, 637, 677, 718, 761, 805, 851, 898,
        947, 997, 1048, 1101, 1156, 1212, 1270, 1330, 1391, 1453, 1517, 1583, 1651, 1720, 1790, 1863,
        1937, 2013, 2090, 2170, 2250, 2333, 2418, 2504, 2592, 2681, 2773, 2866, 2961, 3058, 3157, 3258,
        3360, 3464, 3570, 3678, 3788, 3900, 4014, 4129, 4247, 4366, 4488, 4611, 4736, 4864, 4993, 5124,
        5257, 5392, 5530, 5669, 5810, 5953, 6099, 6246, 6395, 6547, 6700, 6856, 7014, 7174, 7335, 7500,
        7666, 7834, 8004, 8177, 8352, 8528, 8708, 8889, 9072, 9258, 9445, 9635, 9828, 10022, 10219, 10417,
        10619, 10822, 11028, 11235, 11446, 11658, 11873, 12090, 12309, 12530, 12754, 12980, 13209, 13440, 13673, 13909,
        14146, 14387, 14629, 14874, 15122, 15371, 15623, 15878, 16135, 16394, 16656, 16920, 17187, 17456, 17727, 18001,
        18277, 18556, 18837, 19121, 19407, 19696, 19987, 20281, 20577, 20876, 21177, 21481, 21787, 22096, 22407, 22721,
        23038, 23357, 23678, 24002, 24329, 24658, 24990, 25325, 25662, 26001, 26344, 26688, 27036, 27386, 27739, 28094,
        28452, 28813, 29176, 29542, 29911, 30282, 30656, 31033, 31412, 31794, 32179, 32567, 32957, 33350, 33745, 34143,
        34544, 34948, 35355, 35764, 36176, 36591, 37008, 37429, 37852, 38278, 38706, 39138, 39572, 40009, 40449, 40891,
        41337, 41785, 42236, 42690, 43147, 43606, 44069, 44534, 45002, 45473, 45947, 46423, 46903, 47385, 47871, 48359,
        48850, 49344, 49841, 50341, 50844, 51349, 51858, 52369, 52884, 53401, 53921, 54445, 54971, 55500, 56032, 56567,
        57105, 57646, 58190, 58737, 59287, 59840, 60396, 60955, 61517, 62082, 62650, 63221, 63795, 64372, 64952, 65535,
];

impl Color {
    /// Relative luminance (WCAG 2.x), 0.0 for black to 1.0 for white. Alpha
    /// is ignored: blend onto the real background first.
    pub fn relative_luminance(self) -> f32 {
        let red = LINEAR[self.red as usize] as f32;
        let green = LINEAR[self.green as usize] as f32;
        let blue = LINEAR[self.blue as usize] as f32;
        (0.2126 * red + 0.7152 * green + 0.0722 * blue) / 65535.0
    }

    /// WCAG 2.x contrast ratio against `other`, from 1.0 (identical) to 21.0
    /// (black on white). Symmetric. Compare it with [`MIN_TEXT_CONTRAST`] /
    /// [`MIN_GRAPHIC_CONTRAST`].
    pub fn contrast_ratio(self, other: Color) -> f32 {
        let a = self.relative_luminance();
        let b = other.relative_luminance();
        let (lighter, darker) = if a >= b { (a, b) } else { (b, a) };
        (lighter + 0.05) / (darker + 0.05)
    }

    /// Whichever of [`LABEL`] and white reads better on `self`: for text over
    /// a surface whose color is only known at runtime (a wallpaper, a
    /// user-chosen tint).
    pub fn best_label(self) -> Color {
        if self.contrast_ratio(LABEL) >= self.contrast_ratio(Color::WHITE) {
            LABEL
        } else {
            Color::WHITE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_at_least(foreground: Color, background: Color, minimum: f32, what: &str) {
        let ratio = foreground.contrast_ratio(background);
        assert!(ratio >= minimum, "{what}: {ratio:.2}:1, below {minimum}:1");
    }

    #[test]
    fn the_contrast_formula_matches_its_reference_points() {
        assert!((Color::BLACK.contrast_ratio(Color::WHITE) - 21.0).abs() < 0.01);
        assert!((Color::WHITE.contrast_ratio(Color::WHITE) - 1.0).abs() < 0.001);
        // #767676 on white is the well-known 4.54:1 grey.
        let grey = Color::rgb(0x76, 0x76, 0x76);
        assert!((grey.contrast_ratio(Color::WHITE) - 4.54).abs() < 0.01);
        assert_eq!(grey.contrast_ratio(Color::WHITE), Color::WHITE.contrast_ratio(grey));
    }

    #[test]
    fn text_colors_are_readable_on_every_system_surface() {
        for (surface, name) in [
            (WINDOW_BACKGROUND, "window"),
            (CONTENT_BACKGROUND, "content"),
            (TITLEBAR, "titlebar"),
            (MENUBAR, "menu bar"),
        ] {
            assert_at_least(LABEL, surface, MIN_TEXT_CONTRAST, name);
            assert_at_least(SECONDARY_LABEL, surface, MIN_TEXT_CONTRAST, name);
            assert_at_least(ACCENT, surface, MIN_GRAPHIC_CONTRAST, name);
        }
    }

    #[test]
    fn tertiary_glyphs_are_visible_on_content_surfaces() {
        assert_at_least(TERTIARY_LABEL, CONTENT_BACKGROUND, MIN_GRAPHIC_CONTRAST, "content");
        assert_at_least(TERTIARY_LABEL, WINDOW_BACKGROUND, MIN_GRAPHIC_CONTRAST, "window");
    }

    #[test]
    fn text_on_the_accent_is_readable_in_every_state() {
        for (fill, name) in [(ACCENT, "idle"), (ACCENT_HOVER, "hover"), (ACCENT_PRESSED, "pressed")] {
            assert_at_least(ON_ACCENT, fill, MIN_TEXT_CONTRAST, name);
        }
    }

    #[test]
    fn disabled_content_reads_as_disabled() {
        // The point of the exemption: disabled must look weaker than enabled.
        assert!(DISABLED_LABEL.contrast_ratio(TITLEBAR) < TERTIARY_LABEL.contrast_ratio(TITLEBAR));
    }

    #[test]
    fn best_label_picks_the_readable_one() {
        assert_eq!(Color::WHITE.best_label(), LABEL);
        assert_eq!(Color::rgb(20, 27, 41).best_label(), Color::WHITE);
        assert_eq!(ACCENT.best_label(), Color::WHITE);
    }
}
