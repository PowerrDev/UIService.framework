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

`ui-app::App` is the application contract. On NXU an app is a process:
`ui-app-nxu` runs it, turning the desktop's messages into `App` calls and
sending back its frames. The desktop around the apps
(`ui-service-nxu/src/desktop.rs`) runs in the kernel:

1. connect to `UIServiceHostV5` and paint the wallpaper once;
2. keep a `RemoteApp` (`remote.rs`) per app process that connected through
   the kernel's UI session bridge: its window's chrome, backing store and
   place, key or not, and the Dock's window (`dockhost.rs`);
3. keep the stacking order, raise and focus a window on a click, and route
   pointer events to the window under the pointer (or the one that took the
   press, until the release), keys to the key window, the wheel to the window
   under the pointer;
4. let `ui-window` handle dragging and resizing, and the traffic lights close
   (quit) and minimize (hide) the app;
5. give only the content canvas to the app and rebuild it only on
   `AppAction::Redraw`; repaint just the titlebar when a window turns key or
   not;
6. own the menu bar (`menubar.rs`): the logo's system menu, the key app's
   name (Hide, Hide Others, Show All, Quit), its `App::MENUS`, and Window.
   An open menu is its own WindowServer window above everything.

`Present()` answering `false` means "nothing was damaged" (an app redraw
that came out pixel-identical), never a reason to end the session.

## Future userspace model

Apps already run as processes and never touch display callbacks. The desktop
itself (`ui-service-nxu`) is still in the kernel: moving it and WindowServer
into a userspace process of their own is what is left.

The framework is intentionally arranged so `ui-app`, widgets, rendering and
text can survive that transition while the platform/runtime crate is replaced.

## Recovery boundary

Recovery.framework and triageOS are a separate product/runtime boundary.
triageOS does not import UIService and UIService does not import recovery UI.
A future recovery redesign should have its own recovery-only UI foundation or
service rather than making normal sevOS applications depend on recovery code.
