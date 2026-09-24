//! Voyager's file/folder model, backed by the real filesystem through
//! `ui_core::fs` (the host's `UI_SERVICE_HOST_CAP_FS` capability -- see
//! `nav::Navigator`, which owns actually calling `fs::list_directory` and
//! turning its raw entries into these).
//!
//! Names and counts are unbounded on a real volume, so unlike the static
//! demo tree this replaced, `Entry` owns a fixed-capacity copy of its name
//! (truncated past `NAME_MAX`, matching the host ABI's own truncation) and
//! `Navigator` caps how many entries of one directory it will ever hold at
//! once (`nav::MAX_ENTRIES`) -- Voyager also has no scrolling yet, so a
//! directory deeper than that or than fits on screen is only partially
//! browsable regardless.

use crate::icons::FileIcon;

pub const NAME_MAX: usize = ui::core::fs::NAME_MAX;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Folder,
    Image,
    Audio,
    Document,
    /// Anything not recognized by extension -- shown as a plain "File"
    /// rather than guessed at, since there is no executable-bit or
    /// content-sniffing check backing "Application"/etc. here.
    Other,
}

impl EntryKind {
    pub const fn label(self) -> &'static str {
        match self {
            EntryKind::Folder => "Folder",
            EntryKind::Image => "Image",
            EntryKind::Audio => "Audio",
            EntryKind::Document => "Document",
            EntryKind::Other => "File",
        }
    }

    /// The Finder-style picture this kind of entry gets in lists and the grid.
    pub const fn icon(self) -> FileIcon {
        match self {
            EntryKind::Folder => FileIcon::Folder,
            EntryKind::Image => FileIcon::Image,
            EntryKind::Audio => FileIcon::Audio,
            EntryKind::Document => FileIcon::Document,
            EntryKind::Other => FileIcon::Generic,
        }
    }

    /// Classify a non-directory entry by extension. Deliberately narrow --
    /// a short, easy-to-audit list beats a clever guess that's wrong for
    /// something a user actually has on disk.
    fn from_extension(name: &str) -> Self {
        let extension = match name.rsplit_once('.') {
            Some((_, extension)) if !extension.is_empty() => extension,
            _ => return EntryKind::Other,
        };

        const IMAGE: [&str; 6] = ["png", "jpg", "jpeg", "gif", "bmp", "webp"];
        const AUDIO: [&str; 6] = ["wav", "mp3", "m4a", "aiff", "flac", "ogg"];
        const DOCUMENT: [&str; 6] = ["txt", "md", "pdf", "doc", "docx", "rtf"];

        if matches_case_insensitive(extension, &IMAGE) {
            EntryKind::Image
        } else if matches_case_insensitive(extension, &AUDIO) {
            EntryKind::Audio
        } else if matches_case_insensitive(extension, &DOCUMENT) {
            EntryKind::Document
        } else {
            EntryKind::Other
        }
    }
}

/// `str::eq_ignore_ascii_case` without needing an owned, lowercased copy --
/// there is no allocator here to hand one back from.
fn matches_case_insensitive(value: &str, candidates: &[&str]) -> bool {
    candidates.iter().any(|candidate| value.eq_ignore_ascii_case(candidate))
}

#[derive(Clone, Copy)]
pub struct Entry {
    name_buf: [u8; NAME_MAX],
    name_len: u8,
    pub kind: EntryKind,
    pub size_bytes: u64,
}

impl Entry {
    pub const fn empty() -> Self {
        Self { name_buf: [0; NAME_MAX], name_len: 0, kind: EntryKind::Other, size_bytes: 0 }
    }

    fn from_dir_entry(raw: &ui::core::fs::DirEntry) -> Self {
        let name = raw.name();
        let mut name_buf = [0u8; NAME_MAX];
        let name_len = name.len().min(NAME_MAX);
        name_buf[..name_len].copy_from_slice(&name.as_bytes()[..name_len]);

        let kind = if raw.is_directory() {
            EntryKind::Folder
        } else {
            EntryKind::from_extension(name)
        };

        Self {
            name_buf,
            name_len: name_len as u8,
            kind,
            size_bytes: raw.size_bytes,
        }
    }

    pub fn name(&self) -> &str {
        core::str::from_utf8(&self.name_buf[..self.name_len as usize]).unwrap_or("")
    }

    pub const fn is_folder(&self) -> bool {
        matches!(self.kind, EntryKind::Folder)
    }
}

/// Build an `Entry` from one raw host listing result. `pub(crate)` rather
/// than a `From`/`TryFrom` impl since it is only ever called from
/// `nav::Navigator::refresh`.
pub(crate) fn entry_from_raw(raw: &ui::core::fs::DirEntry) -> Entry {
    Entry::from_dir_entry(raw)
}

/// Render a byte count the way the status bar and detail line want it: whole
/// units only, written into a caller-owned scratch buffer since there is no
/// allocator to hand back an owned string.
pub fn format_size(bytes: u64, scratch: &mut [u8; 20]) -> &str {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;

    let (value, suffix) = if bytes >= GIB {
        (bytes / GIB, " GB")
    } else if bytes >= MIB {
        (bytes / MIB, " MB")
    } else if bytes >= KIB {
        (bytes / KIB, " KB")
    } else {
        (bytes, " B")
    };

    write_u64(value, suffix, scratch)
}

fn write_u64<'a>(value: u64, suffix: &str, scratch: &'a mut [u8; 20]) -> &'a str {
    let mut digits = [0u8; 20];
    let mut count = 0;
    let mut remaining = value;

    loop {
        digits[count] = b'0' + (remaining % 10) as u8;
        remaining /= 10;
        count += 1;
        if remaining == 0 {
            break;
        }
    }

    let mut written = 0;
    for index in (0..count).rev() {
        scratch[written] = digits[index];
        written += 1;
    }
    for byte in suffix.bytes() {
        scratch[written] = byte;
        written += 1;
    }

    core::str::from_utf8(&scratch[..written]).unwrap_or("")
}

/// What a sidebar destination's glyph looks like.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DestinationIcon {
    Folder,
    Drive,
}

#[derive(Clone, Copy)]
pub struct Destination {
    pub label: &'static str,
    pub path: &'static str,
    /// Index into [`SECTIONS`]. Destinations of one section are contiguous in
    /// [`DESTINATIONS`], in section order.
    pub section: usize,
    pub icon: DestinationIcon,
}

/// The sidebar's group headings, like Finder's "Favorites" and "Locations".
pub const SECTIONS: &[&str] = &["Favorites", "Locations"];

/// There is no real per-user home directory concept on sevOS yet, so these
/// point at what actually exists on the mounted volume (see
/// `assets/DiskRoot` in the nxu repo) instead of a `Documents`/`Downloads`
/// shape borrowed from a real desktop OS that would just be empty here.
pub const DESTINATIONS: &[Destination] = &[
    Destination { label: "System", path: "/disk/System", section: 0, icon: DestinationIcon::Folder },
    Destination { label: "Library", path: "/disk/System/Library", section: 0, icon: DestinationIcon::Folder },
    Destination { label: "Resources", path: "/disk/System/Library/Resources", section: 0, icon: DestinationIcon::Folder },
    Destination { label: "Disk", path: "/disk", section: 1, icon: DestinationIcon::Drive },
];

/// The volume's root: what Voyager opens to.
pub const DEFAULT_DESTINATION: usize = 3;

/// The sidebar entry that *is* `path`, if there is one. Like Finder, the
/// sidebar highlights a destination only while its own folder is the one
/// showing, not any folder beneath it.
pub fn destination_for_path(path: &str) -> Option<usize> {
    DESTINATIONS.iter().position(|destination| destination.path == path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_follow_the_extension_ignoring_case() {
        assert!(EntryKind::from_extension("Boot_Audio.WAV") == EntryKind::Audio);
        assert!(EntryKind::from_extension("logo.png") == EntryKind::Image);
        assert!(EntryKind::from_extension("notes.md") == EntryKind::Document);
        assert!(EntryKind::from_extension("Makefile") == EntryKind::Other);
        assert!(EntryKind::from_extension("archive.tar.zst") == EntryKind::Other);
    }

    #[test]
    fn a_destination_is_found_by_its_exact_path_only() {
        assert_eq!(destination_for_path("/disk"), Some(DEFAULT_DESTINATION));
        assert_eq!(destination_for_path("/disk/System"), Some(0));
        assert_eq!(destination_for_path("/disk/System/Library/CoreServices"), None);
        assert_eq!(destination_for_path("/nowhere"), None);
    }

    #[test]
    fn destinations_are_grouped_by_section_in_order() {
        for pair in DESTINATIONS.windows(2) {
            assert!(pair[0].section <= pair[1].section);
        }
        assert!(DESTINATIONS.iter().all(|destination| destination.section < SECTIONS.len()));
    }
}
