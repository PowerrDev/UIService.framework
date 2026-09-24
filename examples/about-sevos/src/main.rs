use std::fs::File;
use std::io::{BufWriter, Write};

use ui::prelude::*;
use ui::render::{Canvas, Surface, TextRenderer};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 420;

struct PreviewText;

impl TextRenderer for PreviewText {
    fn measure(&self, text: &str, point_size: u32, _semibold: bool) -> Size {
        Size::new(
            (text.chars().count() as u32 * point_size * 3) / 5,
            point_size + 4,
        )
    }

    fn draw<C: Canvas + ?Sized>(
        &mut self,
        _canvas: &mut C,
        _origin: Point,
        _text: &str,
        _color: Color,
        _point_size: u32,
        _semibold: bool,
    ) {
        // UI deliberately has no emergency bitmap font. NXU/sevOS should
        // connect this trait to the real font service once that layer exists.
    }
}

fn main() -> std::io::Result<()> {
    let info = about_sevos::SYSTEM_INFO;
    let mut pixels = vec![0u32; (WIDTH * HEIGHT) as usize];
    let mut surface = Surface::new(&mut pixels, WIDTH, HEIGHT, WIDTH).expect("valid preview surface");
    about_sevos::draw_preview(&mut surface, &mut PreviewText);

    write_ppm("about-sevos.ppm", surface.pixels(), WIDTH, HEIGHT, WIDTH)?;
    println!("wrote about-sevos.ppm");
    println!("{}", info.product_name);
    println!("{}", info.version);
    println!("{}", info.build);
    println!("{}", info.kernel);
    println!("{}", info.copyright);
    println!("text drawing is intentionally delegated to UIService text backend");
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
