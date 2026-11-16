//! Pixel buffers: their size, the per-pixel conversions from what Windows
//! returns to straight-alpha RGBA, and PNG encoding.

/// Bytes per pixel in every buffer handled here.
pub const BYTES_PER_PIXEL: usize = 4;

/// How the alpha channel of a 32-bit source buffer is encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlphaMode {
    /// Colour channels are independent of alpha (`HICON` colour bitmaps).
    Straight,
    /// Colour channels were multiplied by alpha
    /// (`IShellItemImageFactory::GetImage` bitmaps).
    Premultiplied,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IconConvertError {
    #[error("icon has invalid dimensions {width}x{height}")]
    BadDimensions { width: u32, height: u32 },
    #[error("pixel buffer holds {actual} bytes, expected {expected}")]
    BufferSize { expected: usize, actual: usize },
    #[error("png encoding failed: {0}")]
    Png(String),
}

/// Largest edge we are willing to convert; anything bigger is not an icon.
pub const MAX_EDGE: u32 = 1024;

/// Byte length of a `width`×`height` 32-bit buffer, or `None` when the
/// dimensions are zero or unreasonably large.
pub fn buffer_len(width: u32, height: u32) -> Option<usize> {
    if width == 0 || height == 0 || width > MAX_EDGE || height > MAX_EDGE {
        return None;
    }
    Some(width as usize * height as usize * BYTES_PER_PIXEL)
}

/// Swap the first and third channel of every pixel (BGRA ↔ RGBA).
/// Trailing bytes that do not form a whole pixel are dropped.
pub fn bgra_to_rgba(bgra: &[u8]) -> Vec<u8> {
    bgra.chunks_exact(BYTES_PER_PIXEL)
        .flat_map(|px| [px[2], px[1], px[0], px[3]])
        .collect()
}

/// `true` when at least one pixel has a non-zero alpha byte.
pub fn has_any_alpha(pixels: &[u8]) -> bool {
    pixels.chunks_exact(BYTES_PER_PIXEL).any(|px| px[3] != 0)
}

/// Set alpha from an AND mask (same dimensions, 32-bit as described in the
/// `icons` module docs): a mask pixel with any non-zero channel is transparent,
/// a black mask pixel is opaque. Pixels without a mask counterpart are made
/// opaque.
pub fn apply_mask_alpha(rgba: &mut [u8], mask: &[u8]) {
    let mut mask_px = mask.chunks_exact(BYTES_PER_PIXEL);
    for px in rgba.chunks_exact_mut(BYTES_PER_PIXEL) {
        let transparent = mask_px
            .next()
            .is_some_and(|m| m[0] != 0 || m[1] != 0 || m[2] != 0);
        px[3] = if transparent { 0 } else { 255 };
    }
}

/// Make every pixel fully opaque.
pub fn set_opaque(rgba: &mut [u8]) {
    for px in rgba.chunks_exact_mut(BYTES_PER_PIXEL) {
        px[3] = 255;
    }
}

/// Convert premultiplied colour channels back to straight alpha, rounding
/// to nearest and clamping to 255. Fully transparent pixels keep colour 0.
pub fn unpremultiply(rgba: &mut [u8]) {
    for px in rgba.chunks_exact_mut(BYTES_PER_PIXEL) {
        let a = u32::from(px[3]);
        if a == 0 {
            px[0] = 0;
            px[1] = 0;
            px[2] = 0;
            continue;
        }
        if a == 255 {
            continue;
        }
        for c in &mut px[..3] {
            let v = (u32::from(*c) * 255 + a / 2) / a;
            *c = v.min(255) as u8;
        }
    }
}

/// Split the bitmap of a monochrome icon (no colour bitmap; the mask bitmap
/// is twice as tall) into its AND half (top) and XOR half (bottom).
/// `height` is the icon height, i.e. half the bitmap height.
pub fn split_stacked_mask(stacked: &[u8], width: u32, height: u32) -> Option<(&[u8], &[u8])> {
    let half = buffer_len(width, height)?;
    if stacked.len() != half * 2 {
        return None;
    }
    Some(stacked.split_at(half))
}

/// Encode an RGBA buffer as an 8-bit RGBA PNG.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, IconConvertError> {
    let expected =
        buffer_len(width, height).ok_or(IconConvertError::BadDimensions { width, height })?;
    if rgba.len() != expected {
        return Err(IconConvertError::BufferSize {
            expected,
            actual: rgba.len(),
        });
    }
    let mut out = Vec::with_capacity(expected / 4 + 64);
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| IconConvertError::Png(e.to_string()))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| IconConvertError::Png(e.to_string()))?;
    }
    Ok(out)
}
