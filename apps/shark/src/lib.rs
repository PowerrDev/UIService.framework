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
