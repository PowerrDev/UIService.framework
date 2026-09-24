# sevOS applications

This directory contains product applications. Framework implementation belongs
under `crates/`; app-specific UI and state belong here.

Each app currently builds as a small Rust crate implementing `ui::App`:

```rust
use ui::prelude::*;

pub struct MyApp;

impl App for MyApp {
    const INFO: AppInfo<'static> =
        AppInfo::new("My App", "com.butterscotch.my-app", "0.1.0");
    const WINDOW: WindowConfig = WindowConfig::new(640, 420);

    fn draw(&mut self, ui: &mut Frame<'_>) {
        ui.fill(Color::rgb(250, 250, 251));
    }
}
```

Create one with `make new-app APP=... NAME=... ID=...` from the
`UIService.framework` root.

The crates are statically usable during bring-up. They are deliberately kept
free of NXU host details so the same app contract can move to real `.app`
processes when sevOS has executable loading and a userspace WindowServer.
