//! Render every login/setup screen to PNGs, with the real wallpaper, Inter
//! and Borel, so the design can be checked without booting NXU.
//!
//!     cargo run --offline -p login-preview -- OUT_DIR [WIDTH HEIGHT SCALE_PERMILLE]

use std::path::PathBuf;

use ui_abi::{AuthResult, AuthStatus};
use ui_core::{key, scale, Event, Size};
use ui_login::{Backdrop, LoginScreen, PasscodeChecker};
use ui_render::Surface;
use ui_text::{FontFamily, TextScratch, TtfTextRenderer};

const INTER: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
const INTER_SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf");
const BOREL: &[u8] = include_bytes!("../../../assets/fonts/Borel-Regular.ttf");

struct Scripted {
    status: Option<AuthStatus>,
    verify: AuthResult,
}

impl PasscodeChecker for Scripted {
    fn status(&mut self) -> Option<AuthStatus> {
        self.status
    }
    fn verify(&mut self, _passcode: &[u8]) -> (AuthResult, u32) {
        (self.verify, 0)
    }
    fn set(&mut self, _passcode: &[u8]) -> (AuthResult, u32) {
        (AuthResult::Ok, 0)
    }
}

fn status(set: bool, failures: u32, locked: bool, wait: u32) -> Option<AuthStatus> {
    Some(AuthStatus { passcode_set: u32::from(set), failures, locked: u32::from(locked), wait_seconds: wait })
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let out = PathBuf::from(arguments.get(1).cloned().unwrap_or_else(|| "login-preview".into()));
    let width: u32 = arguments.get(2).and_then(|value| value.parse().ok()).unwrap_or(1366);
    let height: u32 = arguments.get(3).and_then(|value| value.parse().ok()).unwrap_or(693);
    let permille: u32 = arguments.get(4).and_then(|value| value.parse().ok()).unwrap_or(1000);
    scale::set_permille(permille);
    std::fs::create_dir_all(&out).unwrap();

    // The wallpaper, "cover"-fitted like scene::UIDrawDesktop.
    let wallpaper = image::open(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/Backgrounds/DefaultWallpaper.jpg"))
        .unwrap()
        .resize_to_fill(width, height, image::imageops::FilterType::Triangle)
        .to_rgb8();
    let mut wall: Vec<u32> = wallpaper.pixels().map(|p| (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2])).collect();
    let wall_surface = Surface::new(&mut wall, width, height, width).unwrap();
    let low = Backdrop::low_size(Size::new(width, height));
    let mut low_pixels = vec![0u32; (low.width * low.height) as usize];
    let mut temp = low_pixels.clone();
    let backdrop = Backdrop::build(&wall_surface, &mut low_pixels, &mut temp).unwrap();

    let mut scratch = Box::new(TextScratch::new());

    let mut shot = |name: &str, screen: &mut LoginScreen, now: u64| {
        let mut pixels = vec![0u32; (width * height) as usize];
        let mut surface = Surface::new(&mut pixels, width, height, width).unwrap();
        let region = ui_core::Rect::new(0, 0, width, height);
        if screen.wants_display_face() {
            let family = FontFamily::from_bytes(BOREL, None).unwrap().with_cache_tag(1);
            let mut text = TtfTextRenderer::new(family, &mut scratch);
            screen.paint(&mut surface, &backdrop, &mut text, region, now);
        } else {
            let family = FontFamily::from_bytes(INTER, Some(INTER_SEMIBOLD)).unwrap();
            let mut text = TtfTextRenderer::new(family, &mut scratch);
            screen.paint(&mut surface, &backdrop, &mut text, region, now);
        }
        let mut image = image::RgbImage::new(width, height);
        for (index, pixel) in pixels.iter().enumerate() {
            image.put_pixel(index as u32 % width, index as u32 / width, image::Rgb([(pixel >> 16) as u8, (pixel >> 8) as u8, *pixel as u8]));
        }
        let path = out.join(format!("{name}.png"));
        image.save(&path).unwrap();
        println!("{}", path.display());
    };

    let typed = |screen: &mut LoginScreen, text: &str| {
        for character in text.chars() {
            screen.event(Event::KeyDown { code: 0x1000, character: Some(character) }, 1);
        }
    };

    // First boot.
    let mut checker = Scripted { status: status(false, 0, false, 0), verify: AuthResult::Ok };
    let mut screen = LoginScreen::new();
    screen.start(&mut checker, 0);
    shot("1-greeting-fading-in", &mut screen, 450_000);
    shot("2-greeting", &mut screen, 2_000_000);
    screen.tick(&mut checker, 10_000_000);
    typed(&mut screen, "secret");
    shot("3-setup", &mut screen, 10_000_001);
    screen.event(Event::KeyDown { code: key::TAB, character: None }, 10_000_002);
    typed(&mut screen, "secrex");
    screen.event(Event::KeyDown { code: key::ENTER, character: None }, 10_000_003);
    shot("4-setup-mismatch", &mut screen, 10_000_004);

    // Later boots.
    let mut checker = Scripted { status: status(true, 0, false, 0), verify: AuthResult::Denied };
    let mut screen = LoginScreen::new();
    screen.start(&mut checker, 0);
    typed(&mut screen, "hunter2");
    shot("5-login", &mut screen, 1);
    screen.event(Event::KeyDown { code: key::ENTER, character: None }, 2);
    shot("6-login-checking", &mut screen, 3);
    screen.run_pending(&mut checker, 4);
    shot("7-login-wrong", &mut screen, 2_000_000);

    let mut checker = Scripted { status: status(true, 5, false, 59), verify: AuthResult::Denied };
    let mut screen = LoginScreen::new();
    screen.start(&mut checker, 0);
    shot("8-login-delay", &mut screen, 1);

    let mut checker = Scripted { status: status(true, 10, true, 0), verify: AuthResult::Locked };
    let mut screen = LoginScreen::new();
    screen.start(&mut checker, 0);
    shot("9-locked", &mut screen, 1);

    let mut checker = Scripted { status: None, verify: AuthResult::Unavailable };
    let mut screen = LoginScreen::new();
    screen.start(&mut checker, 0);
    shot("0-waiting", &mut screen, 1);
}
