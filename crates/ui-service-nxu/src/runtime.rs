use ui_abi::{HostV5, Status};
use ui_app::{App, AppAction, Frame};
use ui_core::{Event, Point, Size};
use ui_platform::InteractiveHostV5;
use ui_render::{Canvas, CanvasView, Surface};
use ui_window::{ResizeEdges, Window, WindowStyle};

use crate::{aqua, clock, scene, windowserver};
use crate::storage::{StaticCell, TEXT_SCRATCH};
use crate::text;
use crate::windowserver::CursorKind;

/// What the window chrome itself (dragging, resizing, the titlebar) wants the
/// pointer to show, decided from `window`'s own state and, for a plain hover,
/// where `position` is -- see the call site in `run`'s event loop. Content
/// inside the window (a button, a link, something disabled) gets the last
/// word: it runs its own hit-testing and calls `windowserver::Set_Cursor_Kind`
/// itself, afterward, so this only ever sets `Arrow` as a hover default for
/// the content case, never overriding what the content just asked for.
/// The directional resize cursor `edges` calls for, or `None` for
/// `ResizeEdges::none()` (not near, or not on, a resizable edge). A corner
/// (two edges at once) picks the diagonal its two edges actually lie on:
/// top+right or bottom+left is the "/" diagonal (`ResizeDiagonalNeSw`),
/// top+left or bottom+right is the "\" one (`ResizeDiagonalNwSe`).
fn resize_cursor_kind(edges: ResizeEdges) -> Option<CursorKind> {
    if edges.is_none() {
        return None;
    }

    let horizontal = edges.left || edges.right;
    let vertical = edges.top || edges.bottom;

    Some(if horizontal && vertical {
        let ne_sw_diagonal = (edges.top && edges.right) || (edges.bottom && edges.left);
        if ne_sw_diagonal { CursorKind::ResizeDiagonalNeSw } else { CursorKind::ResizeDiagonalNwSe }
    } else if horizontal {
        CursorKind::ResizeHorizontal
    } else {
        CursorKind::ResizeVertical
    })
}

fn chrome_cursor_kind(window: &Window, event: Event) -> Option<CursorKind> {
    if window.is_dragging() {
        return Some(CursorKind::Move);
    }
    // An active resize keeps the cursor matching the edge(s) the drag
    // actually started on, not wherever the pointer has since wandered to
    // (see resizing_edges's own doc comment).
    if let Some(kind) = resize_cursor_kind(window.resizing_edges()) {
        return Some(kind);
    }

    let Event::PointerMoved { position } = event else { return None; };

    if window.titlebar_rect().contains(position) {
        Some(CursorKind::Hand)
    } else if let Some(kind) = resize_cursor_kind(window.resize_edges_at(position)) {
        Some(kind)
    } else {
        Some(CursorKind::Arrow)
    }
}

fn localized_event(event: Event, window: Window, suppress: bool) -> Option<Event> {
    let content = window.content_rect();

    match event {
        Event::PointerDown { position, .. } if window.titlebar_rect().contains(position) => None,
        Event::PointerDown { position, .. } if !window.resize_edges_at(position).is_none() => None,
        Event::PointerMoved { .. } if suppress => None,
        Event::PointerUp { .. } if suppress => None,
        Event::PointerMoved { position } => Some(Event::PointerMoved {
            position: Point::new(position.x - content.origin.x, position.y - content.origin.y),
        }),
        Event::PointerDown { position, button } => Some(Event::PointerDown {
            position: Point::new(position.x - content.origin.x, position.y - content.origin.y),
            button,
        }),
        Event::PointerUp { position, button } => Some(Event::PointerUp {
            position: Point::new(position.x - content.origin.x, position.y - content.origin.y),
            button,
        }),
        Event::PointerLeft => Some(Event::PointerLeft),
        // Only a wheel turned over the content scrolls the app: over the
        // titlebar, the border or the desktop it means nothing to it, and
        // while the window is being dragged or resized it must not reach it.
        Event::Scroll { position, delta } => {
            if suppress || !content.contains(position) {
                None
            } else {
                Some(Event::Scroll {
                    position: Point::new(position.x - content.origin.x, position.y - content.origin.y),
                    delta,
                })
            }
        }
    }
}

unsafe extern "C" {
    /// NXU's monotonic microsecond counter: the same kernel symbol
    /// WindowServer's own NXU bridge uses for frame pacing and its cursor
    /// animation, declared directly for the same reason (one final kernel
    /// image, so this is same-build coupling, not a new cross-framework
    /// contract).
    fn timer_get_microseconds() -> u64;
}

/// See `WindowConfig::effective_size`/`effective_style`: `A::WINDOW` is a
/// fixed 2x-density reference that also sizes this app's static backing
/// store, so the window actually drawn at runtime is that reference
/// rescaled to the host's real content scale.
fn effective_window<A: App>() -> (Size, WindowStyle) {
    let reference = A::WINDOW;
    (reference.effective_size(), reference.effective_style())
}

/// `full_repaint`: whether to clear the whole window buffer to the
/// transparent key and redraw the chrome (title, traffic lights, border)
/// before calling `app.draw()`. Chrome only depends on `size`/`style`, both
/// fixed between resizes, and any full-content repaint an app does already
/// fills its own content area (see `App::draw`'s contract) -- so this only
/// needs to happen on the very first build and again whenever `size`
/// actually changes. Skipping it on every other redraw is also what makes
/// an app's *partial* repaints (an app drawing only the one row/control
/// that actually changed, instead of its whole content area, to skip
/// needless work) actually work: re-clearing the whole buffer first would
/// wipe everything the app didn't touch back to the transparent key, which
/// then reads as "outside the window" and shows the desktop through it.
unsafe fn build_window<A: App, const N: usize>(
    app: &mut A,
    backing: &StaticCell<[u32; N]>,
    size: Size,
    style: WindowStyle,
    full_repaint: bool,
) -> Result<(), Status> {
    let needed = size.width as usize * size.height as usize;
    if needed == 0 || needed > N || size.width == 0 || size.height == 0 {
        return Err(Status::NoSurface);
    }

    let pixels = unsafe { backing.get_mut() };
    let Some(mut surface) = Surface::new(
        &mut pixels[..needed],
        size.width,
        size.height,
        size.width,
    ) else {
        return Err(Status::NoSurface);
    };

    let local_window = Window::new(
        ui_core::Rect::new(0, 0, size.width, size.height),
        style,
    );

    let scratch = unsafe { TEXT_SCRATCH.get_mut() };
    let mut text = text::backend(scratch);

    if full_repaint {
        surface.fill(ui_core::Color::from_xrgb8888(scene::TRANSPARENT_KEY));
        aqua::UIDrawWindow(&mut surface, &mut text, &local_window, A::INFO.name);
    }

    let content = local_window.local_content_rect();
    let mut canvas = CanvasView::new(&mut surface, content);
    let mut frame = Frame::new(&mut canvas, &mut text);
    app.draw(&mut frame);

    // The app drew into a plain rectangle; give the window its rounded bottom
    // corners and side/bottom border back.
    local_window.draw_content_mask(&mut surface, ui_core::Color::from_xrgb8888(scene::TRANSPARENT_KEY), true);
    Ok(())
}

/// Draw the desktop (background + menu bar) and submit it as the bottom-most
/// WindowServer surface. `app_name` is the menu bar's `<AppName>`; the menu
/// bar's `<OtherInfo>` clock comes from the host's RTC when it reports one
/// (`UI_SERVICE_HOST_CAP_TIME`) and is left blank otherwise.
unsafe fn redraw_desktop(
    host: &mut InteractiveHostV5<'_>,
    desktop_id: windowserver::WindowId,
    app_name: &str,
) -> Result<(), Status> {
    let mut clock_buffer = [0u8; 32];
    let other_info: &str = match host.get_time() {
        Ok(Some(unix_seconds)) => clock::format(unix_seconds, &mut clock_buffer),
        _ => "",
    };

    let scratch = unsafe { TEXT_SCRATCH.get_mut() };
    let mut text = text::backend(scratch);
    let desktop_result = unsafe {
        host.with_surface(|surface| {
            scene::UIDrawDesktop(surface, &mut text, app_name, other_info);
            (surface.pixels_mut().as_ptr(), surface.width(), surface.height(), surface.stride())
        })?
    };

    if !windowserver::Render_Window(
        desktop_id,
        unsafe { core::slice::from_raw_parts(desktop_result.0, desktop_result.2 as usize * desktop_result.3 as usize) },
        desktop_result.1,
        desktop_result.2,
        desktop_result.3,
    ) {
        return Err(Status::NoSurface);
    }

    Ok(())
}

/// Run one statically linked application through the early NXU UIService host.
///
/// This is deliberately generic even though NXU currently launches only About
/// sevOS. Later userspace runtimes can implement the same `App` contract
/// without inheriting this early-boot host bridge.
///
/// Takes a constructor (`make_app`) rather than an already-built `A`, and
/// calls it only *after* connecting: connecting is what wires up
/// `ui_core::scale`/`ui_core::fs` from the host ABI, and an app's own `new`
/// can reasonably depend on either already being set (Voyager's does --
/// `Navigator::new` eagerly lists its starting directory, which needs
/// `ui_core::fs` connected or it would always come back empty).
pub(crate) unsafe fn run<A: App, const N: usize>(
    host_abi: &HostV5,
    make_app: impl FnOnce() -> A,
    backing: &StaticCell<[u32; N]>,
) -> Result<(), Status> {
    let mut host = InteractiveHostV5::connect(host_abi)?;
    let mut app = make_app();
    let app = &mut app;
    let surface_size = unsafe { host.with_surface(|surface| surface.size()) }?;
    let (window_size, window_style) = effective_window::<A>();
    let mut window_size = window_size;
    let mut window_pixel_count = window_size.width as usize * window_size.height as usize;
    let min_size = A::WINDOW.effective_min_size();
    let max_size = A::WINDOW.effective_max_size();
    let mut window = scene::centered_window(surface_size, window_size, window_style);
    if A::WINDOW.resizable {
        window = window.with_resize_limits(min_size, max_size);
    }
    let pointer = Point::new(
        (surface_size.width / 2) as i32,
        (surface_size.height / 2) as i32,
    );

    let desktop_id = windowserver::Create_Background_Window(
        ui_core::Rect::new(0, 0, surface_size.width, surface_size.height),
    ).ok_or(Status::NoSurface)?;

    let window_id = windowserver::Create_Window(window.frame(), false, window.corner_radius())
        .ok_or_else(|| {
            windowserver::Destroy_Window(desktop_id);
            Status::NoSurface
        })?;

    if A::WINDOW.resizable {
        windowserver::Set_Size_Limits(window_id, min_size, max_size);
    }

    unsafe { build_window(app, backing, window_size, window_style, true)? };

    if unsafe { redraw_desktop(&mut host, desktop_id, A::INFO.name) }.is_err() {
        windowserver::Destroy_Window(window_id);
        windowserver::Destroy_Window(desktop_id);
        return Err(Status::NoSurface);
    }

    let pixels = unsafe { backing.get_mut() };
    if !windowserver::Render_Window(
        window_id,
        &pixels[..window_pixel_count],
        window_size.width,
        window_size.height,
        window_size.width,
    ) {
        windowserver::Destroy_Window(window_id);
        windowserver::Destroy_Window(desktop_id);
        return Err(Status::NoSurface);
    }

    let _ = windowserver::Focus_Window(window_id);
    crate::cursor::install_system_cursors();
    if !windowserver::Pointer_Move(pointer.x, pointer.y) {
        windowserver::Destroy_Window(window_id);
        windowserver::Destroy_Window(desktop_id);
        return Err(Status::PresentFailed);
    }
    if !windowserver::Present() {
        windowserver::Destroy_Window(window_id);
        windowserver::Destroy_Window(desktop_id);
        return Err(Status::PresentFailed);
    }

    let mut last_clock_minute = host.get_time().ok().flatten().map(|seconds| seconds / 60);

    loop {
        let old_window = window.frame();
        let mut dirty = false;
        let mut redraw_window = false;

        let current_clock_minute = host.get_time().ok().flatten().map(|seconds| seconds / 60);
        if current_clock_minute != last_clock_minute {
            last_clock_minute = current_clock_minute;
            if unsafe { redraw_desktop(&mut host, desktop_id, A::INFO.name) }.is_err() {
                windowserver::Destroy_Window(window_id);
                windowserver::Destroy_Window(desktop_id);
                return Err(Status::PresentFailed);
            }
            dirty = true;
        }

        loop {
            let event = match host.poll_event()? {
                Some(event) => event,
                None => break,
            };

            match event {
                Event::PointerMoved { position }
                | Event::PointerDown { position, .. }
                | Event::PointerUp { position, .. } => {
                    dirty = true;
                }
                Event::PointerLeft | Event::Scroll { .. } => {}
            }

            let suppress_content = window.is_dragging() || window.is_resizing();
            if window.handle_event(event, surface_size) {
                dirty = true;
            }

            // Chrome (titlebar, drag, resize) first; it returns plain Arrow
            // when the pointer is just over content, which the app's own
            // cursor_kind() below is free to override -- see both doc
            // comments for the full handoff.
            let chrome_kind = chrome_cursor_kind(&window, event);
            let over_content = chrome_kind == Some(CursorKind::Arrow);
            if let Some(kind) = chrome_kind {
                windowserver::Set_Cursor_Kind(kind);
            }

            if let Some(event) = localized_event(event, window, suppress_content) {
                let content_size = window.local_content_rect().size;
                match app.event(event, content_size) {
                    AppAction::None => {}
                    AppAction::Redraw => {
                        redraw_window = true;
                        dirty = true;
                    }
                    AppAction::Close => {
                        windowserver::Destroy_Window(window_id);
                        windowserver::Destroy_Window(desktop_id);
                        return Ok(());
                    }
                }

                if over_content {
                    windowserver::Set_Cursor_Kind(match app.cursor_kind() {
                        Some(ui_app::CursorKind::Hand) => CursorKind::Hand,
                        Some(ui_app::CursorKind::NotAllowed) => CursorKind::NotAllowed,
                        Some(ui_app::CursorKind::Arrow) | None => CursorKind::Arrow,
                    });
                }
            }
        }

        // Animations run on the real clock, once per frame, and only while the
        // app says it has one going: an idle app costs nothing here.
        if app.animating() {
            match app.tick(unsafe { timer_get_microseconds() }) {
                AppAction::None => {}
                AppAction::Redraw => {
                    redraw_window = true;
                    dirty = true;
                }
                AppAction::Close => {
                    windowserver::Destroy_Window(window_id);
                    windowserver::Destroy_Window(desktop_id);
                    return Ok(());
                }
            }
        }

        let mut resized = false;
        if window.frame() != old_window {
            if window.frame().size != old_window.size {
                // A resize also needs a fresh content buffer at the new
                // dimensions -- the window's own pixel storage on the
                // WindowServer side gets reallocated (and starts blank) the
                // moment this call lands, so the rebuild+render below can't
                // be skipped or deferred the way a plain move's can.
                if !windowserver::Resize_Window(window_id, window.frame()) {
                    windowserver::Destroy_Window(window_id);
                    windowserver::Destroy_Window(desktop_id);
                    return Err(Status::PresentFailed);
                }
                window_size = window.frame().size;
                window_pixel_count = window_size.width as usize * window_size.height as usize;
                resized = true;
                redraw_window = true;
            } else if !windowserver::Move_Window(window_id, window.frame()) {
                windowserver::Destroy_Window(window_id);
                windowserver::Destroy_Window(desktop_id);
                return Err(Status::PresentFailed);
            }
        }

        if redraw_window {
            unsafe { build_window(app, backing, window_size, window_style, resized)? };
            let pixels = unsafe { backing.get_mut() };
            if !windowserver::Render_Window(
                window_id,
                &pixels[..window_pixel_count],
                window_size.width,
                window_size.height,
                window_size.width,
            ) {
                windowserver::Destroy_Window(window_id);
                windowserver::Destroy_Window(desktop_id);
                return Err(Status::PresentFailed);
            }
        }

        // Always present, not just when something we know about changed:
        // WindowServer's cursor can be mid-animation (shake-to-grow easing
        // back down, see WindowServer.framework's animation.rs) entirely on
        // its own clock, and that only advances when WS_Present actually
        // runs. Gating this on `dirty` would freeze that animation solid the
        // instant the pointer stops moving instead of letting it settle on
        // its own like a real system's cursor does. A `false` return here
        // most commonly just means "nothing was actually damaged" (the
        // common idle case), which is only a real failure when we know we
        // handed it work to do.
        let presented = windowserver::Present();
        if dirty && !presented {
            windowserver::Destroy_Window(window_id);
            windowserver::Destroy_Window(desktop_id);
            return Err(Status::PresentFailed);
        }

        core::hint::spin_loop();
    }
}
