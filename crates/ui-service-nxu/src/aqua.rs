#![allow(non_snake_case)]

use ui_core::{scale::pt, system_color, Color, Point, Rect};
use ui_render::{Canvas, TextRenderer};
use ui_window::Window;

/// Primitive Aqua rounded rectangle drawing.
#[allow(dead_code)]
pub(crate) fn UIDrawRoundedRect<C: Canvas>(
    canvas: &mut C,
    rect: Rect,
    radius: u32,
    color: Color,
) {
    canvas.fill_rounded_rect(rect, radius, color);
}

/// Draw the standard Aqua traffic lights.
///
/// Positions, diameter and halo are expressed as real 1x-point Aqua values
/// (12px diameter, 20px spacing) and converted to physical pixels through
/// `ui_core::scale::pt` for whatever content scale the connected host
/// actually reported, instead of a hardcoded 2x. `titlebar_height` centers
/// the lights vertically instead of assuming a fixed bar height.
pub(crate) fn UIDrawTrafficLights<C: Canvas>(canvas: &mut C, titlebar_height: u32) {
    let lights = [
        (Color::rgb(255, 95, 87), pt(22)),
        (Color::rgb(255, 189, 46), pt(42)),
        (Color::rgb(40, 201, 64), pt(62)),
    ];
    let y = (titlebar_height / 2) as i32;

    for (color, x) in lights {
        canvas.fill_circle(Point::new(x as i32, y), pt(7), Color::rgba(0, 0, 0, 45));
        canvas.fill_circle(Point::new(x as i32, y), pt(6), color);
    }
}

/// Draw an Aqua window surface, including chrome and title.
pub(crate) fn UIDrawWindow<C: Canvas, T: TextRenderer>(
    canvas: &mut C,
    text: &mut T,
    window: &Window,
    title: &str,
) {
    window.draw_chrome(canvas, true);

    // Top-rounded, not a plain `fill_rect`: the titlebar spans the window's
    // full outer width, so a square fill here would paint right over the
    // rounded top corners `draw_chrome` just cut out above.
    let titlebar = Rect::new(0, 0, window.frame().size.width, window.style().titlebar_height);
    canvas.fill_top_rounded_rect(titlebar, window.corner_radius(), window.style().titlebar);
    UIDrawTrafficLights(canvas, window.style().titlebar_height);

    // 13pt, a real Aqua title label's size at 1x, converted to physical
    // pixels for the host's actual content scale (see `ui_core::scale`)
    // instead of a hardcoded 26 (2x-only) point size.
    let title_point_size = pt(13);
    let measured = text.measure(title, title_point_size, false);
    let titlebar_height = window.style().titlebar_height;
    let x = ((window.frame().size.width.saturating_sub(measured.width)) / 2) as i32;
    let y = ((titlebar_height.saturating_sub(measured.height)) / 2) as i32;

    text.draw(
        canvas,
        Point::new(x, y),
        title,
        system_color::LABEL,
        title_point_size,
        false,
    );
}
