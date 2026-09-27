//! The Dock as it will look: real icons (decoded by the Dock's own code),
//! the first one running, a divider and the Trash, on the panel material the
//! desktop draws (the wallpaper blurred under a white veil).

use dock::image::{decode_png, decoded_size, icns_best_png, scale_to_argb};
use dock::layout::{Layout, Metrics, Slot};
use dock::render::{draw, draw_trash, over, rounded_rect_contains, TileView};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!("usage: dock-preview OUT.png SCALE_PERMILLE WALLPAPER ICNS...");
        std::process::exit(2);
    }
    let scale: u32 = args[2].parse().unwrap();
    let pt = |points: u32| points * scale / 1000;
    let tile = pt(48);

    let mut icons: Vec<Vec<u32>> = Vec::new();
    for path in &args[4..] {
        let icns = std::fs::read(path).unwrap();
        let png = icns_best_png(&icns, tile).unwrap();
        let (raw_size, rgba_size) = decoded_size(png).unwrap();
        let mut raw = vec![0; raw_size];
        let mut rgba = vec![0; rgba_size];
        let header = decode_png(png, &mut raw, &mut rgba).unwrap();
        let mut icon = vec![0u32; (tile * tile) as usize];
        scale_to_argb(&rgba, header.width, header.height, tile, &mut icon);
        println!("{path}: {}x{} png -> {tile} px", header.width, header.height);
        icons.push(icon);
    }

    let mut slots: Vec<Slot> = (0..icons.len()).map(Slot::App).collect();
    slots.push(Slot::Divider);
    slots.push(Slot::Trash);
    let layout = Layout::new(Metrics::new(tile), &slots);
    let views: Vec<TileView> = icons
        .iter()
        .enumerate()
        .map(|(index, icon)| TileView { icon: Some(icon), running: index == 0, pressed: false })
        .collect();
    let mut trash = vec![0u32; (tile * tile) as usize];
    draw_trash(tile, &mut trash);
    let mut dock = vec![0u32; (layout.width * layout.height) as usize];
    draw(&layout, &views, &trash, &mut dock);

    // A strip of the wallpaper, 1366 pt wide, with the Dock at its bottom.
    let screen_w = pt(1366);
    let strip_h = layout.height + pt(80);
    let wallpaper = image::open(&args[3]).unwrap().to_rgb8();
    let wallpaper = image::imageops::resize(&wallpaper, screen_w, screen_w * wallpaper.height() / wallpaper.width(), image::imageops::FilterType::Triangle);
    let offset = wallpaper.height().saturating_sub(strip_h);
    let mut strip = image::RgbImage::new(screen_w, strip_h);
    for y in 0..strip_h {
        for x in 0..screen_w {
            strip.put_pixel(x, y, *wallpaper.get_pixel(x, (y + offset).min(wallpaper.height() - 1)));
        }
    }

    let dock_x = (screen_w - layout.width) / 2;
    let dock_y = strip_h - layout.height - pt(4);
    let backdrop = image::imageops::blur(&image::imageops::crop_imm(&strip, dock_x, dock_y, layout.width, layout.height).to_image(), pt(10) as f32);
    let radius = layout.metrics.radius as i32 * 4;
    for y in 0..layout.height {
        for x in 0..layout.width {
            let (w, h) = (layout.width as i32 * 4, layout.height as i32 * 4);
            let mut hits = 0;
            for sy in 0..4 {
                for sx in 0..4 {
                    if rounded_rect_contains(x as i32 * 4 + sx, y as i32 * 4 + sy, 0, 0, w, h, radius) {
                        hits += 1;
                    }
                }
            }
            if hits == 0 {
                continue;
            }
            let b = backdrop.get_pixel(x, y).0;
            let mut pixel = 0xFF00_0000 | ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
            pixel = over(pixel, 0x5C_FF_FF_FF);
            let edge = !rounded_rect_contains(x as i32 * 4 + 2, y as i32 * 4 + 2, 4, 4, w - 4, h - 4, radius - 4);
            if edge {
                pixel = over(pixel, if y < layout.height / 2 { 0x78_FF_FF_FF } else { 0x26_00_00_00 });
            }
            pixel = over(pixel, dock[(y * layout.width + x) as usize]);
            let under = strip.get_pixel(dock_x + x, dock_y + y).0;
            let mix = |shift: u32, u: u8| (((pixel >> shift) & 0xFF) * hits + u as u32 * (16 - hits)) / 16;
            strip.put_pixel(dock_x + x, dock_y + y, image::Rgb([mix(16, under[0]) as u8, mix(8, under[1]) as u8, mix(0, under[2]) as u8]));
        }
    }
    strip.save(&args[1]).unwrap();
    println!("{}: dock {}x{} at {} permille", args[1], layout.width, layout.height, scale);
}
