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

/// The traffic lights' centres, as 1x points from the window's left edge:
/// close, minimize, zoom. The desktop hit-tests the same points.
pub(crate) const TRAFFIC_LIGHT_X: [u32; 3] = [22, 42, 62];
/// A light's radius, in 1x points (its halo is one point wider).
pub(crate) const TRAFFIC_LIGHT_RADIUS: u32 = 6;

/// Draw the standard Aqua traffic lights: coloured in the key window, flat
/// grey in every other one, as on macOS.
///
/// Positions, diameter and halo are expressed as real 1x-point Aqua values
/// (12px diameter, 20px spacing) and converted to physical pixels through
/// `ui_core::scale::pt` for whatever content scale the connected host
/// actually reported, instead of a hardcoded 2x. `titlebar_height` centers
/// the lights vertically instead of assuming a fixed bar height.
pub(crate) fn UIDrawTrafficLights<C: Canvas>(canvas: &mut C, titlebar_height: u32, active: bool) {
    const INACTIVE: Color = Color::rgb(206, 206, 210);
    let colors = [Color::rgb(255, 95, 87), Color::rgb(255, 189, 46), Color::rgb(40, 201, 64)];
    let y = (titlebar_height / 2) as i32;

    for (color, x) in colors.into_iter().zip(TRAFFIC_LIGHT_X) {
        let x = pt(x) as i32;
        canvas.fill_circle(Point::new(x, y), pt(TRAFFIC_LIGHT_RADIUS + 1), Color::rgba(0, 0, 0, if active { 45 } else { 25 }));
        canvas.fill_circle(Point::new(x, y), pt(TRAFFIC_LIGHT_RADIUS), if active { color } else { INACTIVE });
    }
}

/// Draw an Aqua window surface, including chrome and title.
pub(crate) fn UIDrawWindow<C: Canvas, T: TextRenderer>(
    canvas: &mut C,
    text: &mut T,
    window: &Window,
    title: &str,
    active: bool,
) {
    window.draw_chrome(canvas, true);
    UIDrawTitlebar(canvas, text, window, title, active);
}

/// Just the titlebar band: its background, the traffic lights and the title.
/// Repainting it alone is how a window turns key or not without touching
/// the content an app draws in pieces.
pub(crate) fn UIDrawTitlebar<C: Canvas, T: TextRenderer>(
    canvas: &mut C,
    text: &mut T,
    window: &Window,
    title: &str,
    active: bool,
) {
    // Top-rounded, not a plain `fill_rect`: the titlebar spans the window's
    // full outer width, so a square fill here would paint right over the
    // rounded top corners `draw_chrome` just cut out above.
    let titlebar = Rect::new(0, 0, window.frame().size.width, window.style().titlebar_height);
    let background = if active { window.style().titlebar } else { Color::rgb(236, 236, 238) };
    canvas.fill_top_rounded_rect(titlebar, window.corner_radius(), background);
    UIDrawTrafficLights(canvas, window.style().titlebar_height, active);

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
        if active { system_color::LABEL } else { system_color::TERTIARY_LABEL },
        title_point_size,
        false,
    );
}
