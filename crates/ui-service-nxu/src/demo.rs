use ui_core::{Color, Event, Point, PointerButton, Rect};
use ui_render::Canvas;
use ui_widgets::Button;

pub(crate) fn apply_button_state(button: &mut Button<'_>, state: u32) -> bool {
    let center = Point::new(
        button.frame().origin.x + (button.frame().size.width / 2) as i32,
        button.frame().origin.y + (button.frame().size.height / 2) as i32,
    );

    match state {
        0 => true,
        1 => {
            button.handle_event(Event::PointerMoved { position: center });
            true
        }
        2 => {
            button.handle_event(Event::PointerDown {
                position: center,
                button: PointerButton::Primary,
            });
            true
        }
        3 => {
            button.set_enabled(false);
            true
        }
        _ => false,
    }
}

pub(crate) fn draw_demo<C: Canvas>(surface: &mut C) {
    surface.fill(Color::rgb(22, 24, 29));

    for (index, state) in [0u32, 1, 2, 3].into_iter().enumerate() {
        let mut button = Button::new("", Rect::new(40 + index as i32 * 180, 60, 150, 48));
        if apply_button_state(&mut button, state) {
            button.draw(surface);
        }
    }
}
