#![no_std]

use ui::prelude::*;

pub struct HelloWorldApp;

impl App for HelloWorldApp {
    const INFO: AppInfo<'static> =
        AppInfo::new("Hello World", "com.butterscotch.hello-world", "0.1.0");
    const WINDOW: WindowConfig = WindowConfig::new(480, 280);

    fn draw(&mut self, ui: &mut Frame<'_>) {
        ui.fill(system_color::WINDOW_BACKGROUND);
        ui.text_semibold(
            Point::new(scale::pt_i32(24), scale::pt_i32(28)),
            "Hello World",
            system_color::LABEL,
            scale::pt(24),
        );
    }
}
