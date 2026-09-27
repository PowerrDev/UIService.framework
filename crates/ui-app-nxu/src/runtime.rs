//! Running a `ui_app::App` as a process: connect to the desktop with the
//! app's name, window and menus, then turn the desktop's messages into the
//! app's events, draws and ticks, and send back each frame and the app's
//! state (its pointer, its menu items, whether it animates).
//!
//! Messages are handled in bursts: everything already queued is applied
//! before one redraw, so a fast pointer never makes the app draw frames
//! nobody will see.

use ui_app::{App, AppAction, CursorKind, Frame, MenuItem, MenuItemState};
use ui_core::{scale, Event, Point, PointerButton, Rect, Size};
use ui_render::{CanvasView, Surface};
use ui_session::{self as session, Connect, Message, Session, Submit};

use crate::connection::{self, Connection, Received};
use crate::{backends, sys, text};

/// Fill in what the desktop needs to know about `A` to put up its window.
fn describe<A: App>(bundle_path: &str, desktop: &Session) -> Connect {
    let mut info = Connect::empty(session::KIND_APP);
    session::set_str(&mut info.name, A::INFO.name);
    session::set_str(&mut info.bundle_path, bundle_path);

    let size = A::WINDOW.effective_size();
    let min = A::WINDOW.effective_min_size();
    let mut max = A::WINDOW.effective_max_size();
    if A::WINDOW.resizable && A::WINDOW.max_size.is_none() {
        // No limit given: the window can grow, by half again its opening
        // size, within the screen below the menu bar. Not the whole screen
        // by default: the kernel keeps a few copies of a window at its
        // largest size (the frames in flight, the desktop's backing store,
        // WindowServer's), and a screen's worth each at 2x does not fit
        // the kernel's memory with a couple of apps open on i386.
        max = Size::new(
            (size.width + size.width / 2).min(desktop.screen_width).max(size.width),
            (size.height + size.height / 2).min(desktop.screen_height.saturating_sub(desktop.menubar_height)).max(size.height),
        );
    }
    let style = A::WINDOW.effective_style();
    info.width = size.width;
    info.height = size.height;
    info.min_width = min.width.min(size.width);
    info.min_height = min.height.min(size.height);
    info.max_width = max.width.max(size.width);
    info.max_height = max.height.max(size.height);
    info.resizable = A::WINDOW.resizable as u32;
    info.titlebar_height = style.titlebar_height;
    info.corner_radius = style.corner_radius;

    let mut items = 0usize;
    for (menu_index, menu) in A::MENUS.iter().enumerate().take(session::MENU_MAX) {
        session::set_str(&mut info.menu_titles[menu_index], menu.title);
        info.menu_count = menu_index as u32 + 1;
        for item in menu.items {
            if items == session::MENU_ITEM_MAX {
                break;
            }
            let slot = &mut info.items[items];
            slot.menu = menu_index as u32;
            match *item {
                MenuItem::Command { title, command } => {
                    slot.command = command;
                    session::set_str(&mut slot.title, title);
                }
                MenuItem::Separator => slot.command = session::MENU_SEPARATOR,
            }
            items += 1;
        }
    }
    info.item_count = items as u32;
    info
}

fn event_from(message: &Message) -> Option<Event> {
    let position = Point::new(message.x, message.y);
    let button = match message.button {
        session::BUTTON_SECONDARY => PointerButton::Secondary,
        session::BUTTON_MIDDLE => PointerButton::Middle,
        _ => PointerButton::Primary,
    };
    Some(match message.event {
        session::EVENT_POINTER_MOVED => Event::PointerMoved { position },
        session::EVENT_POINTER_DOWN => Event::PointerDown { position, button },
        session::EVENT_POINTER_UP => Event::PointerUp { position, button },
        session::EVENT_POINTER_LEFT => Event::PointerLeft,
        session::EVENT_SCROLL => Event::Scroll { position, delta: message.delta },
        session::EVENT_KEY_DOWN => Event::KeyDown {
            code: message.button,
            character: char::from_u32(message.character).filter(|&character| character != '\0'),
        },
        _ => return None,
    })
}

fn cursor_code(kind: Option<CursorKind>) -> u32 {
    match kind {
        Some(CursorKind::Hand) => session::CURSOR_HAND,
        Some(CursorKind::NotAllowed) => session::CURSOR_NOT_ALLOWED,
        Some(CursorKind::Arrow) | None => session::CURSOR_ARROW,
    }
}

fn menu_state_bits(state: MenuItemState) -> u8 {
    (if state.enabled { session::MENU_ENABLED } else { 0 }) | (if state.checked { session::MENU_CHECKED } else { 0 })
}

/// Run `A` until the user quits it. `bundle_path` is where the app is
/// installed (`/Applications/Voyager.app`). Returns the process's exit status.
pub fn run<A: App>(bundle_path: &str, make: fn() -> A) -> i32 {
    sys::log_line(&[A::INFO.name, ": starting"]);
    let Some(desktop) = connection::wait_for_desktop() else {
        sys::log_line(&[A::INFO.name, ": no desktop to show a window on"]);
        return 1;
    };
    scale::set_permille(desktop.scale_permille);
    backends::install();

    let info = describe::<A>(bundle_path, &desktop);
    let connection = match Connection::open(&info) {
        Ok(connection) => connection,
        Err(_) => {
            sys::log_line(&[A::INFO.name, ": the desktop refused the window"]);
            return 2;
        }
    };

    // The content, at most the window's largest size: private memory the
    // desktop copies each frame out of.
    let capacity = info.max_width as usize * info.max_height as usize;
    let Some(bytes) = sys::map(capacity * 4) else {
        sys::log_line(&[A::INFO.name, ": no memory for the window"]);
        return 3;
    };
    let pixels = unsafe { core::slice::from_raw_parts_mut(bytes.as_mut_ptr() as *mut u32, capacity) };

    let mut text = text::load();
    let mut app = make();
    let mut content = Size::new(0, 0);
    // Item index -> command, for the menu states sent with every submit.
    let commands: [u32; session::MENU_ITEM_MAX] = core::array::from_fn(|index| info.items[index].command);

    sys::log_line(&[A::INFO.name, ": connected, window up"]);

    loop {
        let mut redraw = false;
        let mut close = false;
        let mut wait = connection::WAIT_FOREVER;

        // Everything queued now, then one frame.
        loop {
            let message = match connection.receive(wait) {
                Received::Message(message) => message,
                Received::Empty => break,
                Received::Closed => return 0,
            };
            wait = 0;

            let size = Size::new(message.width, message.height);
            let action = match message.kind {
                session::MSG_REDRAW => {
                    if size != content {
                        content = size;
                        // A new stride: start from the window's background,
                        // not the old frame's rows.
                        let count = (size.width as usize * size.height as usize).min(pixels.len());
                        let background = A::WINDOW.style.background.to_xrgb8888();
                        pixels[..count].fill(background);
                    }
                    AppAction::Redraw
                }
                session::MSG_EVENT => match event_from(&message) {
                    Some(event) => app.event(event, size),
                    None => AppAction::None,
                },
                session::MSG_MENU_COMMAND => app.menu_command(message.command),
                session::MSG_FRAME => {
                    if app.animating() { app.tick(sys::uptime_us()) } else { AppAction::None }
                }
                session::MSG_QUIT => return 0,
                _ => AppAction::None,
            };
            match action {
                AppAction::Redraw => redraw = true,
                AppAction::Close => close = true,
                AppAction::None => {}
            }
        }

        let mut submit = Submit::empty();
        submit.cursor = cursor_code(app.cursor_kind());
        submit.flags = session::SUBMIT_MENU_STATE;
        if app.animating() {
            submit.flags |= session::SUBMIT_ANIMATING;
        }
        if close {
            submit.action = session::ACTION_CLOSE;
        }
        for (index, &command) in commands.iter().enumerate().take(info.item_count as usize) {
            if command != session::MENU_SEPARATOR {
                submit.menu_state[index] = menu_state_bits(app.menu_item_state(command));
            }
        }

        if redraw && content.width > 0 && content.height > 0 {
            let count = content.width as usize * content.height as usize;
            if count <= pixels.len() {
                if let Some(mut surface) = Surface::new(&mut pixels[..count], content.width, content.height, content.width) {
                    let mut canvas = CanvasView::new(&mut surface, Rect::new(0, 0, content.width, content.height));
                    let mut frame = Frame::new(&mut canvas, &mut text);
                    app.draw(&mut frame);
                }
                submit.pixels = pixels.as_ptr() as usize as u64;
                submit.width = content.width;
                submit.height = content.height;
                submit.stride = content.width;
            }
        }

        connection.submit(&submit);
    }
}
