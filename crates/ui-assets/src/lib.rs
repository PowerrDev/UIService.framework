#![no_std]

//! UIService resource decoders, including classic cursor assets.

use ui_core::{Color, Point, Size};
use ui_render::Canvas;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssetError {
    InvalidData,
    Unsupported,
}

#[derive(Clone, Copy)]
pub struct CursorImage<'a> {
    data: &'a [u8],
    pixel_offset: usize,
    width: u32,
    height: u32,
    row_stride: usize,
    hotspot: Point,
    bottom_up: bool,
}

impl<'a> CursorImage<'a> {
    pub fn from_cur(data: &'a [u8], preferred_size: u32) -> Result<Self, AssetError> {
        if read_u16(data, 0)? != 0 || read_u16(data, 2)? != 2 {
            return Err(AssetError::InvalidData);
        }

        let count = read_u16(data, 4)? as usize;
        if count == 0 {
            return Err(AssetError::InvalidData);
        }

        let mut best: Option<(u32, usize)> = None;
        for index in 0..count {
            let entry = 6 + index * 16;
            if entry + 16 > data.len() {
                return Err(AssetError::InvalidData);
            }

            let width = match data[entry] {
                0 => 256,
                value => value as u32,
            };
            let height = match data[entry + 1] {
                0 => 256,
                value => value as u32,
            };
            if width != height {
                continue;
            }

            let distance = width.abs_diff(preferred_size);
            match best {
                Some((best_distance, _)) if best_distance <= distance => {}
                _ => best = Some((distance, entry)),
            }
        }

        let (_, entry) = best.ok_or(AssetError::Unsupported)?;
        Self::from_entry(data, entry)
    }

    fn from_entry(data: &'a [u8], entry: usize) -> Result<Self, AssetError> {
        let directory_width = match data[entry] {
            0 => 256,
            value => value as u32,
        };
        let directory_height = match data[entry + 1] {
            0 => 256,
            value => value as u32,
        };
        let hotspot = Point::new(
            read_u16(data, entry + 4)? as i32,
            read_u16(data, entry + 6)? as i32,
        );
        let bytes_in_resource = read_u32(data, entry + 8)? as usize;
        let image_offset = read_u32(data, entry + 12)? as usize;
        let image_end = image_offset
            .checked_add(bytes_in_resource)
            .ok_or(AssetError::InvalidData)?;
        if image_end > data.len() || image_offset + 40 > image_end {
            return Err(AssetError::InvalidData);
        }

        if matches!(
            data.get(image_offset..image_offset + 8),
            Some(signature) if signature == b"\x89PNG\r\n\x1a\n"
        ) {
            return Err(AssetError::Unsupported);
        }

        let header_size = read_u32(data, image_offset)? as usize;
        if header_size < 40 || image_offset + header_size > image_end {
            return Err(AssetError::Unsupported);
        }

        let dib_width = read_i32(data, image_offset + 4)?;
        let dib_height = read_i32(data, image_offset + 8)?;
        let bit_count = read_u16(data, image_offset + 14)?;
        let compression = read_u32(data, image_offset + 16)?;
        if dib_width == 0 || dib_height == 0 || bit_count != 32 || compression != 0 {
            return Err(AssetError::Unsupported);
        }

        let width = dib_width.unsigned_abs();
        let stored_height = dib_height.unsigned_abs();
        let height = if stored_height >= directory_height.saturating_mul(2) {
            stored_height / 2
        } else {
            directory_height
        };
        if width != directory_width || height != directory_height {
            return Err(AssetError::InvalidData);
        }

        let row_stride = width as usize * 4;
        let pixel_offset = image_offset + header_size;
        let pixel_bytes = row_stride
            .checked_mul(height as usize)
            .ok_or(AssetError::InvalidData)?;
        if pixel_offset + pixel_bytes > image_end {
            return Err(AssetError::InvalidData);
        }

        Ok(Self {
            data,
            pixel_offset,
            width,
            height,
            row_stride,
            hotspot,
            bottom_up: dib_height > 0,
        })
    }

    pub const fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }

    pub const fn hotspot(&self) -> Point {
        self.hotspot
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<Color> {
        if x >= self.width || y >= self.height {
            return None;
        }

        let source_y = if self.bottom_up {
            self.height - 1 - y
        } else {
            y
        };
        let offset = self.pixel_offset
            + source_y as usize * self.row_stride
            + x as usize * 4;
        let blue = *self.data.get(offset)?;
        let green = *self.data.get(offset + 1)?;
        let red = *self.data.get(offset + 2)?;
        let alpha = *self.data.get(offset + 3)?;
        Some(Color::rgba(red, green, blue, alpha))
    }

    pub fn draw<C: Canvas + ?Sized>(&self, canvas: &mut C, pointer_position: Point) {
        let origin = Point::new(
            pointer_position.x - self.hotspot.x,
            pointer_position.y - self.hotspot.y,
        );

        for y in 0..self.height {
            for x in 0..self.width {
                let Some(color) = self.pixel(x, y) else {
                    continue;
                };
                if color.alpha == 0 {
                    continue;
                }

                canvas.blend_pixel(
                    Point::new(origin.x + x as i32, origin.y + y as i32),
                    color,
                );
            }
        }
    }
}

fn read_u16(data: &[u8], offset: usize) -> Result<u16, AssetError> {
    let bytes = data.get(offset..offset + 2).ok_or(AssetError::InvalidData)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32, AssetError> {
    let bytes = data.get(offset..offset + 4).ok_or(AssetError::InvalidData)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_i32(data: &[u8], offset: usize) -> Result<i32, AssetError> {
    Ok(read_u32(data, offset)? as i32)
}
