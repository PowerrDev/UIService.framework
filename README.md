# UIService.framework

UIService is sevOS's graphical application framework and the system-facing UI
runtime for NXU.

The framework has two intentionally different surfaces:

- **Apps use the tiny Rust `ui` crate.** They should not know about NXU,
  VirtIO, framebuffer addresses, host callbacks or the C ABI.
- **NXU links UIService.framework.** The kernel provides display/input through
  `UIService.h`; the `ui-service-nxu` adapter turns that host ABI into the same
  application model used by apps.

```text
sevOS app
  use ui::prelude::*
        │
        ▼
      ui::App
        │
        ▼
 widgets · text · windows · render · assets
        │
        ▼
 UIService runtime / WindowServer (future)
        │
        ▼
      NXU host ABI
        │
        ▼
 display · input · memory · VirtIO
```

## Creating an app

The app API is deliberately small:

```rust
#![no_std]

use ui::prelude::*;

pub struct SharkApp;

impl App for SharkApp {
    const INFO: AppInfo<'static> =
        AppInfo::new("Shark", "com.butterscotch.shark", "0.1.0");
    const WINDOW: WindowConfig = WindowConfig::new(760, 520);

    fn draw(&mut self, ui: &mut Frame<'_>) {
        ui.fill(system_color::WINDOW_BACKGROUND);
        ui.text_semibold(
            Point::new(scale::pt_i32(24), scale::pt_i32(28)),
            "Shark",
            system_color::LABEL,
            scale::pt(24),
        );
    }
}
```

Or generate the skeleton:

```sh
make new-app \
  APP=shark \
  NAME="Shark" \
  ID=com.butterscotch.shark \
  WIDTH=760 \
  HEIGHT=520
```

See [`docs/APPS.md`](docs/APPS.md) for app lifecycle/input details.

## Build

```sh
make check
make nxu
```

The NXU-facing outputs are:

```text
build/libUIService.a
build/UIService.h
```

`make docs` builds Rust API documentation locally.

## Repository map

```text
apps/                       sevOS applications
  about-sevos/              reference system app
  hello-world/              minimal app example

crates/
  ui/                       app-facing umbrella crate
  ui-app/                   App + WindowConfig lifecycle contract
  ui-core/                  geometry, color, normalized input
  ui-render/                Canvas, Surface, blending
  ui-widgets/               reusable controls/views
  ui-window/                window chrome and dragging policy
  ui-text/                  no_std TrueType renderer
  ui-login/                 first-boot setup and login screen
  ui-assets/                cursor/resource decoding
  ui-support/               app/system metadata
  ui-abi/                   stable repr(C) NXU boundary
  ui-platform/              Rust wrappers around the host ABI
  ui-service-nxu/           NXU-only staticlib/runtime adapter

include/UIService.h         public NXU C ABI
docs/                       architecture and API documentation
tools/new_app.py            app skeleton generator
```

## Recovery / triageOS

UIService is the **normal sevOS UI stack**. Recovery remains separate. Current
triageOS code must not link UIService, and UIService must not depend on
Recovery.framework. A future recovery rewrite can get its own small UI service
or foundation while sharing design principles, not runtime dependencies.
