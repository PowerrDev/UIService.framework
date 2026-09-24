#![allow(non_snake_case)]

use ui_core::{Rect, Size};

unsafe extern "C" {
    fn WS_Create_Window(
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        opaque: bool,
        corner_radius: u32,
    ) -> u32;

    fn WS_Create_Background_Window(
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> u32;

    fn WS_Destroy_Window(window_id: u32) -> bool;

    fn WS_Move_Window(window_id: u32, x: i32, y: i32) -> bool;

    fn WS_Set_Size_Limits(
        window_id: u32,
        min_width: u32,
        min_height: u32,
        max_width: u32,
        max_height: u32,
    ) -> bool;

    fn WS_Resize_Window(
        window_id: u32,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> bool;

    fn WS_Focus_Window(window_id: u32) -> bool;

    fn WS_Render_Window(
        window_id: u32,
        pixels: *const u32,
        width: u32,
        height: u32,
        stride: u32,
    ) -> bool;

    fn WS_Pointer_Move(x: i32, y: i32) -> bool;

    fn WS_Set_Cursor(
        kind: u32,
        pixels: *const u32,
        width: u32,
        height: u32,
        stride: u32,
        hotspot_x: i32,
        hotspot_y: i32,
    ) -> bool;

    fn WS_Present() -> bool;

    fn WS_Set_Cursor_Kind(kind: u32) -> bool;
}

/// What the pointer shows -- mirrors `windowserver::CursorKind` numerically
/// (`WS_Set_Cursor`/`WS_Set_Cursor_Kind`'s own contract; there is no direct
/// Rust dependency between these two frameworks, only the shared FFI
/// encoding -- see `cursor_kind_to_raw` below).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CursorKind {
    Arrow,
    Move,
    Hand,
    NotAllowed,
    ResizeHorizontal,
    ResizeVertical,
    ResizeDiagonalNeSw,
    ResizeDiagonalNwSe,
    Text,
}

fn cursor_kind_to_raw(kind: CursorKind) -> u32 {
    match kind {
        CursorKind::Arrow => 0,
        CursorKind::Move => 1,
        CursorKind::Hand => 2,
        CursorKind::NotAllowed => 3,
        CursorKind::ResizeHorizontal => 4,
        CursorKind::ResizeVertical => 5,
        CursorKind::ResizeDiagonalNeSw => 6,
        CursorKind::ResizeDiagonalNwSe => 7,
        CursorKind::Text => 8,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowId(u32);

impl WindowId {
    pub const fn get(self) -> u32 { self.0 }
}

pub(crate) fn Create_Window(frame: Rect, opaque: bool, corner_radius: u32) -> Option<WindowId> {
    let id = unsafe {
        WS_Create_Window(
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
            opaque,
            corner_radius,
        )
    };
    (id != 0).then_some(WindowId(id))
}

pub(crate) fn Create_Background_Window(frame: Rect) -> Option<WindowId> {
    let id = unsafe {
        WS_Create_Background_Window(
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
        )
    };
    (id != 0).then_some(WindowId(id))
}

pub(crate) fn Destroy_Window(id: WindowId) -> bool {
    unsafe { WS_Destroy_Window(id.get()) }
}

pub(crate) fn Move_Window(id: WindowId, frame: Rect) -> bool {
    unsafe { WS_Move_Window(id.get(), frame.origin.x, frame.origin.y) }
}

pub(crate) fn Set_Size_Limits(id: WindowId, min_size: Size, max_size: Size) -> bool {
    unsafe {
        WS_Set_Size_Limits(
            id.get(),
            min_size.width,
            min_size.height,
            max_size.width,
            max_size.height,
        )
    }
}

pub(crate) fn Resize_Window(id: WindowId, frame: Rect) -> bool {
    unsafe {
        WS_Resize_Window(
            id.get(),
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
        )
    }
}

pub(crate) fn Focus_Window(id: WindowId) -> bool {
    unsafe { WS_Focus_Window(id.get()) }
}

pub(crate) fn Pointer_Move(x: i32, y: i32) -> bool {
    unsafe { WS_Pointer_Move(x, y) }
}

pub(crate) fn Render_Window(
    id: WindowId,
    pixels: &[u32],
    width: u32,
    height: u32,
    stride: u32,
) -> bool {
    unsafe {
        WS_Render_Window(
            id.get(),
            pixels.as_ptr(),
            width,
            height,
            stride,
        )
    }
}

pub(crate) fn Present() -> bool {
    unsafe { WS_Present() }
}

pub(crate) fn Set_Cursor_Kind(kind: CursorKind) -> bool {
    unsafe { WS_Set_Cursor_Kind(cursor_kind_to_raw(kind)) }
}

pub(crate) fn Set_Cursor(
    kind: CursorKind,
    pixels: &[u32],
    width: u32,
    height: u32,
    stride: u32,
    hotspot: ui_core::Point,
) -> bool {
    unsafe {
        WS_Set_Cursor(
            cursor_kind_to_raw(kind),
            pixels.as_ptr(),
            width,
            height,
            stride,
            hotspot.x,
            hotspot.y,
        )
    }
}
