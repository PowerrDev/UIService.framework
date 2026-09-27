//! The system menu bar: `<Logo> <AppName> <App menus> Window` on the left,
//! the clock on the right, and the menu that drops down from a title.
//!
//! The bar itself is painted into the desktop layer (the wallpaper under it
//! is drawn once and never again); an open menu is its own WindowServer
//! window, created on top of everything when it opens and destroyed when it
//! closes. What a menu holds is decided by the desktop (`desktop.rs`) when it
//! opens, from whichever app is frontmost -- this module only lays it out,
//! draws it and hit-tests it.

use ui_core::{scale::pt, system_color, Color, Point, Rect, Size};
use ui_render::{Canvas, Surface, TextRenderer};

use crate::scene;
use crate::storage::StaticCell;
use crate::windowserver::{self, WindowId};

pub(crate) const MAX_TITLES: usize = 8;
pub(crate) const MAX_ITEMS: usize = 16;

const MENUBAR_BACKGROUND: Color = system_color::MENUBAR;
const MENUBAR_BORDER: Color = Color::rgb(210, 210, 206);
// Menu titles and the clock are full-strength labels, as on macOS: they all
// work, and a muted grey reads as disabled.
const MENUBAR_TEXT: Color = system_color::LABEL;
// The rounded plate behind an open menu's title (the bar colour, darkened:
// Surface fills do not blend, so this is the blended result, pre-mixed).
const TITLE_HIGHLIGHT: Color = Color::rgb(212, 212, 208);

const MENU_BACKGROUND: Color = Color::rgb(246, 246, 246);
const MENU_BORDER: Color = Color::rgb(196, 196, 198);
const MENU_SEPARATOR: Color = Color::rgb(222, 222, 224);
const MENU_HIGHLIGHT: Color = Color::rgb(10, 100, 230);

fn padding() -> i32 {
    pt(8) as i32
}
fn logo_gap() -> i32 {
    pt(6) as i32
}
fn title_point_size() -> u32 {
    pt(13)
}
fn title_gap() -> i32 {
    pt(13) as i32
}
fn item_height() -> u32 {
    pt(22)
}
fn separator_height() -> u32 {
    pt(9)
}
fn menu_inset() -> u32 {
    pt(5)
}
fn check_column() -> u32 {
    pt(24)
}

/// The largest open menu, in physical pixels: 240 x 320 points at 2x.
const MENU_MAX_WIDTH: u32 = 480;
const MENU_MAX_HEIGHT: u32 = 640;
static MENU_PIXELS: StaticCell<[u32; (MENU_MAX_WIDTH * MENU_MAX_HEIGHT) as usize]> =
    StaticCell::new([0; (MENU_MAX_WIDTH * MENU_MAX_HEIGHT) as usize]);

/// What a menu-bar title stands for; the desktop fills its menu on open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TitleKind {
    /// The logo: the system menu (About sevOS).
    System,
    /// The frontmost app's bold name: Hide, Quit.
    Application,
    /// The frontmost app's own menu `n` (as it sent them when it connected).
    AppMenu(usize),
    /// Close, Bring All to Front, and the open apps.
    Window,
}

#[derive(Clone, Copy)]
struct Title {
    kind: TitleKind,
    label: &'static str,
    /// Where it was last drawn, in screen coordinates (the whole bar height).
    rect: Rect,
}

impl Title {
    const EMPTY: Self = Self { kind: TitleKind::Window, label: "", rect: Rect::new(0, 0, 0, 0) };
}

/// What choosing a menu item does. Slots are indices into the desktop's apps.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Action {
    /// Open About sevOS (the Dock launches it).
    About,
    Hide(usize),
    HideOthers,
    ShowAll,
    Quit(usize),
    Command(usize, u32),
    Focus(usize),
}

/// One row of an open menu. `label` is drawn as its two parts run together
/// ("Quit " + the app's name) so no string ever has to be built.
#[derive(Clone, Copy)]
pub(crate) struct Item {
    label: [&'static str; 2],
    action: Option<Action>,
    enabled: bool,
    checked: bool,
}

impl Item {
    pub(crate) const SEPARATOR: Self = Self { label: ["", ""], action: None, enabled: false, checked: false };

    pub(crate) const fn new(label: &'static str, action: Action) -> Self {
        Self { label: [label, ""], action: Some(action), enabled: true, checked: false }
    }

    pub(crate) const fn joined(first: &'static str, second: &'static str, action: Action) -> Self {
        Self { label: [first, second], action: Some(action), enabled: true, checked: false }
    }

    pub(crate) const fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub(crate) const fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    const fn is_separator(&self) -> bool {
        self.action.is_none()
    }

    fn height(&self) -> u32 {
        if self.is_separator() { separator_height() } else { item_height() }
    }
}

/// The list of items the desktop builds for a menu that is opening.
pub(crate) struct Items {
    items: [Item; MAX_ITEMS],
    count: usize,
}

impl Items {
    pub(crate) const fn new() -> Self {
        Self { items: [Item::SEPARATOR; MAX_ITEMS], count: 0 }
    }

    /// Add `item`; a menu that is full drops the rest rather than overflow.
    pub(crate) fn push(&mut self, item: Item) {
        // No leading or doubled dividers: they come from optional sections.
        if item.is_separator() && (self.count == 0 || self.items[self.count - 1].is_separator()) {
            return;
        }
        if self.count < MAX_ITEMS {
            self.items[self.count] = item;
            self.count += 1;
        }
    }

    fn as_slice(&self) -> &[Item] {
        let mut count = self.count;
        // Nor a trailing one.
        while count > 0 && self.items[count - 1].is_separator() {
            count -= 1;
        }
        &self.items[..count]
    }
}

/// The dropdown currently on screen.
struct OpenMenu {
    title: usize,
    items: Items,
    highlighted: Option<usize>,
    window: WindowId,
    frame: Rect,
}

pub(crate) struct MenuBar {
    titles: [Title; MAX_TITLES],
    title_count: usize,
    app_name: &'static str,
    open: Option<OpenMenu>,
}

impl MenuBar {
    pub(crate) const fn new() -> Self {
        Self { titles: [Title::EMPTY; MAX_TITLES], title_count: 0, app_name: "", open: None }
    }

    /// Set the titles for the frontmost app: its name in bold, then its own
    /// menus. The logo comes first and Window last, whatever the app.
    pub(crate) fn set_app(&mut self, app_name: &'static str, menus: &[&'static str]) {
        self.app_name = app_name;
        self.title_count = 0;
        self.push_title(TitleKind::System, "");
        self.push_title(TitleKind::Application, app_name);
        for (index, title) in menus.iter().enumerate().take(MAX_TITLES - 3) {
            self.push_title(TitleKind::AppMenu(index), title);
        }
        self.push_title(TitleKind::Window, "Window");
    }

    fn push_title(&mut self, kind: TitleKind, label: &'static str) {
        if self.title_count < MAX_TITLES {
            self.titles[self.title_count] = Title { kind, label, rect: Rect::new(0, 0, 0, 0) };
            self.title_count += 1;
        }
    }

    pub(crate) fn height() -> u32 {
        scene::menubar_height()
    }

    /// Paint the bar into the desktop layer and remember where each title
    /// landed for hit-testing. `clock` is drawn flush right.
    pub(crate) fn draw<C: Canvas, T: TextRenderer>(&mut self, canvas: &mut C, text: &mut T, clock: &str) {
        let size = canvas.size();
        let bar_height = Self::height().min(size.height);
        canvas.fill_rect(Rect::new(0, 0, size.width, bar_height), MENUBAR_BACKGROUND);
        canvas.fill_rect(Rect::new(0, bar_height as i32 - 1, size.width, 1), MENUBAR_BORDER);

        let open_title = self.open.as_ref().map(|menu| menu.title);
        let point_size = title_point_size();
        let plate_padding = pt(7) as i32;
        let mut cursor = padding();

        for index in 0..self.title_count {
            let title = self.titles[index];
            let bold = title.kind == TitleKind::Application;
            let (content_width, content_height) = if title.kind == TitleKind::System {
                let logo = scene::generated_logo::LOGO_SIZE as i32;
                (logo, logo)
            } else {
                let measured = text.measure(title.label, point_size, bold);
                (measured.width as i32, measured.height as i32)
            };

            // The hit area (and the highlight plate) reaches halfway into
            // the gap on both sides, so there is no dead space between titles.
            let rect = Rect::new(
                cursor - plate_padding,
                0,
                (content_width + plate_padding * 2).max(0) as u32,
                bar_height,
            );
            self.titles[index].rect = rect;

            if open_title == Some(index) {
                let inset = pt(3) as i32;
                let plate = Rect::new(rect.origin.x, inset, rect.size.width, bar_height.saturating_sub(inset as u32 * 2 + 1));
                canvas.fill_rounded_rect(plate, pt(5), TITLE_HIGHLIGHT);
            }

            let y = (bar_height as i32 - content_height) / 2;
            if title.kind == TitleKind::System {
                scene::draw_logo(canvas, Point::new(cursor, y));
                cursor += content_width + logo_gap() + plate_padding;
            } else {
                text.draw(canvas, Point::new(cursor, y), title.label, MENUBAR_TEXT, point_size, bold);
                cursor += content_width + title_gap();
            }
        }

        if !clock.is_empty() {
            let measured = text.measure(clock, point_size, false);
            let x = size.width as i32 - padding() - measured.width as i32;
            let y = (bar_height as i32 - measured.height as i32) / 2;
            // Skip it rather than overlap the menus on a very narrow display.
            if x > cursor {
                text.draw(canvas, Point::new(x, y), clock, MENUBAR_TEXT, point_size, false);
            }
        }
    }

    /// The title under `point` (screen coordinates), if any.
    pub(crate) fn title_at(&self, point: Point) -> Option<usize> {
        if point.y < 0 || point.y >= Self::height() as i32 {
            return None;
        }
        (0..self.title_count).find(|&index| {
            let rect = self.titles[index].rect;
            point.x >= rect.origin.x && point.x < rect.origin.x + rect.size.width as i32
        })
    }

    pub(crate) fn title_kind(&self, index: usize) -> Option<TitleKind> {
        (index < self.title_count).then(|| self.titles[index].kind)
    }

    pub(crate) fn open_title(&self) -> Option<usize> {
        self.open.as_ref().map(|menu| menu.title)
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Whether `point` is over the open menu itself.
    pub(crate) fn menu_contains(&self, point: Point) -> bool {
        self.open.as_ref().is_some_and(|menu| menu.frame.contains(point))
    }

    /// Drop down `items` under title `index`, replacing any menu already
    /// open. The caller redraws the bar (for the title's highlight).
    pub(crate) fn open<T: TextRenderer>(&mut self, index: usize, items: Items, screen: Size, text: &mut T) {
        self.close();

        let slice = items.as_slice();
        if slice.is_empty() || index >= self.title_count {
            return;
        }

        let point_size = title_point_size();
        let mut label_width = 0;
        let mut height = menu_inset() * 2;
        for item in slice {
            height += item.height();
            if !item.is_separator() {
                let width = text.measure(item.label[0], point_size, false).width
                    + text.measure(item.label[1], point_size, false).width;
                label_width = label_width.max(width);
            }
        }
        let width = (check_column() + label_width + pt(28)).max(pt(180)).min(MENU_MAX_WIDTH).min(screen.width);
        let height = height.min(MENU_MAX_HEIGHT);

        let title = self.titles[index].rect;
        let max_x = screen.width as i32 - width as i32;
        let x = title.origin.x.min(max_x).max(0);
        let frame = Rect::new(x, Self::height() as i32, width, height);

        let Some(window) = windowserver::Create_Window(frame, false, pt(6)) else { return; };
        windowserver::Set_Window_Shadow(window, windowserver::Shadow::Popup);
        self.open = Some(OpenMenu { title: index, items, highlighted: None, window, frame });
        self.render(text);
    }

    /// Close the open menu, if any. The caller redraws the bar.
    pub(crate) fn close(&mut self) {
        if let Some(menu) = self.open.take() {
            windowserver::Destroy_Window(menu.window);
        }
    }

    /// Track the pointer over the open menu: highlight the enabled item
    /// under it, or nothing. Returns whether the highlight moved.
    pub(crate) fn hover<T: TextRenderer>(&mut self, point: Point, text: &mut T) -> bool {
        let item = self.item_at(point);
        let Some(menu) = self.open.as_mut() else { return false; };
        if menu.highlighted == item {
            return false;
        }
        menu.highlighted = item;
        self.render(text);
        true
    }

    /// Move the highlight one enabled item up or down (the arrow keys).
    pub(crate) fn step<T: TextRenderer>(&mut self, down: bool, text: &mut T) {
        let Some(menu) = self.open.as_mut() else { return; };
        let items = menu.items.as_slice();
        let count = items.len();
        let mut index = menu.highlighted;
        for _ in 0..count {
            let next = match index {
                None if down => 0,
                None => count - 1,
                Some(current) if down => (current + 1) % count,
                Some(current) => (current + count - 1) % count,
            };
            index = Some(next);
            if items[next].enabled && !items[next].is_separator() {
                menu.highlighted = index;
                self.render(text);
                return;
            }
        }
    }

    /// The action of the highlighted item (Enter, or a release over it).
    pub(crate) fn highlighted_action(&self) -> Option<Action> {
        let menu = self.open.as_ref()?;
        let item = menu.items.as_slice().get(menu.highlighted?)?;
        if item.enabled { item.action } else { None }
    }

    /// The enabled item under `point` (screen coordinates).
    fn item_at(&self, point: Point) -> Option<usize> {
        let menu = self.open.as_ref()?;
        if !menu.frame.contains(point) {
            return None;
        }
        let mut y = menu.frame.origin.y + menu_inset() as i32;
        for (index, item) in menu.items.as_slice().iter().enumerate() {
            let height = item.height() as i32;
            if point.y >= y && point.y < y + height {
                return (item.enabled && !item.is_separator()).then_some(index);
            }
            y += height;
        }
        None
    }

    /// Draw the open menu into its window and hand it to WindowServer.
    fn render<T: TextRenderer>(&self, text: &mut T) {
        let Some(menu) = self.open.as_ref() else { return; };
        let size = menu.frame.size;
        let needed = size.width as usize * size.height as usize;
        let pixels = unsafe { MENU_PIXELS.get_mut() };
        let Some(mut surface) = Surface::new(&mut pixels[..needed], size.width, size.height, size.width) else {
            return;
        };

        let local = Rect::new(0, 0, size.width, size.height);
        let radius = pt(6);
        surface.fill(Color::from_xrgb8888(scene::TRANSPARENT_KEY));
        surface.fill_rounded_rect(local, radius, MENU_BORDER);
        surface.fill_rounded_rect(
            Rect::new(1, 1, size.width.saturating_sub(2), size.height.saturating_sub(2)),
            radius.saturating_sub(1),
            MENU_BACKGROUND,
        );

        let point_size = title_point_size();
        let inset = menu_inset() as i32;
        let mut y = inset;
        for (index, item) in menu.items.as_slice().iter().enumerate() {
            let height = item.height();
            if item.is_separator() {
                let line_y = y + height as i32 / 2;
                surface.fill_rect(Rect::new(inset * 2, line_y, size.width.saturating_sub(inset as u32 * 4), 1), MENU_SEPARATOR);
                y += height as i32;
                continue;
            }

            let highlighted = menu.highlighted == Some(index);
            if highlighted {
                surface.fill_rounded_rect(
                    Rect::new(inset, y, size.width.saturating_sub(inset as u32 * 2), height),
                    pt(4),
                    MENU_HIGHLIGHT,
                );
            }

            let color = if highlighted {
                system_color::ON_ACCENT
            } else if item.enabled {
                system_color::LABEL
            } else {
                system_color::DISABLED_LABEL
            };

            if item.checked {
                draw_check(&mut surface, Point::new(inset + pt(4) as i32, y + height as i32 / 2), color);
            }

            let first = text.measure(item.label[0], point_size, false);
            let text_y = y + (height as i32 - first.height as i32) / 2;
            let x = check_column() as i32;
            text.draw(&mut surface, Point::new(x, text_y), item.label[0], color, point_size, false);
            if !item.label[1].is_empty() {
                text.draw(&mut surface, Point::new(x + first.width as i32, text_y), item.label[1], color, point_size, false);
            }
            y += height as i32;
        }

        windowserver::Render_Window(menu.window, &pixels[..needed], size.width, size.height, size.width);
    }
}

/// A check mark drawn from square dots (no glyph needed from the font):
/// a short stroke down to `origin`'s right, then a long one up.
fn draw_check<C: Canvas>(canvas: &mut C, center: Point, color: Color) {
    let unit = pt(1).max(1) as i32;
    let dot = (unit * 3 / 2).max(1) as u32;
    let start = Point::new(center.x, center.y);
    // Down-right for 3 units, then up-right for 7.
    for step in 0..=3 {
        canvas.fill_rect(Rect::new(start.x + step * unit, start.y + step * unit, dot, dot), color);
    }
    let corner = Point::new(start.x + 3 * unit, start.y + 3 * unit);
    for step in 0..=7 {
        canvas.fill_rect(Rect::new(corner.x + step * unit, corner.y - step * unit, dot, dot), color);
    }
}
