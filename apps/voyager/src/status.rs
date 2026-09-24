//! The status line, centred like Finder's: "12 items", or which one is
//! selected. There is no free-space figure: the host does not report volume
//! usage yet, and an "unavailable" placeholder in the corner reads as a bug.

use ui::prelude::*;

use crate::layout::Layout;
use crate::model::{format_size, Entry};
use crate::theme;

pub fn draw(frame: &mut Frame<'_>, layout: &Layout, entries: &[Entry], selected: Option<usize>, truncated: bool) {
    frame.fill_rect(layout.status, theme::STATUS_BACKGROUND);
    frame.fill_rect(layout.status_divider, theme::DIVIDER);

    let mut size_scratch = [0u8; 20];
    let mut detail = [0u8; 96];
    let text: &str = match selected.and_then(|index| entries.get(index).map(|entry| (index, entry))) {
        Some((_, entry)) if entry.is_folder() => selection_text(&mut detail, entry.name(), None, entries.len()),
        Some((_, entry)) => {
            let size_text = format_size(entry.size_bytes, &mut size_scratch);
            selection_text(&mut detail, entry.name(), Some(size_text), entries.len())
        }
        None => item_count_text(&mut detail, entries.len(), truncated),
    };

    let point_size = theme::caption_point_size();
    let measured = frame.measure(text, point_size);
    let x = layout.status.origin.x + (layout.status.size.width.saturating_sub(measured.width) / 2) as i32;
    theme::text_in(frame, layout.status, x, text, theme::TEXT_SECONDARY, point_size, FontWeight::Regular);
}

/// `"name" selected` for a folder, `"name" selected, 41 B` for a file.
fn selection_text<'a>(scratch: &'a mut [u8; 96], name: &str, size: Option<&str>, _count: usize) -> &'a str {
    match size {
        Some(size) => join(scratch, &["\u{201C}", name, "\u{201D} selected, ", size]),
        None => join(scratch, &["\u{201C}", name, "\u{201D} selected"]),
    }
}

fn item_count_text<'a>(scratch: &'a mut [u8; 96], count: usize, truncated: bool) -> &'a str {
    let mut digits = [0u8; 10];
    let mut remaining = count as u32;
    let mut digit_count = 0;
    loop {
        digits[digit_count] = b'0' + (remaining % 10) as u8;
        remaining /= 10;
        digit_count += 1;
        if remaining == 0 {
            break;
        }
    }

    let mut written = 0;
    for index in (0..digit_count).rev() {
        scratch[written] = digits[index];
        written += 1;
    }

    let suffix = if count == 1 { " item" } else { " items" };
    for byte in suffix.bytes() {
        scratch[written] = byte;
        written += 1;
    }

    // A directory bigger than `nav::MAX_ENTRIES` is only partially
    // browsable -- worth saying so rather than silently showing an
    // incomplete list as if it were the whole thing.
    if truncated {
        for byte in " (more not shown)".bytes() {
            if written >= scratch.len() {
                break;
            }
            scratch[written] = byte;
            written += 1;
        }
    }

    core::str::from_utf8(&scratch[..written]).unwrap_or("")
}

/// Concatenate `parts` into `scratch`, cut at a character boundary if they do
/// not all fit.
fn join<'a>(scratch: &'a mut [u8; 96], parts: &[&str]) -> &'a str {
    let mut written = 0;
    for part in parts {
        for character in part.chars() {
            let mut buffer = [0u8; 4];
            let encoded = character.encode_utf8(&mut buffer).as_bytes();
            if written + encoded.len() > scratch.len() {
                return core::str::from_utf8(&scratch[..written]).unwrap_or("");
            }
            scratch[written..written + encoded.len()].copy_from_slice(encoded);
            written += encoded.len();
        }
    }
    core::str::from_utf8(&scratch[..written]).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_pluralised() {
        let mut scratch = [0u8; 96];
        assert_eq!(item_count_text(&mut scratch, 0, false), "0 items");
        assert_eq!(item_count_text(&mut scratch, 1, false), "1 item");
        assert_eq!(item_count_text(&mut scratch, 12, false), "12 items");
        assert_eq!(item_count_text(&mut scratch, 48, true), "48 items (more not shown)");
    }

    #[test]
    fn join_never_splits_a_character() {
        let mut scratch = [0u8; 96];
        let long = "\u{2014}".repeat(40);
        let joined = join(&mut scratch, &[&long]);
        assert!(joined.len() <= 96);
        assert!(joined.chars().all(|character| character == '\u{2014}'));
    }

    #[test]
    fn selection_text_names_the_item_and_its_size() {
        let mut scratch = [0u8; 96];
        assert_eq!(selection_text(&mut scratch, "notes.txt", Some("41 B"), 2), "\u{201C}notes.txt\u{201D} selected, 41 B");
        assert_eq!(selection_text(&mut scratch, "System", None, 2), "\u{201C}System\u{201D} selected");
    }
}
