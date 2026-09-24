use std::fs::File;
use std::io::{BufWriter, Write};

use ui_core::{Color, Event, Point, PointerButton, Rect};
use ui_render::{Canvas, Surface};
use ui_widgets::Button;

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;

fn main() -> std::io::Result<()> {
    let mut pixels = vec![0u32; (WIDTH * HEIGHT) as usize];
    let mut surface = Surface::new(&mut pixels, WIDTH, HEIGHT, WIDTH).expect("valid demo surface");
    surface.fill(Color::rgb(22, 24, 29));

    let idle = Button::new("Idle", Rect::new(70, 80, 150, 48));
    idle.draw(&mut surface);

    let mut hovered = Button::new("Hovered", Rect::new(245, 80, 150, 48));
    hovered.handle_event(Event::PointerMoved {
        position: Point::new(260, 95),
    });
    hovered.draw(&mut surface);

    let mut pressed = Button::new("Pressed", Rect::new(420, 80, 150, 48));
    pressed.handle_event(Event::PointerDown {
        position: Point::new(440, 95),
        button: PointerButton::Primary,
    });
    pressed.draw(&mut surface);

    let mut disabled = Button::new("Disabled", Rect::new(245, 165, 150, 48));
    disabled.set_enabled(false);
    disabled.draw(&mut surface);

    write_ppm("ui-buttons.ppm", surface.pixels(), WIDTH, HEIGHT, WIDTH)?;
    println!("wrote ui-buttons.ppm");
    println!("button labels are stored but text rendering is intentionally deferred");
    Ok(())
}

fn write_ppm(path: &str, pixels: &[u32], width: u32, height: u32, stride: u32) -> std::io::Result<()> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    write!(writer, "P6\n{} {}\n255\n", width, height)?;

    for y in 0..height as usize {
        for x in 0..width as usize {
            let pixel = pixels[y * stride as usize + x];
            writer.write_all(&[
                ((pixel >> 16) & 0xFF) as u8,
                ((pixel >> 8) & 0xFF) as u8,
                (pixel & 0xFF) as u8,
            ])?;
        }
    }

    writer.flush()
}
