//! Pixel conversion helpers for icons.
//!
//! BGRA → RGBA, mask-derived alpha for icons whose colour bitmap carries no
//! alpha, un-premultiplication, and PNG encoding of an RGBA buffer. Pure and
//! tested here with synthetic buffers.
//!
//! Buffer conventions (what `GetDIBits` produces when asked for a top-down
//! 32-bit `BITMAPINFOHEADER`):
//! - rows top to bottom, no padding, 4 bytes per pixel in **B, G, R, A** order;
//! - a monochrome AND mask read the same way becomes a 32-bit buffer where a
//!   set mask bit is white (`FF FF FF ..`) and a clear bit is black; a set
//!   bit means "let the screen show through", i.e. transparent.

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
/// module docs): a mask pixel with any non-zero channel is transparent,
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

/// The whole pipeline: a 32-bit BGRA colour buffer (plus, optionally, its
/// AND mask in the same layout) to RGBA PNG bytes.
///
/// - When the colour buffer has an alpha channel (any non-zero alpha), the
///   mask is ignored, as Windows itself does for 32-bit icons, and the alpha
///   is un-premultiplied when `alpha` says so.
/// - When it has none, alpha comes from the mask; without a mask the icon is
///   treated as opaque (old 24-bit icons).
pub fn convert_icon(
    width: u32,
    height: u32,
    color_bgra: &[u8],
    mask_bgra: Option<&[u8]>,
    alpha: AlphaMode,
) -> Result<Vec<u8>, IconConvertError> {
    let expected =
        buffer_len(width, height).ok_or(IconConvertError::BadDimensions { width, height })?;
    if color_bgra.len() != expected {
        return Err(IconConvertError::BufferSize {
            expected,
            actual: color_bgra.len(),
        });
    }
    let mut rgba = bgra_to_rgba(color_bgra);
    if has_any_alpha(&rgba) {
        if alpha == AlphaMode::Premultiplied {
            unpremultiply(&mut rgba);
        }
    } else if let Some(mask) = mask_bgra.filter(|m| m.len() == expected) {
        apply_mask_alpha(&mut rgba, mask);
    } else {
        set_opaque(&mut rgba);
    }
    encode_png(width, height, &rgba)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    const BLACK: [u8; 4] = [0, 0, 0, 0];
    const WHITE: [u8; 4] = [255, 255, 255, 0];

    fn px(bgra: [u8; 4]) -> Vec<u8> {
        bgra.to_vec()
    }

    fn decode(png: &[u8]) -> (u32, u32, Vec<u8>) {
        let decoder = png::Decoder::new(std::io::Cursor::new(png));
        let mut reader = decoder.read_info().expect("png header");
        let mut buf = vec![0u8; reader.output_buffer_size().expect("buffer size")];
        let info = reader.next_frame(&mut buf).expect("png frame");
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(info.bit_depth, png::BitDepth::Eight);
        buf.truncate(info.buffer_size());
        (info.width, info.height, buf)
    }

    #[test]
    fn buffer_len_rejects_degenerate_sizes() {
        assert_eq!(buffer_len(0, 4), None);
        assert_eq!(buffer_len(4, 0), None);
        assert_eq!(buffer_len(MAX_EDGE + 1, 1), None);
        assert_eq!(buffer_len(2, 3), Some(24));
    }

    #[test]
    fn bgra_to_rgba_swaps_red_and_blue() {
        let bgra = [1, 2, 3, 4, 10, 20, 30, 40, 99];
        assert_eq!(bgra_to_rgba(&bgra), vec![3, 2, 1, 4, 30, 20, 10, 40]);
    }

    #[test]
    fn has_any_alpha_detects_a_single_pixel() {
        assert!(!has_any_alpha(&[9, 9, 9, 0, 1, 1, 1, 0]));
        assert!(has_any_alpha(&[9, 9, 9, 0, 1, 1, 1, 7]));
    }

    #[test]
    fn mask_alpha_white_is_transparent_black_is_opaque() {
        let mut rgba = [1, 2, 3, 0, 4, 5, 6, 0, 7, 8, 9, 0];
        let mask = [WHITE, BLACK, [0, 0, 1, 0]].concat();
        apply_mask_alpha(&mut rgba, &mask);
        assert_eq!(rgba, [1, 2, 3, 0, 4, 5, 6, 255, 7, 8, 9, 0]);
    }

    #[test]
    fn mask_alpha_without_counterpart_is_opaque() {
        let mut rgba = [1, 2, 3, 0, 4, 5, 6, 0];
        apply_mask_alpha(&mut rgba, &WHITE);
        assert_eq!(rgba, [1, 2, 3, 0, 4, 5, 6, 255]);
    }

    #[test]
    fn unpremultiply_restores_straight_colour() {
        let mut rgba = [
            64, 32, 0, 128, 10, 20, 30, 0, 200, 100, 50, 255, 255, 1, 0, 1,
        ];
        unpremultiply(&mut rgba);
        assert_eq!(
            rgba,
            [
                128, 64, 0, 128, 0, 0, 0, 0, 200, 100, 50, 255, 255, 255, 0, 1
            ]
        );
    }

    #[test]
    fn split_stacked_mask_halves_the_buffer() {
        let stacked = [WHITE, BLACK, BLACK, WHITE].concat();
        let (and, xor) = split_stacked_mask(&stacked, 2, 1).expect("split");
        assert_eq!(and, [WHITE, BLACK].concat());
        assert_eq!(xor, [BLACK, WHITE].concat());
        assert_eq!(split_stacked_mask(&stacked, 2, 2), None);
        assert_eq!(split_stacked_mask(&stacked[..8], 2, 1), None);
    }

    #[test]
    fn encode_png_round_trips_pixels() {
        let rgba = [255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 9, 8, 7, 6];
        let png = encode_png(2, 2, &rgba).expect("png");
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(decode(&png), (2, 2, rgba.to_vec()));
    }

    #[test]
    fn encode_png_validates_input() {
        assert_eq!(
            encode_png(0, 1, &[]),
            Err(IconConvertError::BadDimensions {
                width: 0,
                height: 1
            })
        );
        assert_eq!(
            encode_png(1, 1, &[1, 2, 3]),
            Err(IconConvertError::BufferSize {
                expected: 4,
                actual: 3
            })
        );
    }

    #[test]
    fn convert_icon_with_alpha_ignores_mask_and_keeps_straight_alpha() {
        // BGRA: blue-ish pixel at half alpha; white mask would make it transparent.
        let color = px([200, 100, 50, 128]);
        let png = convert_icon(1, 1, &color, Some(&WHITE), AlphaMode::Straight).expect("png");
        assert_eq!(decode(&png).2, vec![50, 100, 200, 128]);
    }

    #[test]
    fn convert_icon_premultiplied_is_unpremultiplied() {
        let color = px([0, 32, 64, 128]);
        let png = convert_icon(1, 1, &color, None, AlphaMode::Premultiplied).expect("png");
        assert_eq!(decode(&png).2, vec![128, 64, 0, 128]);
    }

    #[test]
    fn convert_icon_without_alpha_uses_mask() {
        let color = [[1, 2, 3, 0], [4, 5, 6, 0]].concat();
        let mask = [WHITE, BLACK].concat();
        let png = convert_icon(2, 1, &color, Some(&mask), AlphaMode::Straight).expect("png");
        assert_eq!(decode(&png).2, vec![3, 2, 1, 0, 6, 5, 4, 255]);
    }

    #[test]
    fn convert_icon_without_alpha_or_mask_is_opaque() {
        let color = px([1, 2, 3, 0]);
        let png = convert_icon(1, 1, &color, None, AlphaMode::Straight).expect("png");
        assert_eq!(decode(&png).2, vec![3, 2, 1, 255]);
        // A mask of the wrong size is ignored, not misapplied.
        let png = convert_icon(1, 1, &color, Some(&[0; 8]), AlphaMode::Straight).expect("png");
        assert_eq!(decode(&png).2, vec![3, 2, 1, 255]);
    }

    #[test]
    fn convert_icon_rejects_bad_buffers() {
        assert_eq!(
            convert_icon(2, 2, &[0; 4], None, AlphaMode::Straight),
            Err(IconConvertError::BufferSize {
                expected: 16,
                actual: 4
            })
        );
        assert_eq!(
            convert_icon(0, 0, &[], None, AlphaMode::Straight),
            Err(IconConvertError::BadDimensions {
                width: 0,
                height: 0
            })
        );
    }

    #[test]
    fn convert_icon_handles_a_realistic_32px_buffer() {
        // A 32x32 icon with a transparent frame and an opaque centre.
        let (w, h) = (32u32, 32u32);
        let mut color = Vec::with_capacity(buffer_len(w, h).unwrap());
        for y in 0..h {
            for x in 0..w {
                let inside = (4..28).contains(&x) && (4..28).contains(&y);
                color.extend_from_slice(&[0, 0, 255, if inside { 255 } else { 0 }]);
            }
        }
        let png = convert_icon(w, h, &color, None, AlphaMode::Straight).expect("png");
        let (dw, dh, rgba) = decode(&png);
        assert_eq!((dw, dh), (w, h));
        // Straight alpha: a transparent pixel keeps its colour bytes.
        assert_eq!(&rgba[..4], &[255, 0, 0, 0]);
        let centre = ((16 * w + 16) as usize) * BYTES_PER_PIXEL;
        assert_eq!(&rgba[centre..centre + 4], &[255, 0, 0, 255]);
    }
}
