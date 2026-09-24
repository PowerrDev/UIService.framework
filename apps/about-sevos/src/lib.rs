#![no_std]

//! About sevOS application.
//!
//! This crate intentionally depends only on the public `ui` umbrella. It has
//! no NXU, framebuffer, VirtIO or UIService host-ABI knowledge.

use ui::prelude::*;
use ui::render::{Canvas, TextRenderer};

mod panel;

pub const SYSTEM_INFO: SystemInfo<'static> = SystemInfo::new(
    "sevOS",
    "Version 1.0",
    "I don't know",
    "Running Aqua · ARM64",
    "Copyright 2026 Power",
);

pub struct AboutApp;

impl App for AboutApp {
    const INFO: AppInfo<'static> =
        AppInfo::new("About my PC", "com.butterscotch.about", "0.1.0");
    const WINDOW: WindowConfig = WindowConfig::new(1040, 780);

    fn draw(&mut self, ui: &mut Frame<'_>) {
        panel::draw(ui, false);
    }
}

/// Draw a standalone About panel, useful for host-side previews.
pub fn draw_preview<C: Canvas, T: TextRenderer>(canvas: &mut C, text: &mut T) {
    let mut frame = Frame::new(canvas, text);
    panel::draw(&mut frame, true);
}
