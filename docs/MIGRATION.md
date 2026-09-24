# UI.framework → UIService.framework migration

This refactor separates the friendly Rust app API from the system/framework
name. Application source still imports `ui`; the NXU/link/ABI boundary is now
named UIService.

## Paths and artifacts

| Before | After |
| --- | --- |
| `UI.framework/` | `UIService.framework/` |
| `include/UI.h` | `include/UIService.h` |
| `build/libUI.a` | `build/libUIService.a` |
| `build/UI.h` | `build/UIService.h` |
| `crates/ui-nxu` | `crates/ui-service-nxu` |
| `crates/renos-about` | `apps/about-sevos` |
| `crates/hello-world` | `apps/hello-world` |

The app-facing crate remains `ui` on purpose:

```rust
use ui::prelude::*;
```

## C/NXU names

The public C ABI now uses a consistent `UIService` prefix:

- `UIHostV1` / `UIHostV2` → `UIServiceHostV1` / `UIServiceHostV2`;
- `UIRunAbout` → `UIServiceRunAbout`;
- `UIValidateHostV2` → `UIServiceValidateHostV2`;
- `UI_ABI_*` → `UI_SERVICE_ABI_*`;
- NXU build switch `UI=1` → `UISERVICE=1`;
- NXU source `ui_host.c` → `ui_service_host.c`.

`make ui-about` remains as a compatibility alias for the renamed
`uiservice-about` NXU target.

## Rust app API

Old app/view code commonly carried renderer generics through every function:

```rust
fn draw<C: Canvas, T: TextRenderer>(canvas: &mut C, text: &mut T)
```

Normal application code now implements `App` and receives one `Frame`:

```rust
impl App for MyApp {
    const INFO: AppInfo<'static> =
        AppInfo::new("My App", "com.butterscotch.my-app", "0.1.0");
    const WINDOW: WindowConfig = WindowConfig::new(640, 420);

    fn draw(&mut self, ui: &mut Frame<'_>) {
        ui.fill(Color::WHITE);
        ui.text_semibold(Point::new(24, 24), "My App", Color::BLACK, 20);
    }
}
```

Use `ui::render::*` only for framework work or genuinely custom low-level
rendering.

## App-specific code

`AboutPanel` and `AboutStyle` were removed from `ui-widgets`. About-specific
layout now lives in `apps/about-sevos`; reusable widgets stay app-neutral.

## Recovery / triageOS

No migration makes triageOS depend on UIService. Recovery remains a separate
boundary. A later triageOS rewrite should build on its own recovery-only UI
foundation/service and may copy proven concepts from UIService without linking
normal sevOS application code into recovery.
