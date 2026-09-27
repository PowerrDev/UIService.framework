# Creating sevOS applications

Applications should depend on only the public `ui` umbrella crate unless they
are implementing UIService itself.

## Minimal app

```rust
#![no_std]

use ui::prelude::*;

pub struct DockApp;

impl App for DockApp {
    const INFO: AppInfo<'static> =
        AppInfo::new("Dock", "com.butterscotch.dock", "0.1.0");
    const WINDOW: WindowConfig = WindowConfig::new(640, 120);

    fn draw(&mut self, ui: &mut Frame<'_>) {
        ui.fill(system_color::WINDOW_BACKGROUND);
        ui.text_semibold(
            Point::new(scale::pt_i32(20), scale::pt_i32(20)),
            "Dock",
            system_color::LABEL,
            scale::pt(18),
        );
    }
}
```

That is the complete required interface.

UIService owns:

- window chrome and titlebar drawing;
- window position and titlebar dragging;
- the backing surface;
- font backend selection;
- pointer routing and coordinate conversion;
- damage tracking and presentation;
- NXU/WindowServer integration.

The app owns its state and content.

## Input and redraws

`event()` is optional. Override it when an app becomes interactive:

```rust
fn event(&mut self, event: Event) -> AppAction {
    if self.button.handle_event(event) {
        self.opened = true;
        return AppAction::Redraw;
    }

    AppAction::None
}
```

Events received by an app use **content-local coordinates**. The app never has
to subtract its window's screen position.

`Event::Scroll { position, delta }` arrives only while the pointer is over the
app's content (not over the titlebar or border, and not during a window drag or
resize). `delta` counts wheel notches, positive for wheel up. Voyager's
`scroll` module shows the intended use: a notch moves a target, and the view
eases towards it from `App::tick`.

Return values:

- `AppAction::None` — nothing visual changed;
- `AppAction::Redraw` — rebuild the app backing store and present it;
- `AppAction::Close` — ask the runtime to end the app session.

## Menus

While an app is frontmost the menu bar shows its name (Hide, Quit, ...), then
its own menus, then Window. Declare them as a constant and answer two calls:

```rust
const VIEW: &[MenuItem] = &[
    MenuItem::command("as List", 1),
    MenuItem::command("as Icons", 2),
    MenuItem::SEPARATOR,
    MenuItem::command("Refresh", 3),
];

impl App for Shark {
    const MENUS: &'static [Menu] = &[Menu::new("View", VIEW)];

    // Asked each time a menu opens: greyed out, check mark.
    fn menu_item_state(&self, command: u32) -> MenuItemState {
        match command {
            1 => MenuItemState::checked(self.list),
            2 => MenuItemState::checked(!self.list),
            _ => MenuItemState::ENABLED,
        }
    }

    fn menu_command(&mut self, command: u32) -> AppAction {
        // ... same as the matching toolbar button
        AppAction::Redraw
    }
}
```

## Window configuration

A fixed standard window is one line:

```rust
const WINDOW: WindowConfig = WindowConfig::new(760, 520);
```

Resizable apps can declare bounds without handling resizing themselves:

```rust
const WINDOW: WindowConfig = WindowConfig::new(760, 520)
    .resizable(Size::new(480, 320), Some(Size::new(1200, 900)));
```

The early NXU runtime does not resize windows yet; the fields exist so apps do
not need another API rewrite when resizing moves into the userspace server.

## Generator

```sh
make new-app \
  APP=shark \
  NAME="Shark" \
  ID=com.butterscotch.shark \
  WIDTH=760 \
  HEIGHT=520
```

The generator creates `apps/shark`, writes the minimal `App` implementation and
registers the package in the workspace.

## Where apps live

Framework code belongs in `crates/`. Product applications belong in `apps/`.

```text
apps/
├── about-sevos/
├── dock/                  com.butterscotch.dock
└── shark/                 com.butterscotch.shark
```

Do not add app-specific branches to widgets, renderers or NXU host code. If an
app needs a reusable capability, promote that capability into a framework crate
with an app-neutral API.

## Running on NXU

Apps are processes: `.app executable → ui-app-nxu → NXU UI session calls →
the desktop`. To ship one, add `bundles/<app>` (a staticlib calling
`ui_app_nxu::run("/Applications/<Name>.app", <App>::new)`), list it in
`NXU_BUNDLES` in the Makefile, and give it a bundle on NXU's side (NXU's
`doc/apps-and-dock.md`, "Adding an app"). `apps/dock` is the Dock's logic
(property lists, `.icns` icons, layout, drawing; host-tested), and
`examples/dock` renders it with real icons:

```sh
cargo run --offline --release -p dock-preview -- OUT.png 2000 assets/Backgrounds/DefaultWallpaper.jpg ICNS...
```
