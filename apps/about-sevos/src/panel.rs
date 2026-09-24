use ui::prelude::*;

use crate::SYSTEM_INFO;

const BACKGROUND: Color = system_color::WINDOW_BACKGROUND;
const PREVIEW_BACKGROUND: Color = Color::rgb(239, 240, 244);
const CARD: Color = Color::rgb(252, 252, 253);
const ICON: Color = Color::rgb(45, 111, 236);
const TEXT: Color = system_color::LABEL;
const SECONDARY_TEXT: Color = system_color::SECONDARY_LABEL;

pub(crate) fn draw(ui: &mut Frame<'_>, standalone: bool) {
    let size = ui.size();
    ui.fill(if standalone { PREVIEW_BACKGROUND } else { BACKGROUND });

    if size.width < scale::pt(160) || size.height < scale::pt(180) {
        return;
    }

    // Every metric below is a real 1x-point value (520x360 card, 24pt name,
    // etc.) converted to physical pixels through `scale::pt` for whatever
    // content scale the connected host actually reported. The NXU boot
    // targets drive virtio-gpu at a resolution QEMU's cocoa backend then
    // presents natively against the host's real backingScaleFactor, so this
    // no longer assumes that factor is always exactly 2 (see `ui_core::scale`).
    let card_width = size.width.min(scale::pt(520)).saturating_sub(scale::pt(32));
    let card_height = size.height.min(scale::pt(360)).saturating_sub(scale::pt(32));
    let card_x = ((size.width.saturating_sub(card_width)) / 2) as i32;
    let card_y = ((size.height.saturating_sub(card_height)) / 2) as i32;
    let card = Rect::new(card_x, card_y, card_width, card_height);
    let card_color = if standalone { CARD } else { BACKGROUND };
    let corner_radius = if standalone { scale::pt(24) } else { 0 };
    ui.fill_rounded_rect(card, corner_radius, card_color);

    let icon_size = scale::pt(84).min(card_width.saturating_sub(scale::pt(32)));
    let icon_x = card_x + ((card_width.saturating_sub(icon_size)) / 2) as i32;
    let icon_y = card_y + scale::pt(34) as i32;
    ui.fill_rounded_rect(Rect::new(icon_x, icon_y, icon_size, icon_size), scale::pt(22), ICON);

    let stem_width = (icon_size / 7).max(scale::pt(4));
    let stem_height = (icon_size / 2).max(scale::pt(12));
    let stem_x = icon_x + ((icon_size - stem_width) / 2) as i32;
    let stem_y = icon_y + (icon_size / 3) as i32;
    ui.fill_rounded_rect(
        Rect::new(stem_x, stem_y, stem_width, stem_height),
        stem_width / 2,
        Color::WHITE,
    );
    ui.fill_rounded_rect(
        Rect::new(stem_x, icon_y + (icon_size / 5) as i32, stem_width, stem_width),
        stem_width / 2,
        Color::WHITE,
    );

    let name_point_size = scale::pt(24);
    let name_size = ui.measure_with_weight(SYSTEM_INFO.product_name, name_point_size, FontWeight::Semibold);
    let name_x = card_x + ((card_width.saturating_sub(name_size.width)) / 2) as i32;
    let name_y = icon_y + icon_size as i32 + scale::pt(24) as i32;
    ui.text_semibold(
        Point::new(name_x, name_y),
        SYSTEM_INFO.product_name,
        TEXT,
        name_point_size,
    );

    let version_y = name_y + name_size.height as i32 + scale::pt(10) as i32;
    centered_line(ui, SYSTEM_INFO.version, version_y, scale::pt(14));
    centered_line(ui, SYSTEM_INFO.build, version_y + scale::pt(24) as i32, scale::pt(13));
    centered_line(ui, SYSTEM_INFO.kernel, version_y + scale::pt(54) as i32, scale::pt(13));
    centered_line(ui, SYSTEM_INFO.copyright, version_y + scale::pt(78) as i32, scale::pt(12));
}

fn centered_line(ui: &mut Frame<'_>, value: &str, y: i32, point_size: u32) {
    let size = ui.size();
    let measured = ui.measure(value, point_size);
    let x = ((size.width.saturating_sub(measured.width)) / 2) as i32;
    ui.text(Point::new(x, y), value, SECONDARY_TEXT, point_size);
}
