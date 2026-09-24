use std::env;
use std::fs;
use std::path::{Path, PathBuf};

// The embedded background is capped well below typical source photo
// resolution: it is blitted with a "cover" fit at runtime (see
// `scene::UIDrawDesktop`), so extra source pixels beyond what a display can
// show are wasted kernel.bin size, not quality.
const BACKGROUND_MAX_DIMENSION: u32 = 1600;

// The menu bar logo is drawn at one constant on-screen size regardless of
// display resolution (unlike the desktop background), so it is fit and
// centered into this square at build time. `scene.rs` then blits it with no
// runtime scaling math at all.
const MENU_BAR_LOGO_SIZE: u32 = 40;

fn embedded_bytes(name: &str, env_name: &str, extension: &str, output_dir: &Path) -> String {
    println!("cargo:rerun-if-env-changed={env_name}");

    let Ok(source) = env::var(env_name) else {
        return format!("pub static {name}: &[u8] = &[];\n");
    };
    let source = PathBuf::from(source);
    if !source.is_file() {
        return format!("pub static {name}: &[u8] = &[];\n");
    }

    println!("cargo:rerun-if-changed={}", source.display());
    let destination = output_dir.join(format!("{name}.{extension}"));
    fs::copy(&source, &destination).expect("copy UIService asset into Cargo OUT_DIR");
    let path_literal = format!("{:?}", destination.to_string_lossy());
    format!("pub static {name}: &[u8] = include_bytes!({path_literal});\n")
}

/// Decode and downscale the desktop background image at build time so the
/// freestanding kernel-side target never needs a PNG/zlib decoder: build
/// scripts run on the host, where `image` and full `std` are available even
/// though the final `ui-service-nxu` static library is `no_std`.
///
/// The output is raw XRGB8888 pixels, 4 little-endian bytes per pixel, row-
/// major with no padding, so `scene.rs` can read it with plain
/// `u32::from_le_bytes` and no unsafe transmute.
fn embedded_background(env_name: &str, output_dir: &Path) -> String {
    println!("cargo:rerun-if-env-changed={env_name}");

    let empty = "pub static BACKGROUND_DATA: &[u8] = &[];\n\
                 pub const BACKGROUND_WIDTH: u32 = 0;\n\
                 pub const BACKGROUND_HEIGHT: u32 = 0;\n";

    let Ok(source) = env::var(env_name) else {
        return empty.to_string();
    };
    let source = PathBuf::from(source);
    if !source.is_file() {
        return empty.to_string();
    }

    println!("cargo:rerun-if-changed={}", source.display());
    let image = image::open(&source)
        .unwrap_or_else(|error| panic!("decode UIService background {source:?}: {error}"))
        .to_rgba8();

    let (source_width, source_height) = (image.width(), image.height());
    let scale = (BACKGROUND_MAX_DIMENSION as f64 / source_width.max(source_height) as f64).min(1.0);
    let width = ((source_width as f64 * scale).round() as u32).max(1);
    let height = ((source_height as f64 * scale).round() as u32).max(1);

    let resized = if width == source_width && height == source_height {
        image
    } else {
        image::imageops::resize(&image, width, height, image::imageops::FilterType::Lanczos3)
    };

    let mut bytes = Vec::with_capacity(width as usize * height as usize * 4);
    for pixel in resized.pixels() {
        let [red, green, blue, _alpha] = pixel.0;
        bytes.extend_from_slice(&[blue, green, red, 0]);
    }

    let destination = output_dir.join("background.rgba");
    fs::write(&destination, &bytes).expect("write UIService background into Cargo OUT_DIR");
    let path_literal = format!("{:?}", destination.to_string_lossy());
    format!(
        "pub static BACKGROUND_DATA: &[u8] = include_bytes!({path_literal});\n\
         pub const BACKGROUND_WIDTH: u32 = {width};\n\
         pub const BACKGROUND_HEIGHT: u32 = {height};\n"
    )
}

/// Decode, fit and center the menu bar logo into a fixed `MENU_BAR_LOGO_SIZE`
/// square at build time, alpha channel intact (it is alpha-blended over the
/// menu bar chrome at runtime, unlike the opaque desktop background).
///
/// The output is raw little-endian BGRA bytes, 4 per pixel, so `scene.rs`
/// can read each pixel with `u32::from_le_bytes` into an alpha-in-high-byte
/// value (`0xAARRGGBB`) with no unsafe transmute.
fn embedded_logo(env_name: &str, output_dir: &Path) -> String {
    println!("cargo:rerun-if-env-changed={env_name}");

    let empty = "pub static LOGO_DATA: &[u8] = &[];\n\
                 pub const LOGO_SIZE: u32 = 0;\n";

    let Ok(source) = env::var(env_name) else {
        return empty.to_string();
    };
    let source = PathBuf::from(source);
    if !source.is_file() {
        return empty.to_string();
    }

    println!("cargo:rerun-if-changed={}", source.display());
    let image = image::open(&source)
        .unwrap_or_else(|error| panic!("decode UIService menu bar logo {source:?}: {error}"))
        .to_rgba8();

    let (source_width, source_height) = (image.width(), image.height());
    let scale = (MENU_BAR_LOGO_SIZE as f64 / source_width.max(source_height) as f64).min(1.0);
    let fit_width = ((source_width as f64 * scale).round() as u32).max(1);
    let fit_height = ((source_height as f64 * scale).round() as u32).max(1);

    let resized = image::imageops::resize(&image, fit_width, fit_height, image::imageops::FilterType::Lanczos3);

    let mut canvas = image::RgbaImage::new(MENU_BAR_LOGO_SIZE, MENU_BAR_LOGO_SIZE);
    let x_offset = ((MENU_BAR_LOGO_SIZE - fit_width) / 2) as i64;
    let y_offset = ((MENU_BAR_LOGO_SIZE - fit_height) / 2) as i64;
    image::imageops::overlay(&mut canvas, &resized, x_offset, y_offset);

    let mut bytes = Vec::with_capacity((MENU_BAR_LOGO_SIZE * MENU_BAR_LOGO_SIZE * 4) as usize);
    for pixel in canvas.pixels() {
        let [red, green, blue, alpha] = pixel.0;
        bytes.extend_from_slice(&[blue, green, red, alpha]);
    }

    let destination = output_dir.join("menubar_logo.bgra");
    fs::write(&destination, &bytes).expect("write UIService menu bar logo into Cargo OUT_DIR");
    let path_literal = format!("{:?}", destination.to_string_lossy());
    format!(
        "pub static LOGO_DATA: &[u8] = include_bytes!({path_literal});\n\
         pub const LOGO_SIZE: u32 = {MENU_BAR_LOGO_SIZE};\n"
    )
}

fn main() {
    let output_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let mut generated = String::from("/* Generated by ui-service-nxu/build.rs. */\n");
    generated.push_str(&embedded_bytes(
        "INTER_REGULAR",
        "UISERVICE_INTER_REGULAR",
        "ttf",
        &output_dir,
    ));
    generated.push_str(&embedded_bytes(
        "INTER_SEMIBOLD",
        "UISERVICE_INTER_SEMIBOLD",
        "ttf",
        &output_dir,
    ));
    fs::write(output_dir.join("ui_fonts.rs"), generated).expect("write UIService font module");

    let mut cursors = String::from("/* Generated by ui-service-nxu/build.rs. */\n");
    for (name, env_name) in [
        ("CURSOR_ARROW", "UISERVICE_CURSOR_ARROW"),
        ("CURSOR_HAND", "UISERVICE_CURSOR_HAND"),
        ("CURSOR_MOVE", "UISERVICE_CURSOR_MOVE"),
        ("CURSOR_RESIZE_EW", "UISERVICE_CURSOR_RESIZE_EW"),
        ("CURSOR_RESIZE_NS", "UISERVICE_CURSOR_RESIZE_NS"),
        ("CURSOR_RESIZE_NESW", "UISERVICE_CURSOR_RESIZE_NESW"),
        ("CURSOR_RESIZE_NWSE", "UISERVICE_CURSOR_RESIZE_NWSE"),
        ("CURSOR_TEXT", "UISERVICE_CURSOR_TEXT"),
    ] {
        cursors.push_str(&embedded_bytes(name, env_name, "cur", &output_dir));
    }
    fs::write(output_dir.join("ui_cursors.rs"), cursors).expect("write UIService cursor module");

    let background = embedded_background("UISERVICE_BACKGROUND", &output_dir);
    fs::write(output_dir.join("ui_background.rs"), background)
        .expect("write UIService background module");

    let logo = embedded_logo("UISERVICE_MENUBAR_LOGO", &output_dir);
    fs::write(output_dir.join("ui_menubar_logo.rs"), logo)
        .expect("write UIService menu bar logo module");
}
