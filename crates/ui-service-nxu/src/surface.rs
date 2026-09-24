use core::slice;

use ui_render::Surface;

pub(crate) unsafe fn from_raw<'a>(
    pixels: *mut u32,
    width: u32,
    height: u32,
    stride: u32,
) -> Option<Surface<'a>> {
    if pixels.is_null() || width == 0 || height == 0 || stride < width {
        return None;
    }

    let pixel_count = (stride as usize).checked_mul(height as usize)?;
    let pixels = unsafe { slice::from_raw_parts_mut(pixels, pixel_count) };
    Surface::new(pixels, width, height, stride)
}
