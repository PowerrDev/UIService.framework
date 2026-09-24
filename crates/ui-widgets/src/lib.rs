#![no_std]

//! Reusable UI controls and higher-level views.

use ui_core::{system_color, Color, Event, PointerButton, Rect};
use ui_render::Canvas;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ButtonState {
    Idle,
    Hovered,
    Pressed,
    Disabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ButtonStyle {
    pub background: Color,
    pub hovered_background: Color,
    pub pressed_background: Color,
    pub disabled_background: Color,
    pub border: Color,
    pub corner_radius: u32,
    pub border_width: u32,
}

impl Default for ButtonStyle {
    fn default() -> Self {
        Self {
            // The system accent in every enabled state, so a label drawn in
            // `system_color::ON_ACCENT` stays at 4.5:1 however the button is
            // being touched (the old hover, 72,130,250, fell to 3.8:1).
            background: system_color::ACCENT,
            hovered_background: system_color::ACCENT_HOVER,
            pressed_background: system_color::ACCENT_PRESSED,
            disabled_background: Color::rgb(118, 122, 132),
            border: Color::rgb(255, 255, 255),
            corner_radius: 10,
            border_width: 1,
        }
    }
}

pub struct Button<'a> {
    title: &'a str,
    frame: Rect,
    style: ButtonStyle,
    state: ButtonState,
    enabled: bool,
    armed: bool,
}

impl<'a> Button<'a> {
    pub fn new(title: &'a str, frame: Rect) -> Self {
        Self {
            title,
            frame,
            style: ButtonStyle::default(),
            state: ButtonState::Idle,
            enabled: true,
            armed: false,
        }
    }

    pub fn title(&self) -> &'a str {
        self.title
    }

    pub fn frame(&self) -> Rect {
        self.frame
    }

    pub fn state(&self) -> ButtonState {
        self.state
    }

    pub fn set_style(&mut self, style: ButtonStyle) {
        self.style = style;
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.armed = false;
        self.state = if enabled {
            ButtonState::Idle
        } else {
            ButtonState::Disabled
        };
    }

    pub fn handle_event(&mut self, event: Event) -> bool {
        if !self.enabled {
            return false;
        }

        match event {
            Event::PointerMoved { position } => {
                if self.armed {
                    self.state = if self.frame.contains(position) {
                        ButtonState::Pressed
                    } else {
                        ButtonState::Idle
                    };
                } else {
                    self.state = if self.frame.contains(position) {
                        ButtonState::Hovered
                    } else {
                        ButtonState::Idle
                    };
                }
            }
            Event::PointerDown {
                position,
                button: PointerButton::Primary,
            } => {
                self.armed = self.frame.contains(position);
                self.state = if self.armed {
                    ButtonState::Pressed
                } else {
                    ButtonState::Idle
                };
            }
            Event::PointerUp {
                position,
                button: PointerButton::Primary,
            } => {
                let activated = self.armed && self.frame.contains(position);
                self.armed = false;
                self.state = if self.frame.contains(position) {
                    ButtonState::Hovered
                } else {
                    ButtonState::Idle
                };
                return activated;
            }
            Event::PointerLeft => {
                self.armed = false;
                self.state = ButtonState::Idle;
            }
            _ => {}
        }

        false
    }

    pub fn draw<C: Canvas + ?Sized>(&self, canvas: &mut C) {
        let background = match self.state {
            ButtonState::Idle => self.style.background,
            ButtonState::Hovered => self.style.hovered_background,
            ButtonState::Pressed => self.style.pressed_background,
            ButtonState::Disabled => self.style.disabled_background,
        };

        if self.style.border_width > 0 {
            canvas.fill_rounded_rect(self.frame, self.style.corner_radius, self.style.border);
        }

        let inset = self.style.border_width.min(self.frame.size.width / 2).min(self.frame.size.height / 2);
        let inner = Rect::new(
            self.frame.origin.x + inset as i32,
            self.frame.origin.y + inset as i32,
            self.frame.size.width.saturating_sub(inset * 2),
            self.frame.size.height.saturating_sub(inset * 2),
        );
        canvas.fill_rounded_rect(inner, self.style.corner_radius.saturating_sub(inset), background);
    }
}
