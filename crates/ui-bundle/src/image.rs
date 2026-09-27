//! App icons: pick the right image out of an `.icns` file, decode it (every
//! modern `.icns` image is a PNG), and scale it to the Dock's tile size.
//!
//! Nothing here allocates: the caller hands in the buffers (the Dock maps
//! them once), and [`decoded_size`] says how big they must be.

use miniz_oxide::inflate::core::{decompress, inflate_flags, DecompressorOxide};
use miniz_oxide::inflate::TINFLStatus;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    NotIcns,
    NoImage,
    NotPng,
    Unsupported,
    Corrupt,
    BufferTooSmall,
}

/// The `.icns` image types that hold PNG data, and their pixel size.
const ICNS_PNG_TYPES: [(&[u8; 4], u32); 8] = [
    (b"ic11", 32),
    (b"ic12", 64),
    (b"ic07", 128),
    (b"ic13", 256),
    (b"ic08", 256),
    (b"ic14", 512),
    (b"ic09", 512),
    (b"ic10", 1024),
];

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

fn be32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// The PNG in `icns` best suited to a `target` pixel tile: the smallest one
/// at least that big, or else the biggest there is.
pub fn icns_best_png(icns: &[u8], target: u32) -> Result<&[u8], Error> {
    if icns.len() < 8 || &icns[..4] != b"icns" {
        return Err(Error::NotIcns);
    }
    let total = (be32(&icns[4..8]) as usize).min(icns.len());
    let mut best: Option<(&[u8], u32)> = None;
    let mut at = 8;
    while at + 8 <= total {
        let kind = &icns[at..at + 4];
        let length = be32(&icns[at + 4..at + 8]) as usize;
        if length < 8 || at + length > total {
            return Err(Error::NotIcns);
        }
        let data = &icns[at + 8..at + length];
        if let Some(&(_, size)) = ICNS_PNG_TYPES.iter().find(|(name, _)| kind == &name[..]) {
            if data.starts_with(&PNG_SIGNATURE) {
                let better = match best {
                    None => true,
                    Some((_, current)) if current < target => size > current,
                    Some((_, current)) => size >= target && size < current,
                };
                if better {
                    best = Some((data, size));
                }
            }
        }
        at += length;
    }
    best.map(|(data, _)| data).ok_or(Error::NoImage)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PngHeader {
    pub width: u32,
    pub height: u32,
    color_type: u8,
    bit_depth: u8,
    interlace: u8,
}

impl PngHeader {
    fn channels(&self) -> usize {
        match self.color_type {
            0 => 1,
            2 => 3,
            3 => 1,
            4 => 2,
            6 => 4,
            _ => 0,
        }
    }

    /// Bytes of inflated scanlines (a filter byte per row).
    pub fn raw_size(&self) -> usize {
        self.height as usize * (1 + self.width as usize * self.channels())
    }

    /// Bytes of the decoded RGBA image.
    pub fn rgba_size(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }
}

/// Walk a PNG's chunks.
fn chunks(png: &[u8]) -> impl Iterator<Item = (&[u8], &[u8])> {
    let mut at = PNG_SIGNATURE.len();
    core::iter::from_fn(move || {
        if at + 12 > png.len() {
            return None;
        }
        let length = be32(&png[at..at + 4]) as usize;
        let kind = &png[at + 4..at + 8];
        let data = png.get(at + 8..at + 8 + length)?;
        at += 12 + length;
        Some((kind, data))
    })
}

pub fn png_header(png: &[u8]) -> Result<PngHeader, Error> {
    if !png.starts_with(&PNG_SIGNATURE) {
        return Err(Error::NotPng);
    }
    let (kind, data) = chunks(png).next().ok_or(Error::Corrupt)?;
    if kind != b"IHDR" || data.len() != 13 {
        return Err(Error::Corrupt);
    }
    let header = PngHeader {
        width: be32(&data[0..4]),
        height: be32(&data[4..8]),
        bit_depth: data[8],
        color_type: data[9],
        interlace: data[12],
    };
    if header.width == 0 || header.height == 0 || header.width > 4096 || header.height > 4096 {
        return Err(Error::Corrupt);
    }
    if header.bit_depth != 8 || header.channels() == 0 || header.interlace != 0 {
        return Err(Error::Unsupported);
    }
    Ok(header)
}

/// How big `raw` and `rgba` must be for [`decode_png`].
pub fn decoded_size(png: &[u8]) -> Result<(usize, usize), Error> {
    let header = png_header(png)?;
    Ok((header.raw_size(), header.rgba_size()))
}

fn paeth(left: u8, up: u8, up_left: u8) -> u8 {
    let p = left as i16 + up as i16 - up_left as i16;
    let pa = (p - left as i16).abs();
    let pb = (p - up as i16).abs();
    let pc = (p - up_left as i16).abs();
    if pa <= pb && pa <= pc {
        left
    } else if pb <= pc {
        up
    } else {
        up_left
    }
}

/// Decode `png` into straight-alpha RGBA (`rgba`, 4 bytes a pixel, rows
/// packed), using `raw` for the inflated scanlines. 8-bit, non-interlaced
/// greyscale, RGB, palette and their alpha forms.
pub fn decode_png(png: &[u8], raw: &mut [u8], rgba: &mut [u8]) -> Result<PngHeader, Error> {
    let header = png_header(png)?;
    let raw_size = header.raw_size();
    if raw.len() < raw_size || rgba.len() < header.rgba_size() {
        return Err(Error::BufferTooSmall);
    }

    let mut palette = [[0u8, 0, 0, 255]; 256];
    let mut inflater = DecompressorOxide::new();
    let mut produced = 0usize;
    let mut done = false;
    let base = inflate_flags::TINFL_FLAG_PARSE_ZLIB_HEADER | inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;

    let mut idat = chunks(png).filter(|(kind, _)| *kind == b"IDAT").peekable();
    for (kind, data) in chunks(png) {
        match kind {
            b"PLTE" => {
                for (index, rgb) in data.chunks_exact(3).take(256).enumerate() {
                    palette[index] = [rgb[0], rgb[1], rgb[2], 255];
                }
            }
            b"tRNS" if header.color_type == 3 => {
                for (index, &alpha) in data.iter().take(256).enumerate() {
                    palette[index][3] = alpha;
                }
            }
            _ => {}
        }
    }

    while let Some((_, data)) = idat.next() {
        if done {
            break;
        }
        let more = idat.peek().is_some();
        let flags = base | if more { inflate_flags::TINFL_FLAG_HAS_MORE_INPUT } else { 0 };
        let mut consumed = 0usize;
        loop {
            let (status, read, written) = decompress(&mut inflater, &data[consumed..], &mut raw[..raw_size], produced, flags);
            consumed += read;
            produced += written;
            match status {
                TINFLStatus::Done => {
                    done = true;
                    break;
                }
                TINFLStatus::NeedsMoreInput => break,
                TINFLStatus::HasMoreOutput => {
                    // The output buffer is exactly the image: more means corrupt.
                    if produced >= raw_size {
                        return Err(Error::Corrupt);
                    }
                    if read == 0 && written == 0 {
                        return Err(Error::Corrupt);
                    }
                }
                _ => return Err(Error::Corrupt),
            }
            if consumed >= data.len() {
                break;
            }
        }
    }
    if produced != raw_size {
        return Err(Error::Corrupt);
    }

    // Undo the per-row filters in place, then expand to RGBA.
    let channels = header.channels();
    let stride = header.width as usize * channels;
    for row in 0..header.height as usize {
        let start = row * (stride + 1);
        let filter = raw[start];
        for column in 0..stride {
            let index = start + 1 + column;
            let left = if column >= channels { raw[index - channels] } else { 0 };
            let up = if row > 0 { raw[index - stride - 1] } else { 0 };
            let up_left = if row > 0 && column >= channels { raw[index - stride - 1 - channels] } else { 0 };
            let value = raw[index];
            raw[index] = match filter {
                0 => value,
                1 => value.wrapping_add(left),
                2 => value.wrapping_add(up),
                3 => value.wrapping_add(((left as u16 + up as u16) / 2) as u8),
                4 => value.wrapping_add(paeth(left, up, up_left)),
                _ => return Err(Error::Corrupt),
            };
        }
    }

    for row in 0..header.height as usize {
        let line = &raw[row * (stride + 1) + 1..(row + 1) * (stride + 1)];
        for x in 0..header.width as usize {
            let source = &line[x * channels..(x + 1) * channels];
            let pixel = match header.color_type {
                0 => [source[0], source[0], source[0], 255],
                2 => [source[0], source[1], source[2], 255],
                3 => palette[source[0] as usize],
                4 => [source[0], source[0], source[0], source[1]],
                _ => [source[0], source[1], source[2], source[3]],
            };
            let out = (row * header.width as usize + x) * 4;
            rgba[out..out + 4].copy_from_slice(&pixel);
        }
    }
    Ok(header)
}

/// Scale a straight-alpha RGBA image to `size` x `size` ARGB8888 pixels
/// (`0xAARRGGBB`, straight alpha) by averaging the area each output pixel
/// covers, in premultiplied alpha so transparent edges do not darken.
pub fn scale_to_argb(rgba: &[u8], width: u32, height: u32, size: u32, out: &mut [u32]) {
    let size = size as usize;
    if size == 0 || out.len() < size * size || width == 0 || height == 0 {
        return;
    }
    let (width, height) = (width as usize, height as usize);
    // Source span per output pixel, in 1/256 source pixels.
    let step_x = width * 256 / size;
    let step_y = height * 256 / size;

    for oy in 0..size {
        let y0 = oy * height * 256 / size;
        let y1 = (y0 + step_y.max(1)).min(height * 256);
        for ox in 0..size {
            let x0 = ox * width * 256 / size;
            let x1 = (x0 + step_x.max(1)).min(width * 256);
            let (mut r, mut g, mut b, mut a, mut weight) = (0u64, 0u64, 0u64, 0u64, 0u64);
            let mut sy = y0 / 256;
            while sy * 256 < y1 {
                let wy = ((sy + 1) * 256).min(y1) - (sy * 256).max(y0);
                let mut sx = x0 / 256;
                while sx * 256 < x1 {
                    let wx = ((sx + 1) * 256).min(x1) - (sx * 256).max(x0);
                    let w = (wx * wy) as u64;
                    let index = (sy.min(height - 1) * width + sx.min(width - 1)) * 4;
                    let alpha = rgba[index + 3] as u64;
                    r += rgba[index] as u64 * alpha * w;
                    g += rgba[index + 1] as u64 * alpha * w;
                    b += rgba[index + 2] as u64 * alpha * w;
                    a += alpha * w;
                    weight += w;
                    sx += 1;
                }
                sy += 1;
            }
            let pixel = if a == 0 || weight == 0 {
                0
            } else {
                let alpha = (a + weight / 2) / weight;
                let red = ((r + a / 2) / a).min(255);
                let green = ((g + a / 2) / a).min(255);
                let blue = ((b + a / 2) / a).min(255);
                ((alpha.min(255) as u32) << 24) | ((red as u32) << 16) | ((green as u32) << 8) | blue as u32
            };
            out[oy * size + ox] = pixel;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{vec, vec::Vec};

    /// A PNG of `pixels` (RGBA, one row per slice), stored uncompressed
    /// (zlib "stored" blocks) with the given row filter.
    fn make_png(width: u32, rows: &[&[u8]], filter: u8) -> Vec<u8> {
        let mut raw = Vec::new();
        for row in rows {
            raw.push(filter);
            // Filter 0 or "Sub" encoded properly.
            if filter == 1 {
                for (index, &byte) in row.iter().enumerate() {
                    let left = if index >= 4 { row[index - 4] } else { 0 };
                    raw.push(byte.wrapping_sub(left));
                }
            } else {
                raw.extend_from_slice(row);
            }
        }
        let mut zlib = vec![0x78, 0x01, 0x01];
        zlib.extend_from_slice(&(raw.len() as u16).to_le_bytes());
        zlib.extend_from_slice(&(!(raw.len() as u16)).to_le_bytes());
        zlib.extend_from_slice(&raw);
        let adler = {
            let (mut a, mut b) = (1u32, 0u32);
            for &byte in &raw {
                a = (a + byte as u32) % 65521;
                b = (b + a) % 65521;
            }
            (b << 16) | a
        };
        zlib.extend_from_slice(&adler.to_be_bytes());

        let mut png = PNG_SIGNATURE.to_vec();
        let mut chunk = |kind: &[u8], data: &[u8]| {
            png.extend_from_slice(&(data.len() as u32).to_be_bytes());
            png.extend_from_slice(kind);
            png.extend_from_slice(data);
            png.extend_from_slice(&[0, 0, 0, 0]);
        };
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&(rows.len() as u32).to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        chunk(b"IHDR", &ihdr);
        // Split the image data over two chunks, as encoders may.
        let half = zlib.len() / 2;
        chunk(b"IDAT", &zlib[..half]);
        chunk(b"IDAT", &zlib[half..]);
        chunk(b"IEND", &[]);
        png
    }

    #[test]
    fn decodes_rgba_across_idat_chunks_and_filters() {
        let rows: [&[u8]; 2] = [&[255, 0, 0, 255, 0, 255, 0, 128], &[0, 0, 255, 255, 10, 20, 30, 0]];
        for filter in [0, 1] {
            let png = make_png(2, &rows, filter);
            let (raw_size, rgba_size) = decoded_size(&png).unwrap();
            let mut raw = vec![0; raw_size];
            let mut rgba = vec![0; rgba_size];
            let header = decode_png(&png, &mut raw, &mut rgba).unwrap();
            assert_eq!((header.width, header.height), (2, 2));
            assert_eq!(rgba, [255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 255, 10, 20, 30, 0]);
        }
    }

    #[test]
    fn picks_the_smallest_png_that_covers_the_tile() {
        let png = make_png(1, &[&[1, 2, 3, 4]], 0);
        let mut icns = b"icns\0\0\0\0".to_vec();
        for kind in [b"ic07", b"ic13", b"ic10"] {
            let mut marked = png.clone();
            marked.push(kind[3]);
            icns.extend_from_slice(kind);
            icns.extend_from_slice(&((marked.len() + 8) as u32).to_be_bytes());
            icns.extend_from_slice(&marked);
        }
        let total = icns.len() as u32;
        icns[4..8].copy_from_slice(&total.to_be_bytes());

        let tag = |target| *icns_best_png(&icns, target).unwrap().last().unwrap();
        assert_eq!(tag(96), b'7'); // 128 covers 96
        assert_eq!(tag(200), b'3'); // 256
        assert_eq!(tag(2000), b'0'); // nothing covers it: the biggest
        assert_eq!(icns_best_png(b"nope", 64), Err(Error::NotIcns));
    }

    #[test]
    fn scaling_averages_and_keeps_edges_clean() {
        // 2x2: opaque red, transparent (black), opaque red, transparent.
        let rgba = [255, 0, 0, 255, 0, 0, 0, 0, 255, 0, 0, 255, 0, 0, 0, 0];
        let mut out = [0u32; 1];
        scale_to_argb(&rgba, 2, 2, 1, &mut out);
        // Half covered, and still pure red where it is covered.
        assert_eq!(out[0], (128 << 24) | 0x00FF_0000);
    }
}
