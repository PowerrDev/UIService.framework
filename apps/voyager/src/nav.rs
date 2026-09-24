//! Directory path stack plus the current directory's live listing, backed
//! by the real filesystem through `ui_core::fs`. No heap exists in this
//! `#![no_std]` app, so both the path stack and the entry buffer are
//! fixed-capacity (`MAX_DEPTH`, `MAX_ENTRIES`) rather than growable
//! collections -- see `storage::StaticCell` for why both live in statics
//! instead of embedded in `Navigator` itself: the path stack alone is about
//! 6.4 KiB, and `VoyagerApp` is a stack local on the 16 KiB boot stack, whose
//! neighbour in memory is the kernel's own state.

use ui::core::fs;

use crate::model::{self, Destination, Entry, DEFAULT_DESTINATION, DESTINATIONS};
use crate::storage::StaticCell;

pub const MAX_DEPTH: usize = 32;
/// How many entries of one directory `Navigator` will hold at once. Both
/// buffers are statics (about 40 KiB between them), so this is a memory
/// budget, not a screen size: the view scrolls. A directory with more than
/// this is only partially browsable -- see `truncated`.
pub const MAX_ENTRIES: usize = 256;
const PATH_MAX: usize = 200;

static RAW_ENTRIES: StaticCell<[fs::DirEntry; MAX_ENTRIES]> =
    StaticCell::new([fs::DirEntry::empty(); MAX_ENTRIES]);
static ENTRIES: StaticCell<[Entry; MAX_ENTRIES]> =
    StaticCell::new([Entry::empty(); MAX_ENTRIES]);
static PATHS: StaticCell<[PathBuf; MAX_DEPTH]> =
    StaticCell::new([PathBuf::empty(); MAX_DEPTH]);

#[derive(Clone, Copy)]
struct PathBuf {
    buf: [u8; PATH_MAX],
    len: u8,
}

impl PathBuf {
    const fn empty() -> Self {
        Self { buf: [0; PATH_MAX], len: 0 }
    }

    fn from_str(value: &str) -> Self {
        let mut buf = [0u8; PATH_MAX];
        let len = value.len().min(PATH_MAX);
        buf[..len].copy_from_slice(&value.as_bytes()[..len]);
        Self { buf, len: len as u8 }
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len as usize]).unwrap_or("/")
    }

    /// This path with `name` appended as a new final component.
    fn joined(&self, name: &str) -> Self {
        let base = self.as_str();
        let mut buf = [0u8; PATH_MAX];
        let base_bytes = base.as_bytes();
        let mut len = base_bytes.len().min(PATH_MAX);
        buf[..len].copy_from_slice(&base_bytes[..len]);

        if (len == 0 || buf[len - 1] != b'/') && len < PATH_MAX {
            buf[len] = b'/';
            len += 1;
        }

        let name_bytes = name.as_bytes();
        let copy_len = name_bytes.len().min(PATH_MAX - len);
        buf[len..len + copy_len].copy_from_slice(&name_bytes[..copy_len]);
        len += copy_len;

        Self { buf, len: len as u8 }
    }

    /// The last path component -- what a breadcrumb/title should show for
    /// this level absent a friendlier `Destination` label. Falls back to
    /// the whole path (e.g. "/disk") when there is no "/" to split on other
    /// than a leading one.
    fn label(&self) -> &str {
        let path = self.as_str();
        let trimmed = path.trim_end_matches('/');
        match trimmed.rsplit_once('/') {
            Some((_, last)) if !last.is_empty() => last,
            _ => path,
        }
    }
}

pub struct Navigator {
    depth: usize,
    forward: Option<PathBuf>,
    entry_count: usize,
    truncated: bool,
}

impl Navigator {
    pub fn new() -> Self {
        let mut navigator = Self {
            depth: 1,
            forward: None,
            entry_count: 0,
            truncated: false,
        };
        navigator.stack()[0] = PathBuf::from_str(DESTINATIONS[DEFAULT_DESTINATION].path);
        navigator.refresh();
        navigator
    }

    /// The path stack: `depth` entries of it are live. It lives in a static,
    /// not in `Navigator`; see the module doc.
    fn stack(&self) -> &mut [PathBuf; MAX_DEPTH] {
        unsafe { PATHS.get_mut() }
    }

    fn current_path(&self) -> &str {
        self.stack()[self.depth - 1].as_str()
    }

    /// The directory being shown.
    pub fn path(&self) -> &str {
        self.current_path()
    }

    /// A destination's own root path shows its friendly label ("Disk")
    /// instead of the raw last path component ("disk"); anything navigated
    /// into below that just shows its own name.
    pub fn title(&self) -> &str {
        let current = self.current_path();
        for destination in DESTINATIONS {
            if destination.path == current {
                return destination.label;
            }
        }
        self.stack()[self.depth - 1].label()
    }

    pub fn entries(&self) -> &[Entry] {
        let entries = unsafe { ENTRIES.get_mut() };
        &entries[..self.entry_count]
    }

    /// Whether the current directory held more entries than `MAX_ENTRIES`
    /// could hold -- surfaced in the status bar rather than silently
    /// dropped, since there is no scrolling to fall back on either.
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    fn refresh(&mut self) {
        let raw = unsafe { RAW_ENTRIES.get_mut() };
        match fs::list_directory(self.current_path(), raw) {
            Ok(listing) => {
                let entries = unsafe { ENTRIES.get_mut() };
                for (slot, raw_entry) in entries.iter_mut().zip(raw.iter()).take(listing.count) {
                    *slot = model::entry_from_raw(raw_entry);
                }
                self.entry_count = listing.count;
                self.truncated = listing.truncated;
            }
            Err(_) => {
                self.entry_count = 0;
                self.truncated = false;
            }
        }
    }

    pub fn can_go_back(&self) -> bool {
        self.depth > 1
    }

    pub fn can_go_forward(&self) -> bool {
        self.forward.is_some()
    }

    pub fn go_back(&mut self) -> bool {
        if !self.can_go_back() {
            return false;
        }
        self.forward = Some(self.stack()[self.depth - 1]);
        self.depth -= 1;
        self.refresh();
        true
    }

    pub fn go_forward(&mut self) -> bool {
        let Some(path) = self.forward.take() else {
            return false;
        };
        if self.depth >= MAX_DEPTH {
            return false;
        }
        self.stack()[self.depth] = path;
        self.depth += 1;
        self.refresh();
        true
    }

    /// Enter `entry` if it is a folder. No-op (returns `false`) for a file,
    /// or once the path stack is as deep as it will go.
    pub fn enter(&mut self, entry: &Entry) -> bool {
        if !entry.is_folder() || self.depth >= MAX_DEPTH {
            return false;
        }
        let next = self.stack()[self.depth - 1].joined(entry.name());
        self.stack()[self.depth] = next;
        self.depth += 1;
        self.forward = None;
        self.refresh();
        true
    }

    pub fn go_to_destination(&mut self, destination: &Destination) {
        self.stack()[0] = PathBuf::from_str(destination.path);
        self.depth = 1;
        self.forward = None;
        self.refresh();
    }
}

// `Navigator`'s own behavior (back/forward, entering a folder) now depends
// on a real host filesystem connection that these host-target tests have
// no way to provide (`fs::list_directory` just reports `Unavailable`
// without one) -- what's left to unit-test without one is `PathBuf`'s own
// manipulation, which is exactly the part real filesystem paths made
// non-trivial (arbitrary depth, no known-at-compile-time shape).
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joined_appends_a_separator_when_missing() {
        let base = PathBuf::from_str("/disk");
        assert_eq!(base.joined("System").as_str(), "/disk/System");
    }

    #[test]
    fn joined_does_not_double_an_existing_trailing_separator() {
        let base = PathBuf::from_str("/disk/");
        assert_eq!(base.joined("System").as_str(), "/disk/System");
    }

    #[test]
    fn label_is_the_last_path_component() {
        assert_eq!(PathBuf::from_str("/disk/System/Library").label(), "Library");
    }

    #[test]
    fn label_is_the_last_component_even_directly_under_root() {
        assert_eq!(PathBuf::from_str("/disk").label(), "disk");
    }

    #[test]
    fn label_falls_back_to_the_whole_path_at_the_root_itself() {
        assert_eq!(PathBuf::from_str("/").label(), "/");
    }
}
