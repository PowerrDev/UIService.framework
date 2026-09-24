# Public Rust API

## Start here

Normal applications should need one import:

```rust
use ui::prelude::*;
```

The prelude intentionally stays small. It contains:

- application types: `App`, `AppInfo`, `WindowConfig`, `AppAction`, `Frame`,
  `FontWeight`;
- geometry/input: `Color`, `Point`, `Size`, `Rect`, `Event`, `PointerButton`;
- `scale` (points to physical pixels) and `system_color` (semantic colors);
- reusable app widgets such as `Button`;
- public window styling through `WindowStyle`;
- cursor/assets helpers that are useful to applications.

Low-level drawing primitives such as `Canvas`, `Surface`, `CanvasView` and the
`TextRenderer` trait live under `ui::render`. They are available when a custom
widget or framework component needs them, but normal app code should work
through `Frame` instead.

The app-facing API deliberately does **not** expose `HostV1`, `HostV2`, ABI
capability flags, NXU callback types, damage queues or platform framebuffer
ownership.

## `App`

```rust
pub trait App {
    const INFO: AppInfo<'static>;
    const WINDOW: WindowConfig;

    fn draw(&mut self, ui: &mut Frame<'_>);
    fn event(&mut self, event: Event, content_size: Size) -> AppAction;
    fn animating(&self) -> bool;
    fn tick(&mut self, now_us: u64) -> AppAction;
}
```

Only `draw` is required: `event` defaults to `AppAction::None`, and an app that
never animates leaves `animating` at its default `false`, so the runtime does no
per-frame work for it. An app that does (smooth scrolling, a fade) returns `true`
from `animating` while it has an animation going and advances it in `tick`,
which the runtime calls once per frame with a monotonic microsecond clock and
the real time that has passed, not a fixed step.

`App::INFO` is the app's stable identity. Prefer reverse-DNS identifiers such
as `com.butterscotch.shark` and do not derive behavior from the display name.

## `Frame`

`Frame` is the ordinary drawing API. Common operations are deliberately short:

```rust
fn draw(&mut self, ui: &mut Frame<'_>) {
    ui.fill(system_color::WINDOW_BACKGROUND);
    ui.fill_rounded_rect(Rect::new(20, 20, 180, 48), 14, Color::WHITE);
    ui.text(Point::new(32, 34), "Regular", system_color::SECONDARY_LABEL, 14);
    ui.text_semibold(Point::new(32, 58), "Semibold", system_color::LABEL, 14);
}
```

For uncommon font weights use `text_with_weight` and `measure_with_weight`.
For custom low-level widgets, `Frame::canvas()` exposes the underlying
`dyn Canvas` without forcing every app to carry generic `Canvas` and
`TextRenderer` parameters.

## System colors and contrast

Draw text and glyphs with `system_color`'s semantic colors rather than
hand-picked RGB, the way Apple's HIG asks apps to use `labelColor` and friends.
Every app then agrees with the system chrome, and every pair is already checked
against the same contrast minimums:

| Color | Use it for | Minimum it is held to |
| --- | --- | --- |
| `LABEL` | titles, labels, names, menu titles | 4.5:1 text |
| `SECONDARY_LABEL` | captions, column headings, sizes, section headings | 4.5:1 text |
| `TERTIARY_LABEL` | glyphs that should recede (disclosure chevrons) | 3:1 non-text; not for text |
| `DISABLED_LABEL` | disabled controls **only** | exempt, on purpose |
| `ACCENT` / `ACCENT_HOVER` / `ACCENT_PRESSED` | default buttons, selection | 3:1 on surfaces |
| `ON_ACCENT` | anything drawn on an accent fill | 4.5:1 in every accent state |

The surfaces they are checked against are `WINDOW_BACKGROUND`,
`CONTENT_BACKGROUND`, `TITLEBAR` and `MENUBAR`. The minimums are WCAG 2.x AA's,
which the HIG's accessibility guidance refers to: 4.5:1 for text
(`MIN_TEXT_CONTRAST`) and 3:1 for meaningful non-text (`MIN_GRAPHIC_CONTRAST`).

When an app adds a surface of its own (a sidebar, striped rows, a hover fill),
check its pairs in a test with `Color::contrast_ratio`, as Voyager's
`theme.rs` does. For text over a color only known at runtime, such as a
wallpaper, `Color::best_label()` picks `LABEL` or white, whichever reads better.

Two further rules:

- **Muted is not the same as secondary.** A grey that works should be
  `SECONDARY_LABEL`. Anything lighter reads as disabled.
- **Don't let color alone carry state.** A selected segment or row should
  change in shape or fill as well as in hue.

## `WindowConfig`

The shortest declaration creates a fixed-size standard window:

```rust
const WINDOW: WindowConfig = WindowConfig::new(760, 520);
```

Resizable windows can opt in explicitly:

```rust
const WINDOW: WindowConfig = WindowConfig::new(760, 520)
    .resizable(Size::new(520, 360), Some(Size::new(1440, 1000)));
```

UIService owns the outer chrome, window position, drag behavior and backing
surface. `draw()` receives only the content area.

## Naming policy

- Framework on disk / link boundary: `UIService.framework`.
- NXU C symbols: `UIService...`.
- App-facing Rust umbrella: `ui`.
- Internal Rust crates: `ui-*`.
- Bundle/application identifiers: reverse DNS, for example
  `com.butterscotch.shark`.

The short `ui` import is intentional: implementation naming should not make
application source noisy.
