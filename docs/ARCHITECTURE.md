# Architecture

UIService separates application policy from kernel/platform integration.

```text
apps/about-sevos, apps/shark, apps/dock
                    │
                    ▼
                  ui
                    │
      ┌─────────────┼─────────────┐
      ▼             ▼             ▼
   ui-app       ui-widgets     ui-text
      │             │             │
      └──────┬──────┴──────┬──────┘
             ▼             ▼
         ui-window      ui-render
             │             │
             └──────┬──────┘
                    ▼
                 ui-core

NXU-only side:

ui-service-nxu → ui-platform → ui-abi → UIService.h → NXU
```

## Public versus internal API

Normal apps import `ui::prelude::*`. The prelude intentionally excludes the
host ABI and NXU platform wrappers.

Advanced/internal code can explicitly use:

```rust
ui::abi
ui::platform
ui::render
ui::window
```

This keeps device/ABI details out of ordinary application source without hiding
the lower layers from framework development.

## App runtime

`ui-app::App` is the application contract. The early NXU adapter implements a
generic app runtime around it:

1. connect to `UIServiceHostV2`;
2. create/center the app's standard window;
3. build a cached window backing surface;
4. draw standard chrome;
5. give only the content canvas to the app;
6. coalesce normalized input events;
7. let `ui-window` handle dragging;
8. translate remaining pointer input to content-local coordinates;
9. rebuild only when the app requests `AppAction::Redraw`;
10. present the calculated damage rectangle.

About sevOS is no longer hard-coded into that runtime. It is simply the first
statically registered app during early bring-up.

## Future userspace model

`ui-service-nxu` is transitional. Long-term, application processes should not
link directly against kernel display callbacks. A userspace UIService or
WindowServer should own window lists, z-order, focus, app surfaces and IPC while
preserving the same small app-facing concepts.

The framework is intentionally arranged so `ui-app`, widgets, rendering and
text can survive that transition while the platform/runtime crate is replaced.

## Recovery boundary

Recovery.framework and triageOS are a separate product/runtime boundary.
triageOS does not import UIService and UIService does not import recovery UI.
A future recovery redesign should have its own recovery-only UI foundation or
service rather than making normal sevOS applications depend on recovery code.
