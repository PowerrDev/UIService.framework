# Voyager

sevOS's Finder-equivalent: a folder browser with a sidebar, a toolbar, list
and icon views, and a status line, styled after macOS Finder's light
appearance.

- **Toolbar** — sidebar toggle, back/forward chevrons, the current folder's icon
  and name, an icons/list segmented control and an Open button (which gives way
  in a narrow window). It shares the window titlebar's colour, so the two read
  as one band.
- **Sidebar** — "Favorites" and "Locations" groups of real folders on the
  volume, with a blue glyph each. The current folder's entry is highlighted
  with Finder's soft grey pill while its own folder is showing.
- **List view** — column headers over a hairline, striped rows that continue
  down the empty part of the window, a Finder-style icon per file kind, kind and
  right-aligned size columns (the Kind column drops out of a narrow window),
  and a disclosure chevron on folders. A selected row is a rounded accent bar
  with white text.
- **Icon view** — 44 pt icons over their names; the selected one gets a soft
  plate and an accent pill behind its name. Long names are cut in the middle so
  the extension stays visible (`Release n…es.md`).
- **Status line** — "12 items", or which one is selected and how big it is.

It takes its cue from armOS's Ignite
(`armOS/frameworks/CoreServices/Ignite.app`) but is not built on Ignite's
foundation — `ui::prelude` is a materially different, much smaller platform.
Against `ui-abi`/`ui-core` as they exist today:

- `Canvas` has no path or stroke primitive, only filled rects, hard-edged
  rounded rects and a circle. `draw2d` adds the missing pieces on top of
  `Canvas::blend_pixel` — anti-aliased strokes, convex polygons and rounded
  rects/outlines — using integer maths only (the target is soft-float). `icons`
  draws every icon and glyph with them, procedurally, so they are crisp at any
  content scale and no bitmaps ship.
- Input is the pointer (`PointerMoved`/`Down`/`Up`/`Left`) and the scroll wheel
  (`Event::Scroll`); there is no keyboard event, so no typed search, renaming or
  keyboard navigation.
- **Scrolling is smooth.** A wheel notch (or each small delta of a trackpad
  swipe) moves a target and the view eases towards it from `App::tick`, an
  exponential approach on the runtime's real microsecond clock, so a stream of
  small deltas is one fluid motion and a single notch glides. The list keeps its
  column header sticky; a slim overlay indicator shows while scrolling and fades
  out. It works in both views, the range follows the window size, and going to
  another folder or view starts at the top. A folder of up to 256 entries is
  browsable (the status line says "more not shown" beyond that).
- The directory listing comes from the host's `UI_SERVICE_HOST_CAP_FS`
  capability (`ui_core::fs::list_directory`); NXU's host implements it in
  `platform/arm64/services/ui_service.c` over the kernel VFS.

## The window

`WindowConfig` sizes are a *doubled 2x reference* (see
`WindowConfig::effective_size`): the 860 x 600 in `lib.rs` is a **430 x 300 pt**
window, 268 pt of it content under the 32 pt titlebar, resizable from
240 x 180 pt up to the 860 x 600 px backing store. Every metric here is a
1x-point value run through `ui_core::scale::pt`, sized for the default window and
degrading towards the smallest. `layout` is the one place geometry lives.

## Module map

- `theme` — palette, metrics, and text helpers (vertical centring, end and
  middle truncation).
- `draw2d` — the anti-aliased shape toolkit described above.
- `icons` — file icons (folder, image, audio, document, generic page) and
  glyphs (chevrons, view modes, sidebar toggle, drive).
- `model` — `Entry`/`EntryKind`, the sidebar destinations and their sections.
- `nav` — the directory path stack and the current directory's live listing.
- `scroll` — the smooth-scrolling state: target, eased offset and the overlay
  indicator's fade, all integer maths, with no idea of rows or windows.
- `layout` — one pure `Size -> Layout` geometry pass, called identically by
  `draw` (to paint) and `event` (to hit-test), since `event` has no `Frame`
  to measure text with.
- `toolbar`, `sidebar`, `content`, `status` — one region each: drawing plus
  (where interactive) hit testing.
- `storage` — the `StaticCell` that keeps big buffers off the stack.

## Stack use

`VoyagerApp` is a stack local on the kernel's **16 KiB boot stack**, and the
memory just below that stack is the kernel's own state. A large field here
(an embedded path stack was once 6.4 KiB of it) does not fail in this crate: it
overflows into the kernel and crashes it at start-up (`vm_kern: DEBUG entry
check failed`). Big buffers belong in `storage::StaticCell` statics, and a test
in `lib.rs` pins the app's size.

## Trying it without a kernel boot

`examples/voyager` (`voyager-preview`) runs the real app on the host with the
real Inter text renderer, a small sample filesystem behind the real
`ui::core::fs` hook, and the same event -> redraw loop the NXU runtime runs, then
writes one PPM per scenario: the list and icon views, selection, hover and press
feedback, a folder of every file kind, the sidebar hidden, and the smallest and
largest windows.

```sh
cargo run -p voyager-preview -- /tmp/voyager-shots 1000 2000   # 1x and 2x
cargo test -p voyager
```
