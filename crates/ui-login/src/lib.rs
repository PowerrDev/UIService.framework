#![no_std]

//! sevOS's first-boot setup and login screen.
//!
//! One fullscreen surface over the blurred wallpaper. On a first boot (no
//! passcode yet) it greets in Borel, the way armOS's Setup.app does, then asks
//! for a new passcode; on every later boot it asks for the passcode. Nothing
//! here decides whether a passcode is right: that is the
//! [`PasscodeChecker`]'s job (on NXU, tepOS's AuthenticationService over the
//! Trusted Enclave mailbox, with its delays and lockout). The screen only
//! finishes on an explicit "OK" from the checker; an unreachable checker
//! keeps it waiting.
//!
//! This crate is the screen's logic and drawing, host-testable; the NXU glue
//! (`ui-service-nxu/src/login.rs`) connects it to the host, WindowServer and
//! the checker.

#[cfg(test)]
extern crate std;

use ui_abi::{AuthResult, AuthStatus, UI_SERVICE_PASSCODE_MAX, UI_SERVICE_PASSCODE_MIN};
use ui_core::{key, scale, Color, Event, Point, Rect, Size};
use ui_render::{Canvas, Surface, TextRenderer};

pub const GREETING: &str = "welcome to sevOS";

/// Greeting timeline, in microseconds since it started.
const FADE_IN_US: u64 = 900_000;
const HOLD_US: u64 = 3_000_000;
const FADE_OUT_US: u64 = 700_000;
const GREETING_US: u64 = FADE_IN_US + HOLD_US + FADE_OUT_US;

/// How often the checker is asked again while waiting for it, or while the
/// passcode is locked (a recovery reset happens on the enclave's side).
const WAITING_POLL_US: u64 = 1_000_000;
const LOCKED_POLL_US: u64 = 5_000_000;

const SHAKE_US: u64 = 450_000;

const PASSCODE_CAPACITY: usize = UI_SERVICE_PASSCODE_MAX as usize;

const INK: Color = Color::rgb(0x1F, 0x23, 0x2A);
const MUTED: Color = Color::rgb(0x66, 0x6D, 0x78);
const DANGER: Color = Color::rgb(0xC6, 0x37, 0x37);
const ACCENT: Color = Color::rgb(0x18, 0x7E, 0xEE);
const ACCENT_DIM: Color = Color::rgb(0x9C, 0xC4, 0xF5);
const CARD: Color = Color::rgb(0xFA, 0xFA, 0xFA);
/// macOS-style text fields: white, a thin grey border, and when focused a
/// blue ring (drawn opaque: the ring colour as it looks over the card).
const FIELD: Color = Color::rgb(0xFF, 0xFF, 0xFF);
const FIELD_BORDER: Color = Color::rgb(0xC2, 0xC2, 0xC6);
const FOCUS_RING: Color = Color::rgb(0x86, 0xB6, 0xF7);
const PLACEHOLDER: Color = Color::rgb(0xB4, 0xB6, 0xBC);

// ---- the passcode ----------------------------------------------------------

/// A passcode being typed. Wiped with volatile stores (so they are not
/// dropped as dead) whenever it is cleared and when it goes away.
pub struct Secret {
    bytes: [u8; PASSCODE_CAPACITY],
    len: usize,
}

impl Secret {
    pub const fn new() -> Self {
        Self { bytes: [0; PASSCODE_CAPACITY], len: 0 }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    fn push(&mut self, byte: u8) -> bool {
        if self.len >= PASSCODE_CAPACITY {
            return false;
        }
        self.bytes[self.len] = byte;
        self.len += 1;
        true
    }

    fn pop(&mut self) -> bool {
        if self.len == 0 {
            return false;
        }
        self.len -= 1;
        unsafe { core::ptr::write_volatile(&mut self.bytes[self.len], 0) };
        true
    }

    pub fn wipe(&mut self) {
        for byte in self.bytes.iter_mut() {
            unsafe { core::ptr::write_volatile(byte, 0) };
        }
        self.len = 0;
    }

    fn matches(&self, other: &Secret) -> bool {
        // Not secret-dependent timing that matters: both are the user's own
        // typing, compared before either leaves the screen.
        self.as_bytes() == other.as_bytes()
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.wipe();
    }
}

// ---- the checker -----------------------------------------------------------

/// Whoever checks the passcode. `None` from `status` means it cannot be
/// reached. `set` is only ever used on a first boot (no old passcode).
pub trait PasscodeChecker {
    fn status(&mut self) -> Option<AuthStatus>;
    /// The result and, for `RetryLater`, the seconds to wait.
    fn verify(&mut self, passcode: &[u8]) -> (AuthResult, u32);
    fn set(&mut self, passcode: &[u8]) -> (AuthResult, u32);
}

// ---- the blurred backdrop --------------------------------------------------

/// The wallpaper, blurred and darkened, at a quarter of the screen's
/// resolution. Blurring there is 16x cheaper and bilinear sampling back up
/// is smooth by construction; the whole thing is built once.
pub struct Backdrop<'a> {
    pixels: &'a [u32],
    width: u32,
    height: u32,
}

pub const BACKDROP_DIVISOR: u32 = 4;
const BLUR_RADIUS: usize = 3;
const BLUR_PASSES: usize = 3;
/// What is left of the wallpaper's brightness under the scrim (of 256).
const SCRIM_KEEP: u32 = 176;

impl<'a> Backdrop<'a> {
    /// The quarter-size buffer `build` needs for a screen of `full` pixels.
    pub const fn low_size(full: Size) -> Size {
        Size::new(
            full.width.div_ceil(BACKDROP_DIVISOR),
            full.height.div_ceil(BACKDROP_DIVISOR),
        )
    }

    /// Downsample `source` (the wallpaper at full size) by averaging 4x4
    /// blocks, blur it with a few box passes (close to a Gaussian), darken
    /// it. `low` and `temp` need `low_size(full)` pixels each.
    pub fn build(source: &Surface<'_>, low: &'a mut [u32], temp: &mut [u32]) -> Option<Self> {
        let full = Size::new(source.width(), source.height());
        let size = Self::low_size(full);
        let count = size.width as usize * size.height as usize;
        if count == 0 || low.len() < count || temp.len() < count {
            return None;
        }

        let (width, height) = (size.width as usize, size.height as usize);
        let stride = source.stride() as usize;
        let pixels = source.pixels();
        let divisor = BACKDROP_DIVISOR as usize;

        for y in 0..height {
            for x in 0..width {
                let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
                for sy in y * divisor..((y + 1) * divisor).min(full.height as usize) {
                    for sx in x * divisor..((x + 1) * divisor).min(full.width as usize) {
                        let pixel = pixels[sy * stride + sx];
                        r += (pixel >> 16) & 0xFF;
                        g += (pixel >> 8) & 0xFF;
                        b += pixel & 0xFF;
                        n += 1;
                    }
                }
                let n = n.max(1);
                low[y * width + x] = pack(r / n, g / n, b / n);
            }
        }

        for _ in 0..BLUR_PASSES {
            box_blur(&low[..count], &mut temp[..count], width, height, true);
            box_blur(&temp[..count], &mut low[..count], width, height, false);
        }

        for pixel in low[..count].iter_mut() {
            let value = *pixel;
            *pixel = pack(
                ((value >> 16) & 0xFF) * SCRIM_KEEP / 256,
                ((value >> 8) & 0xFF) * SCRIM_KEEP / 256,
                (value & 0xFF) * SCRIM_KEEP / 256,
            );
        }

        Some(Self { pixels: &low[..count], width: size.width, height: size.height })
    }

    /// A flat dark backdrop, for a screen too large for the blur buffers.
    pub const fn plain() -> Self {
        Self { pixels: &[], width: 0, height: 0 }
    }

    /// Fill `region` of `surface` from the backdrop, bilinearly.
    pub fn paint(&self, surface: &mut Surface<'_>, region: Rect) {
        if self.pixels.is_empty() {
            surface.fill_rect(region, Color::rgb(0x1C, 0x22, 0x2C));
            return;
        }
        let left = region.origin.x.max(0) as u32;
        let top = region.origin.y.max(0) as u32;
        let right = (region.origin.x.saturating_add(region.size.width as i32).max(0) as u32).min(surface.width());
        let bottom = (region.origin.y.saturating_add(region.size.height as i32).max(0) as u32).min(surface.height());
        let stride = surface.stride() as usize;
        let max_u = (self.width as i32 - 1) * 256;
        let max_v = (self.height as i32 - 1) * 256;
        let out = surface.pixels_mut();

        for y in top..bottom {
            // Texel centres: pixel y's centre is at (y + 0.5) / 4 texels.
            let v = ((y as i32 * 2 + 1) * 128 / BACKDROP_DIVISOR as i32 - 128).clamp(0, max_v.max(0));
            let (row0, fy) = ((v >> 8) as usize, (v & 0xFF) as u32);
            let row1 = (row0 + 1).min(self.height as usize - 1);
            for x in left..right {
                let u = ((x as i32 * 2 + 1) * 128 / BACKDROP_DIVISOR as i32 - 128).clamp(0, max_u.max(0));
                let (column0, fx) = ((u >> 8) as usize, (u & 0xFF) as u32);
                let column1 = (column0 + 1).min(self.width as usize - 1);
                let w = self.width as usize;
                let top_mix = mix(self.pixels[row0 * w + column0], self.pixels[row0 * w + column1], fx);
                let bottom_mix = mix(self.pixels[row1 * w + column0], self.pixels[row1 * w + column1], fx);
                out[y as usize * stride + x as usize] = mix(top_mix, bottom_mix, fy);
            }
        }
    }
}

fn pack(r: u32, g: u32, b: u32) -> u32 {
    (r.min(255) << 16) | (g.min(255) << 8) | b.min(255)
}

/// `a` towards `b` by `t`/256, per channel.
fn mix(a: u32, b: u32, t: u32) -> u32 {
    let channel = |shift: u32| {
        let (x, y) = ((a >> shift) & 0xFF, (b >> shift) & 0xFF);
        (x * (256 - t) + y * t) >> 8
    };
    pack(channel(16), channel(8), channel(0))
}

/// One running-sum box pass of radius BLUR_RADIUS along rows or columns,
/// clamping at the edges.
fn box_blur(source: &[u32], out: &mut [u32], width: usize, height: usize, horizontal: bool) {
    let (lines, length) = if horizontal { (height, width) } else { (width, height) };
    let at = |line: usize, index: usize| if horizontal { line * width + index } else { index * width + line };
    let span = (2 * BLUR_RADIUS + 1) as u32;

    for line in 0..lines {
        let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
        let clamped = |index: isize| index.clamp(0, length as isize - 1) as usize;
        for offset in -(BLUR_RADIUS as isize)..=(BLUR_RADIUS as isize) {
            let pixel = source[at(line, clamped(offset))];
            r += (pixel >> 16) & 0xFF;
            g += (pixel >> 8) & 0xFF;
            b += pixel & 0xFF;
        }
        for index in 0..length {
            out[at(line, index)] = pack(r / span, g / span, b / span);
            let leaving = source[at(line, clamped(index as isize - BLUR_RADIUS as isize))];
            let entering = source[at(line, clamped(index as isize + BLUR_RADIUS as isize + 1))];
            r = r + ((entering >> 16) & 0xFF) - ((leaving >> 16) & 0xFF);
            g = g + ((entering >> 8) & 0xFF) - ((leaving >> 8) & 0xFF);
            b = b + (entering & 0xFF) - (leaving & 0xFF);
        }
    }
}

// ---- the screen ------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    /// The checker cannot be reached: nothing to do but wait for it.
    Waiting,
    /// First boot: the Borel greeting, then `Setup`.
    Greeting,
    /// First boot: choose a passcode.
    Setup,
    /// Every later boot: enter the passcode.
    Login,
    /// Too many failures: only a recovery reset on the enclave helps.
    Locked,
    /// Unlocked (or set up). The caller moves on to the desktop.
    Done,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Note {
    None,
    Error(&'static str),
    /// A countdown to `wait_until_us`.
    Wait,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Pending {
    None,
    Verify,
    Set,
}

pub struct LoginScreen {
    phase: Phase,
    phase_started_us: u64,
    greeted: bool,
    field: usize,
    passcode: Secret,
    confirm: Secret,
    note: Note,
    wait_until_us: u64,
    next_poll_us: u64,
    pending: Pending,
    shake_started_us: Option<u64>,
    full_repaint: bool,
}

impl LoginScreen {
    pub const fn new() -> Self {
        Self {
            phase: Phase::Waiting,
            phase_started_us: 0,
            greeted: false,
            field: 0,
            passcode: Secret::new(),
            confirm: Secret::new(),
            note: Note::None,
            wait_until_us: 0,
            next_poll_us: 0,
            pending: Pending::None,
            shake_started_us: None,
            full_repaint: true,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn finished(&self) -> bool {
        self.phase == Phase::Done
    }

    /// Ask the checker where things stand and pick the first screen.
    pub fn start(&mut self, checker: &mut impl PasscodeChecker, now_us: u64) {
        self.refresh(checker, now_us);
    }

    fn enter(&mut self, phase: Phase, now_us: u64) {
        if self.phase != phase {
            self.phase = phase;
            self.phase_started_us = now_us;
            self.field = 0;
            self.passcode.wipe();
            self.confirm.wipe();
            self.note = Note::None;
            self.shake_started_us = None;
            self.full_repaint = true;
        }
    }

    /// Re-read the checker's status and move to the screen it calls for.
    fn refresh(&mut self, checker: &mut impl PasscodeChecker, now_us: u64) {
        let Some(status) = checker.status() else {
            self.enter(Phase::Waiting, now_us);
            self.next_poll_us = now_us + WAITING_POLL_US;
            return;
        };

        if status.passcode_set == 0 {
            if !self.greeted && self.phase != Phase::Setup {
                self.greeted = true;
                self.enter(Phase::Greeting, now_us);
            } else {
                self.enter(Phase::Setup, now_us);
            }
        } else if status.locked != 0 {
            self.enter(Phase::Locked, now_us);
            self.next_poll_us = now_us + LOCKED_POLL_US;
        } else {
            self.enter(Phase::Login, now_us);
            self.start_wait(status.wait_seconds, now_us);
        }
    }

    fn start_wait(&mut self, seconds: u32, now_us: u64) {
        if seconds == 0 {
            if self.note == Note::Wait {
                self.note = Note::None;
            }
            return;
        }
        self.note = Note::Wait;
        self.wait_until_us = now_us + u64::from(seconds) * 1_000_000;
        self.passcode.wipe();
    }

    fn waiting_out_a_delay(&self, now_us: u64) -> bool {
        self.note == Note::Wait && now_us < self.wait_until_us
    }

    /// One input event. True when the screen needs repainting.
    pub fn event(&mut self, event: Event, now_us: u64) -> bool {
        if self.pending != Pending::None || !matches!(self.phase, Phase::Setup | Phase::Login) {
            return false;
        }

        // Clicks need the screen size for hit testing: see `click`.
        match event {
            Event::KeyDown { code, character } => self.key(code, character, now_us),
            _ => false,
        }
    }

    /// A primary click at `position` on a screen of `screen` pixels.
    pub fn click(&mut self, position: Point, screen: Size, now_us: u64) -> bool {
        if self.pending != Pending::None || !matches!(self.phase, Phase::Setup | Phase::Login) {
            return false;
        }
        let layout = self.layout(screen, now_us);
        if layout.button.contains(position) {
            return self.submit(now_us);
        }
        for (index, field) in layout.fields.iter().enumerate().take(layout.field_count) {
            if field.contains(position) && self.field != index {
                self.field = index;
                return true;
            }
        }
        false
    }

    fn current(&mut self) -> &mut Secret {
        if self.field == 1 { &mut self.confirm } else { &mut self.passcode }
    }

    fn key(&mut self, code: u32, character: Option<char>, now_us: u64) -> bool {
        match code {
            key::ENTER => return self.submit(now_us),
            key::BACKSPACE => return self.current().pop(),
            key::ESCAPE => {
                let had = !self.current().is_empty();
                self.current().wipe();
                return had;
            }
            key::TAB if self.phase == Phase::Setup => {
                self.field = 1 - self.field;
                return true;
            }
            _ => {}
        }

        if self.waiting_out_a_delay(now_us) {
            return false;
        }

        match character {
            Some(character) if (' '..='~').contains(&character) => {
                if !self.current().push(character as u8) {
                    return false;
                }
                if matches!(self.note, Note::Error(_)) {
                    self.note = Note::None;
                }
                true
            }
            _ => false,
        }
    }

    fn submit(&mut self, now_us: u64) -> bool {
        let minimum = UI_SERVICE_PASSCODE_MIN as usize;
        match self.phase {
            Phase::Setup => {
                if self.passcode.len() < minimum {
                    self.field = 0;
                    self.note = Note::Error("Use at least 4 characters.");
                } else if self.confirm.is_empty() {
                    self.field = 1;
                } else if !self.passcode.matches(&self.confirm) {
                    self.confirm.wipe();
                    self.field = 1;
                    self.note = Note::Error("The passcodes do not match.");
                } else {
                    self.pending = Pending::Set;
                    self.note = Note::None;
                }
                true
            }
            Phase::Login => {
                if self.waiting_out_a_delay(now_us) {
                    return false;
                }
                if self.passcode.len() < minimum {
                    self.note = Note::Error("Passcodes have at least 4 characters.");
                } else {
                    self.pending = Pending::Verify;
                    self.note = Note::None;
                }
                true
            }
            _ => false,
        }
    }

    /// Whether a checker call is waiting for `run_pending`. The glue paints
    /// the "checking" state first, since the call blocks.
    pub fn has_pending(&self) -> bool {
        self.pending != Pending::None
    }

    /// Make the checker call the last submit asked for.
    pub fn run_pending(&mut self, checker: &mut impl PasscodeChecker, now_us: u64) {
        let pending = core::mem::replace(&mut self.pending, Pending::None);
        let (result, wait) = match pending {
            Pending::None => return,
            Pending::Verify => checker.verify(self.passcode.as_bytes()),
            Pending::Set => checker.set(self.passcode.as_bytes()),
        };
        self.passcode.wipe();
        self.confirm.wipe();
        self.field = 0;

        match result {
            AuthResult::Ok => self.enter(Phase::Done, now_us),
            AuthResult::Denied if pending == Pending::Set => {
                // Someone set a passcode in the meantime: ask for that one.
                self.enter(Phase::Login, now_us);
                self.note = Note::Error("A passcode is already set. Enter it to unlock.");
            }
            AuthResult::Denied => {
                self.shake_started_us = Some(now_us);
                self.note = Note::Error("Wrong passcode.");
                // The failure may have started a delay: show it right away.
                if let Some(status) = checker.status() {
                    if status.locked != 0 {
                        self.enter(Phase::Locked, now_us);
                        self.next_poll_us = now_us + LOCKED_POLL_US;
                    } else {
                        self.start_wait(status.wait_seconds, now_us);
                    }
                }
            }
            AuthResult::RetryLater => self.start_wait(wait.max(1), now_us),
            AuthResult::Locked => {
                self.enter(Phase::Locked, now_us);
                self.next_poll_us = now_us + LOCKED_POLL_US;
            }
            AuthResult::NotSet => self.refresh(checker, now_us),
            AuthResult::Invalid => self.note = Note::Error("Passcodes are 4 to 64 characters."),
            AuthResult::Unavailable | AuthResult::Error => {
                self.note = Note::Error("The Trusted Enclave did not answer. Try again.");
                if checker.status().is_none() {
                    self.enter(Phase::Waiting, now_us);
                    self.next_poll_us = now_us + WAITING_POLL_US;
                }
            }
        }
        self.full_repaint = true;
    }

    /// Timers: the greeting, the countdown, re-asking the checker, the shake.
    /// True when the screen needs repainting.
    pub fn tick(&mut self, checker: &mut impl PasscodeChecker, now_us: u64) -> bool {
        match self.phase {
            Phase::Greeting => {
                if now_us.saturating_sub(self.phase_started_us) >= GREETING_US {
                    self.enter(Phase::Setup, now_us);
                }
                true
            }
            Phase::Waiting | Phase::Locked if now_us >= self.next_poll_us => {
                let before = self.phase;
                self.refresh(checker, now_us);
                if self.phase == before {
                    self.next_poll_us = now_us + if before == Phase::Locked { LOCKED_POLL_US } else { WAITING_POLL_US };
                }
                self.phase != before
            }
            Phase::Login if self.note == Note::Wait => {
                if now_us >= self.wait_until_us {
                    self.note = Note::None;
                    self.refresh(checker, now_us);
                }
                true
            }
            _ => match self.shake_started_us {
                Some(started) if now_us.saturating_sub(started) < SHAKE_US => true,
                Some(_) => {
                    self.shake_started_us = None;
                    true
                }
                None => false,
            },
        }
    }

    /// Whether anything moves on its own (the frame loop can idle otherwise).
    pub fn animating(&self) -> bool {
        self.phase == Phase::Greeting || self.shake_started_us.is_some() || self.note == Note::Wait
    }

    /// The part of the screen the next paint changes, or the whole screen.
    /// Consumes the "everything changed" flag.
    pub fn take_repaint_region(&mut self, screen: Size, now_us: u64) -> Rect {
        let whole = Rect::new(0, 0, screen.width, screen.height);
        if core::mem::replace(&mut self.full_repaint, false) {
            return whole;
        }
        match self.phase {
            Phase::Greeting => {
                let band = scale::pt(150);
                Rect::new(0, (screen.height / 2) as i32 - band as i32 / 2 - scale::pt_i32(40), screen.width, band)
            }
            Phase::Setup | Phase::Login | Phase::Locked => {
                let card = self.layout(screen, now_us).card;
                let margin = scale::pt(16);
                Rect::new(
                    card.origin.x - margin as i32,
                    card.origin.y,
                    card.size.width + 2 * margin,
                    card.size.height,
                )
            }
            _ => whole,
        }
    }

    /// Whether the next paint wants the display face (Borel): only the
    /// greeting does, everything else is Inter.
    pub fn wants_display_face(&self) -> bool {
        self.phase == Phase::Greeting
    }

    /// Greeting opacity (0..=255) and how far it still has to rise.
    fn greeting_state(&self, now_us: u64) -> (u8, i32) {
        let elapsed = now_us.saturating_sub(self.phase_started_us);
        let opacity = if elapsed < FADE_IN_US {
            ease_out(elapsed, FADE_IN_US)
        } else if elapsed <= FADE_IN_US + HOLD_US {
            255
        } else {
            let out = elapsed - FADE_IN_US - HOLD_US;
            if out >= FADE_OUT_US { 0 } else { (255 - out * 255 / FADE_OUT_US) as u8 }
        };
        let rise = if elapsed < FADE_IN_US {
            scale::pt_i32(14) * (255 - i32::from(opacity)) / 255
        } else {
            0
        };
        (opacity, rise)
    }

    fn shake_offset(&self, now_us: u64) -> i32 {
        const STEPS: [i32; 10] = [0, -10, 10, -8, 8, -5, 5, -2, 2, 0];
        match self.shake_started_us {
            Some(started) => {
                let elapsed = now_us.saturating_sub(started);
                if elapsed >= SHAKE_US {
                    return 0;
                }
                let step = (elapsed * STEPS.len() as u64 / SHAKE_US) as usize;
                scale::pt_i32(STEPS[step.min(STEPS.len() - 1)])
            }
            None => 0,
        }
    }

    fn layout(&self, screen: Size, now_us: u64) -> Layout {
        let pt = scale::pt;
        let (height, field_count) = match self.phase {
            Phase::Setup => (pt(316), 2),
            Phase::Locked => (pt(150), 0),
            _ => (pt(232), 1),
        };
        let width = pt(420).min(screen.width.saturating_sub(pt(32)));
        let x = (screen.width as i32 - width as i32) / 2 + self.shake_offset(now_us);
        let y = (screen.height as i32 - height as i32) / 2;
        let card = Rect::new(x, y, width, height);
        let inset = pt(32) as i32;
        let field_width = width.saturating_sub(2 * pt(32));
        let first_field_y = y + pt(if field_count == 2 { 124 } else { 108 }) as i32;
        let fields = [
            Rect::new(x + inset, first_field_y, field_width, pt(28)),
            Rect::new(x + inset, first_field_y + pt(62) as i32, field_width, pt(28)),
        ];
        let button_width = pt(112);
        let button = if field_count == 0 {
            Rect::new(0, 0, 0, 0)
        } else {
            Rect::new(
                x + width as i32 - inset - button_width as i32,
                y + height as i32 - pt(62) as i32,
                button_width,
                pt(36),
            )
        };
        Layout { card, fields, field_count, button }
    }

    /// Paint the screen, or `region` of it: the backdrop first, then this
    /// screen's content. `text` is Borel when `wants_display_face` said so
    /// (or Inter as a fallback), Inter otherwise.
    pub fn paint<T: TextRenderer>(
        &self,
        surface: &mut Surface<'_>,
        backdrop: &Backdrop<'_>,
        text: &mut T,
        region: Rect,
        now_us: u64,
    ) {
        backdrop.paint(surface, region);
        let screen = Size::new(surface.width(), surface.height());

        match self.phase {
            Phase::Waiting => {
                let center = (screen.height / 2) as i32;
                centered(surface, text, "Waiting for the Trusted Enclave", Color::WHITE, scale::pt(20), true, center - scale::pt_i32(26));
                centered(
                    surface,
                    text,
                    "sevOS unlocks once tepOS can check your passcode.",
                    Color::rgba(255, 255, 255, 200),
                    scale::pt(13),
                    false,
                    center + scale::pt_i32(8),
                );
            }
            Phase::Greeting => {
                let (opacity, rise) = self.greeting_state(now_us);
                if opacity != 0 {
                    // 56 pt, but never over 84 px: Borel's tallest letter is
                    // 1.5 em and ui-text skips glyphs over 128 px.
                    let size = scale::pt(56).min(84);
                    let line = text.measure(GREETING, size, false);
                    // Borel's line box is two em tall around a baseline near
                    // its middle: centring the box centres the letters.
                    let top = (screen.height as i32 - line.height as i32) / 2 - scale::pt_i32(20) + rise;
                    let left = (screen.width as i32 - line.width as i32) / 2;
                    text.draw(surface, Point::new(left, top), GREETING, Color::WHITE.with_alpha(opacity), size, false);
                }
            }
            Phase::Setup | Phase::Login | Phase::Locked => self.paint_card(surface, text, screen, now_us),
            Phase::Done => {}
        }
    }

    fn paint_card<T: TextRenderer>(&self, surface: &mut Surface<'_>, text: &mut T, screen: Size, now_us: u64) {
        let pt = scale::pt;
        let layout = self.layout(screen, now_us);
        let card = layout.card;
        let left = card.origin.x + pt(32) as i32;
        surface.fill_rounded_rect(card, pt(22), CARD);

        let (title, lines): (&str, [&str; 2]) = match self.phase {
            Phase::Setup => ("Create a passcode", ["Protect your data and create a strong passcode.", ""]),
            Phase::Locked => ("Passcode locked", [
                "There were too many wrong attempts.",
                "It can only be reset on the Trusted Enclave's machine.",
            ]),
            _ => ("Enter your passcode", ["sevOS is locked. tepOS checks your passcode.", ""]),
        };
        text.draw(surface, Point::new(left, card.origin.y + pt(28) as i32), title, INK, pt(22), true);
        for (index, line) in lines.iter().enumerate() {
            if !line.is_empty() {
                let y = card.origin.y + pt(66 + 20 * index as u32) as i32;
                text.draw(surface, Point::new(left, y), line, MUTED, pt(13), false);
            }
        }

        if self.phase == Phase::Locked {
            return;
        }

        let busy = self.pending != Pending::None;
        let waiting = self.waiting_out_a_delay(now_us);
        let labels = ["Passcode", "Confirm passcode"];
        for index in 0..layout.field_count {
            let field = layout.fields[index];
            if layout.field_count == 2 {
                text.draw(surface, Point::new(field.origin.x, field.origin.y - pt(20) as i32), labels[index], MUTED, pt(12), false);
            }
            let focused = index == self.field && !waiting;
            let radius = pt(7);
            if focused {
                let ring = pt(3).max(2);
                round_rect(
                    surface,
                    Rect::new(field.origin.x - ring as i32, field.origin.y - ring as i32, field.size.width + 2 * ring, field.size.height + 2 * ring),
                    radius + ring,
                    FOCUS_RING,
                );
            }
            let border = 1;
            round_rect(surface, field, radius, FIELD_BORDER);
            round_rect(
                surface,
                Rect::new(field.origin.x + border, field.origin.y + border, field.size.width - 2 * border as u32, field.size.height - 2 * border as u32),
                radius.saturating_sub(1),
                FIELD,
            );

            let secret = if index == 1 { &self.confirm } else { &self.passcode };
            let first = field.origin.x + pt(10) as i32;
            let middle = field.origin.y + field.size.height as i32 / 2;
            if secret.is_empty() {
                let placeholder = match (self.phase, index) {
                    (Phase::Setup, 0) => "Required",
                    (Phase::Setup, _) => "Verify",
                    _ => "Passcode",
                };
                let measured = text.measure(placeholder, pt(13), false);
                text.draw(surface, Point::new(first, middle - measured.height as i32 / 2), placeholder, PLACEHOLDER, pt(13), false);
            }
            let dot = pt(3).max(2);
            let spacing = pt(10) as i32;
            let room = (field.size.width as i32 - pt(24) as i32) / spacing;
            let shown = (secret.len() as i32).min(room.max(0));
            for dot_index in 0..shown {
                surface.fill_circle(Point::new(first + dot as i32 + dot_index * spacing, middle), dot, INK);
            }
            if focused && !busy {
                let caret_x = if shown == 0 { first - 1 } else { first + shown * spacing + 1 };
                surface.fill_rect(Rect::new(caret_x, middle - pt(8) as i32, pt(1).max(1), pt(16)), INK);
            }
        }

        // The note line sits under the last field.
        let note_y = layout.fields[layout.field_count - 1].origin.y + pt(38) as i32;
        let mut countdown = [0u8; 48];
        let note: Option<(&str, Color)> = if busy {
            Some((if self.pending == Pending::Set { "Setting your passcode..." } else { "Checking..." }, MUTED))
        } else {
            match self.note {
                Note::None => None,
                Note::Error(message) => Some((message, DANGER)),
                Note::Wait if waiting => {
                    let seconds = (self.wait_until_us - now_us).div_ceil(1_000_000);
                    Some((format_wait(seconds, &mut countdown), DANGER))
                }
                Note::Wait => None,
            }
        };
        if let Some((message, color)) = note {
            text.draw(surface, Point::new(left, note_y), message, color, pt(12), false);
        }

        let button = layout.button;
        let enabled = !busy && !waiting;
        surface.fill_rounded_rect(button, button.size.height / 2, if enabled { ACCENT } else { ACCENT_DIM });
        let label = if self.phase == Phase::Setup { "Continue" } else { "Unlock" };
        let measured = text.measure(label, pt(14), true);
        text.draw(
            surface,
            Point::new(
                button.origin.x + (button.size.width as i32 - measured.width as i32) / 2,
                button.origin.y + (button.size.height as i32 - measured.height as i32) / 2,
            ),
            label,
            Color::WHITE,
            pt(14),
            true,
        );

        let hint = if self.phase == Phase::Setup { "Tab switches fields" } else { "Press Enter to unlock" };
        text.draw(surface, Point::new(left, button.origin.y + pt(10) as i32), hint, MUTED, pt(12), false);
    }
}

impl Default for LoginScreen {
    fn default() -> Self {
        Self::new()
    }
}

struct Layout {
    card: Rect,
    fields: [Rect; 2],
    field_count: usize,
    button: Rect,
}

/// A rectangle with anti-aliased circular corners: interior spans are
/// filled, corner pixels are blended by how much of them the arc covers.
/// (`Canvas::fill_rounded_rect` draws hard-edged squircle corners, which at
/// small sizes read as square.)
fn round_rect(surface: &mut Surface<'_>, rect: Rect, radius: u32, color: Color) {
    let (width, height) = (rect.size.width as i32, rect.size.height as i32);
    let r = (radius as i32).min(width / 2).min(height / 2).max(0);
    let (x0, y0) = (rect.origin.x, rect.origin.y);

    // The middle band and the side bands between the corners.
    surface.fill_rect(Rect::new(x0, y0 + r, width as u32, (height - 2 * r).max(0) as u32), color);
    surface.fill_rect(Rect::new(x0 + r, y0, (width - 2 * r).max(0) as u32, r as u32), color);
    surface.fill_rect(Rect::new(x0 + r, y0 + height - r, (width - 2 * r).max(0) as u32, r as u32), color);

    // The corners: coverage from each pixel centre's distance to the arc
    // centre, in 1/256 px, with a one-pixel soft edge.
    for dy in 0..r {
        for dx in 0..r {
            let cx = (r - dx) * 256 - 128;
            let cy = (r - dy) * 256 - 128;
            let distance = isqrt((cx as u64 * cx as u64 + cy as u64 * cy as u64) as u64) as i32;
            let coverage = (r * 256 - distance + 128).clamp(0, 256);
            if coverage == 0 {
                continue;
            }
            let alpha = (u32::from(color.alpha) * coverage as u32 / 256) as u8;
            let tint = color.with_alpha(alpha);
            for (px, py) in [
                (x0 + dx, y0 + dy),
                (x0 + width - 1 - dx, y0 + dy),
                (x0 + dx, y0 + height - 1 - dy),
                (x0 + width - 1 - dx, y0 + height - 1 - dy),
            ] {
                surface.blend_pixel(Point::new(px, py), tint);
            }
        }
    }
}

fn isqrt(value: u64) -> u64 {
    if value < 2 {
        return value;
    }
    let mut x = value;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + value / x) / 2;
    }
    x
}

fn centered<T: TextRenderer>(surface: &mut Surface<'_>, text: &mut T, line: &str, color: Color, size: u32, semibold: bool, y: i32) {
    let measured = text.measure(line, size, semibold);
    let x = (surface.width() as i32 - measured.width as i32) / 2;
    text.draw(surface, Point::new(x, y), line, color, size, semibold);
}

/// Ease-out cubic from 0 to 255 over `duration`.
fn ease_out(elapsed: u64, duration: u64) -> u8 {
    if duration == 0 || elapsed >= duration {
        return 255;
    }
    let inverse = duration - elapsed;
    let cubed = inverse * inverse / duration * inverse / duration;
    (255 - cubed * 255 / duration).min(255) as u8
}

/// "Too many attempts. Try again in M:SS."
fn format_wait(seconds: u64, buffer: &mut [u8; 48]) -> &str {
    const PREFIX: &[u8] = b"Too many attempts. Try again in ";
    let mut length = PREFIX.len();
    buffer[..length].copy_from_slice(PREFIX);

    let minutes = seconds / 60;
    let mut digits = [0u8; 20];
    let mut count = 0;
    let mut value = minutes;
    loop {
        digits[count] = b'0' + (value % 10) as u8;
        count += 1;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    for index in (0..count).rev() {
        buffer[length] = digits[index];
        length += 1;
    }
    let rest = seconds % 60;
    for byte in [b':', b'0' + (rest / 10) as u8, b'0' + (rest % 10) as u8, b'.'] {
        buffer[length] = byte;
        length += 1;
    }
    core::str::from_utf8(&buffer[..length]).unwrap_or("Too many attempts.")
}

#[cfg(test)]
mod tests;
