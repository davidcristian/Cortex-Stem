//! Pixel conversion helpers for icons.
//!
//! BGRA → RGBA, mask-derived alpha for icons whose colour bitmap carries no
//! alpha, un-premultiplication, an area-averaging downscale, and PNG encoding
//! of an RGBA buffer. Pure and tested here with synthetic buffers. The
//! `RT_GROUP_ICON` directory parser and the rule that picks the image to
//! load from it ([`parse_group_icon_dir`], [`select_group_icon_entry`]) live
//! here too, with the rule for which paths are PE images
//! ([`is_pe_image_path`]); the Windows side only moves bytes.
//!
//! Buffer conventions (what `GetDIBits` produces when asked for a top-down
//! 32-bit `BITMAPINFOHEADER`):
//! - rows top to bottom, no padding, 4 bytes per pixel in **B, G, R, A** order;
//! - a monochrome AND mask read the same way becomes a 32-bit buffer where a
//!   set mask bit is white (`FF FF FF ..`) and a clear bit is black; a set
//!   bit means "let the screen show through", i.e. transparent.
//!
//! Pipeline: [`decode_icon`] turns a bitmap (plus mask) into an [`IconImage`]
//! (straight-alpha RGBA); [`IconImage::fit_within`] shrinks it to the
//! requested edge with a box filter when the source is larger (it never
//! upscales, and the Windows side never asks the OS to upscale either);
//! [`IconImage::encode_png`] produces the bytes the UI receives.
//! [`convert_icon`] is the unscaled shortcut through the same stages.

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

/// A decoded icon: straight-alpha RGBA, rows top to bottom, no padding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl IconImage {
    /// Wrap a buffer, validating the dimensions against [`buffer_len`].
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, IconConvertError> {
        let expected =
            buffer_len(width, height).ok_or(IconConvertError::BadDimensions { width, height })?;
        if rgba.len() != expected {
            return Err(IconConvertError::BufferSize {
                expected,
                actual: rgba.len(),
            });
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    /// The longer side.
    pub fn edge(&self) -> u32 {
        self.width.max(self.height)
    }

    /// Shrink so that the longer side is `max_edge`, preserving the aspect
    /// ratio, with [`downscale_rgba`]. An image that already fits is returned
    /// unchanged: this never upscales.
    pub fn fit_within(self, max_edge: u32) -> Self {
        let Some((width, height)) = fit_dimensions(self.width, self.height, max_edge) else {
            return self;
        };
        match downscale_rgba(self.width, self.height, &self.rgba, width, height) {
            Ok(rgba) => Self {
                width,
                height,
                rgba,
            },
            // `fit_dimensions` only yields targets `downscale_rgba` accepts
            // for a buffer `new` validated; keep the source rather than fail.
            Err(_) => self,
        }
    }

    /// Encode as an 8-bit RGBA PNG.
    pub fn encode_png(&self) -> Result<Vec<u8>, IconConvertError> {
        encode_png(self.width, self.height, &self.rgba)
    }
}

/// Decode a 32-bit BGRA colour buffer (plus, optionally, its AND mask in the
/// same layout) into a straight-alpha [`IconImage`].
///
/// - When the colour buffer has an alpha channel (any non-zero alpha), the
///   mask is ignored, as Windows itself does for 32-bit icons, and the alpha
///   is un-premultiplied when `alpha` says so.
/// - When it has none, alpha comes from the mask; without a mask the icon is
///   treated as opaque (old 24-bit icons).
pub fn decode_icon(
    width: u32,
    height: u32,
    color_bgra: &[u8],
    mask_bgra: Option<&[u8]>,
    alpha: AlphaMode,
) -> Result<IconImage, IconConvertError> {
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
    IconImage::new(width, height, rgba)
}

/// The whole pipeline without scaling: [`decode_icon`] then
/// [`IconImage::encode_png`].
pub fn convert_icon(
    width: u32,
    height: u32,
    color_bgra: &[u8],
    mask_bgra: Option<&[u8]>,
    alpha: AlphaMode,
) -> Result<Vec<u8>, IconConvertError> {
    decode_icon(width, height, color_bgra, mask_bgra, alpha)?.encode_png()
}

/// Dimensions that fit `width`×`height` inside a `max_edge` square while
/// preserving the aspect ratio (the shorter side rounded to nearest, at
/// least 1). `None` when the image already fits, or when `max_edge` is 0:
/// this is a shrink-only rule.
pub fn fit_dimensions(width: u32, height: u32, max_edge: u32) -> Option<(u32, u32)> {
    if max_edge == 0 || width == 0 || height == 0 || width.max(height) <= max_edge {
        return None;
    }
    let shrink = |side: u32, longer: u32| -> u32 {
        let scaled =
            (u64::from(side) * u64::from(max_edge) + u64::from(longer) / 2) / u64::from(longer);
        (scaled as u32).max(1)
    };
    Some(if width >= height {
        (max_edge, shrink(height, width))
    } else {
        (shrink(width, height), max_edge)
    })
}

/// For every destination index, the source indices it covers with their
/// overlap, in units of `1/dst` source pixels; the overlaps of one
/// destination index sum to `src`. Exact integer arithmetic, so weights of a
/// non-integer ratio (e.g. 3 → 2: 2 + 1, 1 + 2) carry no rounding error.
fn coverage(src: u32, dst: u32) -> Vec<Vec<(usize, u64)>> {
    let (src, dst) = (u64::from(src), u64::from(dst));
    (0..dst)
        .map(|i| {
            let start = i * src;
            let end = start + src;
            let first = start / dst;
            let last = end.div_ceil(dst).min(src);
            (first..last)
                .filter_map(|j| {
                    let overlap = end.min((j + 1) * dst).saturating_sub(start.max(j * dst));
                    (overlap > 0).then_some((j as usize, overlap))
                })
                .collect()
        })
        .collect()
}

/// Area-averaging (box filter) downscale of a straight-alpha RGBA buffer to
/// `dst_width`×`dst_height`, each at most the source side. Colour is
/// averaged **premultiplied** (so fully transparent neighbours contribute no
/// colour and edges get no light or dark fringe) and converted back to
/// straight alpha, rounding to nearest. Fully transparent output pixels are
/// all zero. Never upscales: a larger target is `BadDimensions`.
pub fn downscale_rgba(
    width: u32,
    height: u32,
    rgba: &[u8],
    dst_width: u32,
    dst_height: u32,
) -> Result<Vec<u8>, IconConvertError> {
    let expected =
        buffer_len(width, height).ok_or(IconConvertError::BadDimensions { width, height })?;
    if rgba.len() != expected {
        return Err(IconConvertError::BufferSize {
            expected,
            actual: rgba.len(),
        });
    }
    let out_len = buffer_len(dst_width, dst_height).ok_or(IconConvertError::BadDimensions {
        width: dst_width,
        height: dst_height,
    })?;
    if dst_width > width || dst_height > height {
        return Err(IconConvertError::BadDimensions {
            width: dst_width,
            height: dst_height,
        });
    }
    let columns = coverage(width, dst_width);
    let rows = coverage(height, dst_height);
    // Every destination pixel's weights sum to this (see `coverage`).
    let total = u64::from(width) * u64::from(height);
    let round_div = |n: u64, d: u64| ((n + d / 2) / d).min(255) as u8;
    let stride = width as usize * BYTES_PER_PIXEL;

    let mut out = Vec::with_capacity(out_len);
    for ys in &rows {
        for xs in &columns {
            let (mut r, mut g, mut b, mut a) = (0u64, 0u64, 0u64, 0u64);
            for &(sy, wy) in ys {
                let row = &rgba[sy * stride..(sy + 1) * stride];
                for &(sx, wx) in xs {
                    let px = &row[sx * BYTES_PER_PIXEL..(sx + 1) * BYTES_PER_PIXEL];
                    let w = wy * wx;
                    let alpha = u64::from(px[3]);
                    let wa = w * alpha;
                    r += wa * u64::from(px[0]);
                    g += wa * u64::from(px[1]);
                    b += wa * u64::from(px[2]);
                    a += wa;
                }
            }
            let out_alpha = round_div(a, total);
            if out_alpha == 0 || a == 0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                out.extend_from_slice(&[
                    round_div(r, a),
                    round_div(g, a),
                    round_div(b, a),
                    out_alpha,
                ]);
            }
        }
    }
    Ok(out)
}

// ---- PE icon resources ----

/// Whether `path` names a PE image whose icon resources can be read
/// directly: the file name ends in `.exe`, `.dll` or `.scr` (any case).
/// Explicit string handling, so the answer is the same on every host
/// (`std::path` treats `\` as a separator only on Windows).
pub fn is_pe_image_path(path: &str) -> bool {
    let file_name = path.rsplit(['\\', '/']).next().unwrap_or(path);
    let Some((stem, extension)) = file_name.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && matches!(
            extension.to_ascii_lowercase().as_str(),
            "exe" | "dll" | "scr"
        )
}

/// Byte length of the `GRPICONDIR` header (`idReserved`, `idType`,
/// `idCount`, each `u16`).
pub const GROUP_ICON_HEADER_LEN: usize = 6;
/// Byte length of one `GRPICONDIRENTRY`.
pub const GROUP_ICON_ENTRY_LEN: usize = 14;
/// `idType` of an icon directory (2 is a cursor directory).
const GROUP_ICON_TYPE: u16 = 1;

/// One image of an `RT_GROUP_ICON` directory: the dimensions it declares
/// (0 in the resource means 256) and the id of the `RT_ICON` resource that
/// holds its bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupIconEntry {
    pub width: u32,
    pub height: u32,
    /// `wBitCount`: bits per pixel of the image (a PNG entry says 32).
    pub bit_count: u16,
    /// `dwBytesInRes`: the size of the `RT_ICON` resource.
    pub bytes_in_res: u32,
    /// `nId`: the `RT_ICON` resource id.
    pub id: u16,
}

impl GroupIconEntry {
    /// The longer side.
    pub fn edge(&self) -> u32 {
        self.width.max(self.height)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum GroupIconError {
    #[error("icon group directory is truncated ({len} bytes, {needed} needed)")]
    Truncated { len: usize, needed: usize },
    #[error("icon group directory has type {0}, expected {GROUP_ICON_TYPE}")]
    BadType(u16),
    #[error("icon group directory lists no images")]
    Empty,
}

/// Parse an `RT_GROUP_ICON` resource (`GRPICONDIR`: `idReserved u16`,
/// `idType u16 = 1`, `idCount u16`, then `idCount` entries of
/// `bWidth u8, bHeight u8, bColorCount u8, bReserved u8, wPlanes u16,
/// wBitCount u16, dwBytesInRes u32, nId u16`, all little-endian). A width
/// or height of 0 means 256. Trailing bytes after the last entry are
/// ignored; `idReserved` is not checked (tools do write junk there).
pub fn parse_group_icon_dir(bytes: &[u8]) -> Result<Vec<GroupIconEntry>, GroupIconError> {
    let u16_at = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let u32_at =
        |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    if bytes.len() < GROUP_ICON_HEADER_LEN {
        return Err(GroupIconError::Truncated {
            len: bytes.len(),
            needed: GROUP_ICON_HEADER_LEN,
        });
    }
    let kind = u16_at(2);
    if kind != GROUP_ICON_TYPE {
        return Err(GroupIconError::BadType(kind));
    }
    let count = usize::from(u16_at(4));
    if count == 0 {
        return Err(GroupIconError::Empty);
    }
    let needed = GROUP_ICON_HEADER_LEN + count * GROUP_ICON_ENTRY_LEN;
    if bytes.len() < needed {
        return Err(GroupIconError::Truncated {
            len: bytes.len(),
            needed,
        });
    }
    let edge = |declared: u8| {
        if declared == 0 {
            256
        } else {
            u32::from(declared)
        }
    };
    Ok((0..count)
        .map(|i| {
            let at = GROUP_ICON_HEADER_LEN + i * GROUP_ICON_ENTRY_LEN;
            GroupIconEntry {
                width: edge(bytes[at]),
                height: edge(bytes[at + 1]),
                bit_count: u16_at(at + 6),
                bytes_in_res: u32_at(at + 8),
                id: u16_at(at + 12),
            }
        })
        .collect())
}

/// The image to load from an icon group when the request is at most
/// `max_edge` pixels: the largest edge that is `<= max_edge`, ties broken by
/// the highest bit depth, then the largest resource, then the earliest
/// entry. `None` when every entry is larger than `max_edge`.
pub fn select_group_icon_entry(
    entries: &[GroupIconEntry],
    max_edge: u32,
) -> Option<GroupIconEntry> {
    entries
        .iter()
        .rev()
        .filter(|entry| entry.edge() <= max_edge)
        .max_by_key(|entry| (entry.edge(), entry.bit_count, entry.bytes_in_res))
        .copied()
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

    /// A 32x32 BGRA icon with a transparent frame and an opaque red centre.
    fn framed_red_32() -> Vec<u8> {
        let (w, h) = (32u32, 32u32);
        let mut color = Vec::with_capacity(buffer_len(w, h).unwrap());
        for y in 0..h {
            for x in 0..w {
                let inside = (4..28).contains(&x) && (4..28).contains(&y);
                color.extend_from_slice(&[0, 0, 255, if inside { 255 } else { 0 }]);
            }
        }
        color
    }

    #[test]
    fn convert_icon_handles_a_realistic_32px_buffer() {
        let (w, h) = (32u32, 32u32);
        let color = framed_red_32();
        let png = convert_icon(w, h, &color, None, AlphaMode::Straight).expect("png");
        let (dw, dh, rgba) = decode(&png);
        assert_eq!((dw, dh), (w, h));
        // Straight alpha: a transparent pixel keeps its colour bytes.
        assert_eq!(&rgba[..4], &[255, 0, 0, 0]);
        let centre = ((16 * w + 16) as usize) * BYTES_PER_PIXEL;
        assert_eq!(&rgba[centre..centre + 4], &[255, 0, 0, 255]);
    }

    #[test]
    fn icon_image_validates_its_buffer() {
        assert_eq!(
            IconImage::new(2, 1, vec![0; 4]),
            Err(IconConvertError::BufferSize {
                expected: 8,
                actual: 4
            })
        );
        assert_eq!(
            IconImage::new(0, 1, vec![]),
            Err(IconConvertError::BadDimensions {
                width: 0,
                height: 1
            })
        );
        let image = IconImage::new(2, 1, vec![0; 8]).expect("image");
        assert_eq!(image.edge(), 2);
        assert_eq!(IconImage::new(3, 7, vec![0; 84]).unwrap().edge(), 7);
    }

    #[test]
    fn fit_dimensions_shrinks_only_and_keeps_the_aspect_ratio() {
        assert_eq!(fit_dimensions(256, 256, 48), Some((48, 48)));
        assert_eq!(fit_dimensions(256, 128, 48), Some((48, 24)));
        assert_eq!(fit_dimensions(128, 256, 48), Some((24, 48)));
        assert_eq!(fit_dimensions(64, 48, 32), Some((32, 24)));
        // Rounded to nearest, never below 1.
        assert_eq!(fit_dimensions(100, 30, 10), Some((10, 3)));
        assert_eq!(fit_dimensions(100, 1, 10), Some((10, 1)));
        assert_eq!(fit_dimensions(1, 100, 10), Some((1, 10)));
        // Already fits: untouched (no upscaling).
        assert_eq!(fit_dimensions(48, 48, 48), None);
        assert_eq!(fit_dimensions(32, 32, 48), None);
        assert_eq!(fit_dimensions(16, 32, 32), None);
        // Degenerate inputs.
        assert_eq!(fit_dimensions(0, 32, 16), None);
        assert_eq!(fit_dimensions(32, 32, 0), None);
    }

    #[test]
    fn coverage_weights_are_exact_for_non_integer_ratios() {
        // 3 source pixels into 2: each destination pixel spans 1.5 source
        // pixels, in units of 1/2: [2, 1] and [1, 2].
        assert_eq!(
            coverage(3, 2),
            vec![vec![(0, 2), (1, 1)], vec![(1, 1), (2, 2)]]
        );
        // Integer ratio: whole pixels, weight = dst each.
        assert_eq!(
            coverage(4, 2),
            vec![vec![(0, 2), (1, 2)], vec![(2, 2), (3, 2)]]
        );
        // Identity.
        assert_eq!(coverage(2, 2), vec![vec![(0, 2)], vec![(1, 2)]]);
        // Every destination index's overlaps sum to `src`.
        for (src, dst) in [(256, 48), (97, 13), (5, 4), (1024, 1)] {
            for weights in coverage(src, dst) {
                assert_eq!(weights.iter().map(|w| w.1).sum::<u64>(), u64::from(src));
            }
        }
    }

    #[test]
    fn downscale_averages_whole_pixels() {
        // 2x2 opaque: red, green / blue, white -> one pixel, the mean.
        let rgba = [
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255, 255, 255, 255],
        ]
        .concat();
        let out = downscale_rgba(2, 2, &rgba, 1, 1).expect("downscale");
        assert_eq!(out, vec![128, 128, 128, 255]);
    }

    #[test]
    fn downscale_handles_fractional_coverage() {
        // 3x1 opaque greys 0, 90, 180 -> 2x1: (0*2 + 90*1)/3 and (90*1 + 180*2)/3.
        let rgba = [[0, 0, 0, 255], [90, 90, 90, 255], [180, 180, 180, 255]].concat();
        let out = downscale_rgba(3, 1, &rgba, 2, 1).expect("downscale");
        assert_eq!(out, vec![30, 30, 30, 255, 150, 150, 150, 255]);
    }

    #[test]
    fn downscale_averages_premultiplied_so_transparent_neighbours_add_no_colour() {
        // An opaque red pixel next to a fully transparent *green* pixel: the
        // result is pure red at half alpha, not a brownish fringe.
        let rgba = [[255, 0, 0, 255], [0, 255, 0, 0]].concat();
        let out = downscale_rgba(2, 1, &rgba, 1, 1).expect("downscale");
        assert_eq!(out, vec![255, 0, 0, 128]);
        // Half-transparent white next to opaque black: premultiplied mean is
        // (255*128 + 0) / (128 + 255) for the colour, (128 + 255) / 2 for alpha.
        let rgba = [[255, 255, 255, 128], [0, 0, 0, 255]].concat();
        let out = downscale_rgba(2, 1, &rgba, 1, 1).expect("downscale");
        assert_eq!(out, vec![85, 85, 85, 192]);
        // All transparent stays all zero (no colour leaks through).
        let rgba = [[9, 9, 9, 0], [200, 1, 2, 0]].concat();
        let out = downscale_rgba(2, 1, &rgba, 1, 1).expect("downscale");
        assert_eq!(out, vec![0, 0, 0, 0]);
    }

    #[test]
    fn downscale_rejects_upscaling_and_bad_buffers() {
        assert_eq!(
            downscale_rgba(2, 2, &[0; 16], 3, 2),
            Err(IconConvertError::BadDimensions {
                width: 3,
                height: 2
            })
        );
        assert_eq!(
            downscale_rgba(2, 2, &[0; 16], 1, 0),
            Err(IconConvertError::BadDimensions {
                width: 1,
                height: 0
            })
        );
        assert_eq!(
            downscale_rgba(2, 2, &[0; 8], 1, 1),
            Err(IconConvertError::BufferSize {
                expected: 16,
                actual: 8
            })
        );
        // Same size is a copy.
        let rgba = [1, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(downscale_rgba(2, 1, &rgba, 2, 1).unwrap(), rgba.to_vec());
    }

    #[test]
    fn fit_within_never_upscales_and_keeps_the_aspect_ratio() {
        let small = IconImage::new(32, 32, vec![7; 32 * 32 * 4]).unwrap();
        assert_eq!(small.clone().fit_within(48), small);
        assert_eq!(small.clone().fit_within(32), small);
        let wide = IconImage::new(64, 32, vec![7; 64 * 32 * 4]).unwrap();
        let fitted = wide.fit_within(16);
        assert_eq!((fitted.width, fitted.height), (16, 8));
        assert_eq!(fitted.rgba.len(), 16 * 8 * 4);
        assert!(fitted.rgba.iter().all(|&b| b == 7));
    }

    #[test]
    fn decode_fit_encode_pipeline_shrinks_the_realistic_icon() {
        let image = decode_icon(32, 32, &framed_red_32(), None, AlphaMode::Straight).unwrap();
        assert_eq!((image.width, image.height), (32, 32));
        let fitted = image.fit_within(16);
        let png = fitted.encode_png().expect("png");
        let (w, h, rgba) = decode(&png);
        assert_eq!((w, h), (16, 16));
        // The 4-pixel transparent frame became a 2-pixel one: corner is
        // transparent (and zeroed), the pixel at (2, 2) is fully opaque red,
        // and nothing in between is fringed with another colour.
        assert_eq!(&rgba[..4], &[0, 0, 0, 0]);
        let at = |x: usize, y: usize| &rgba[(y * 16 + x) * 4..(y * 16 + x) * 4 + 4];
        assert_eq!(at(2, 2), &[255, 0, 0, 255]);
        assert_eq!(at(8, 8), &[255, 0, 0, 255]);
        assert_eq!(at(1, 8), &[0, 0, 0, 0]);
        for px in rgba.chunks_exact(4) {
            assert!(px[3] == 0 || px[..3] == [255, 0, 0], "pixel {px:?}");
        }
    }

    #[test]
    fn downscale_of_a_large_icon_is_bounded_and_exact_on_flat_colour() {
        // 256 -> 48 (the shell's SIIGBF_BIGGERSIZEOK case): a flat colour
        // survives untouched, and alpha of a half-transparent flat image too.
        let image = IconImage::new(256, 256, [10, 20, 30, 200].repeat(256 * 256)).unwrap();
        let fitted = image.fit_within(48);
        assert_eq!((fitted.width, fitted.height), (48, 48));
        assert!(
            fitted
                .rgba
                .chunks_exact(4)
                .all(|px| px == [10, 20, 30, 200])
        );
    }

    // ---- PE icon resources ----

    #[test]
    fn pe_image_paths_are_exe_dll_scr_in_any_case() {
        for path in [
            r"C:\Program Files\Discord\Discord.exe",
            r"C:\WINDOWS\EXPLORER.EXE",
            r"C:\Windows\System32\shell32.dll",
            "C:/Windows/System32/imageres.Dll",
            r"C:\Windows\System32\Bubbles.scr",
            "app.exe",
        ] {
            assert!(is_pe_image_path(path), "{path}");
        }
        for path in [
            r"C:\Users\me\Desktop\Discord.lnk",
            r"C:\dir.exe\readme.txt",
            r"C:\Program Files\App\app.exe\",
            r"C:\Program Files\App\.exe",
            "exe",
            "app.exe.bak",
            "notes.",
            "",
        ] {
            assert!(!is_pe_image_path(path), "{path}");
        }
    }

    /// A `GRPICONDIR` with the given entries (`width, height, bit_count,
    /// bytes_in_res, id`), the type word as given.
    fn group_dir(kind: u16, entries: &[(u8, u8, u16, u32, u16)]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&kind.to_le_bytes());
        bytes.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for &(w, h, bits, size, id) in entries {
            bytes.extend_from_slice(&[w, h, 0, 0]);
            bytes.extend_from_slice(&1u16.to_le_bytes());
            bytes.extend_from_slice(&bits.to_le_bytes());
            bytes.extend_from_slice(&size.to_le_bytes());
            bytes.extend_from_slice(&id.to_le_bytes());
        }
        bytes
    }

    fn entry(
        width: u32,
        height: u32,
        bit_count: u16,
        bytes_in_res: u32,
        id: u16,
    ) -> GroupIconEntry {
        GroupIconEntry {
            width,
            height,
            bit_count,
            bytes_in_res,
            id,
        }
    }

    #[test]
    fn parse_group_icon_dir_reads_entries_and_maps_zero_to_256() {
        let bytes = group_dir(
            1,
            &[
                (16, 16, 4, 744, 1),
                (48, 48, 32, 9640, 2),
                (0, 0, 32, 9442, 3),
            ],
        );
        assert_eq!(
            bytes.len(),
            GROUP_ICON_HEADER_LEN + 3 * GROUP_ICON_ENTRY_LEN
        );
        assert_eq!(
            parse_group_icon_dir(&bytes),
            Ok(vec![
                entry(16, 16, 4, 744, 1),
                entry(48, 48, 32, 9640, 2),
                entry(256, 256, 32, 9442, 3),
            ])
        );
        // Trailing bytes (padding) are tolerated; junk in idReserved too.
        let mut padded = bytes.clone();
        padded.extend_from_slice(&[0xAA, 0xBB]);
        padded[0] = 0xFF;
        assert_eq!(parse_group_icon_dir(&padded).map(|e| e.len()), Ok(3));
        // A non-square entry keeps both sides.
        let wide = group_dir(1, &[(64, 0, 24, 100, 7)]);
        assert_eq!(
            parse_group_icon_dir(&wide),
            Ok(vec![entry(64, 256, 24, 100, 7)])
        );
    }

    #[test]
    fn parse_group_icon_dir_rejects_malformed_directories() {
        assert_eq!(
            parse_group_icon_dir(&[]),
            Err(GroupIconError::Truncated { len: 0, needed: 6 })
        );
        assert_eq!(
            parse_group_icon_dir(&[0, 0, 1, 0, 1]),
            Err(GroupIconError::Truncated { len: 5, needed: 6 })
        );
        // Header says two entries, only one is present.
        let mut short = group_dir(1, &[(32, 32, 32, 4264, 1)]);
        short[4] = 2;
        assert_eq!(
            parse_group_icon_dir(&short),
            Err(GroupIconError::Truncated {
                len: 20,
                needed: 34
            })
        );
        // A cursor directory is not an icon directory.
        assert_eq!(
            parse_group_icon_dir(&group_dir(2, &[(32, 32, 1, 326, 1)])),
            Err(GroupIconError::BadType(2))
        );
        assert_eq!(
            parse_group_icon_dir(&group_dir(0, &[(32, 32, 1, 326, 1)])),
            Err(GroupIconError::BadType(0))
        );
        assert_eq!(
            parse_group_icon_dir(&group_dir(1, &[])),
            Err(GroupIconError::Empty)
        );
    }

    #[test]
    fn select_group_icon_entry_prefers_largest_edge_then_depth_then_bytes() {
        // Discord-like directory: a 256 px PNG entry wins at 256.
        let entries = [
            entry(16, 16, 32, 1128, 1),
            entry(32, 32, 32, 4264, 2),
            entry(48, 48, 32, 9640, 3),
            entry(256, 256, 32, 9442, 4),
        ];
        assert_eq!(select_group_icon_entry(&entries, 256), Some(entries[3]));
        // The cap filters by edge: 48 for a 96 px request, 32 for 32.
        assert_eq!(select_group_icon_entry(&entries, 96), Some(entries[2]));
        assert_eq!(select_group_icon_entry(&entries, 32), Some(entries[1]));
        assert_eq!(select_group_icon_entry(&entries, 8), None);
        assert_eq!(select_group_icon_entry(&[], 256), None);
        // Same edge: the highest bit depth wins regardless of order.
        let depths = [
            entry(48, 48, 32, 9640, 1),
            entry(48, 48, 8, 3752, 2),
            entry(48, 48, 4, 1640, 3),
        ];
        assert_eq!(select_group_icon_entry(&depths, 256), Some(depths[0]));
        let mut reversed = depths;
        reversed.reverse();
        assert_eq!(select_group_icon_entry(&reversed, 256), Some(depths[0]));
        // Same edge and depth: the larger resource wins.
        let sizes = [
            entry(256, 256, 32, 9442, 1),
            entry(256, 256, 32, 270_376, 2),
        ];
        assert_eq!(select_group_icon_entry(&sizes, 256), Some(sizes[1]));
        // A full tie keeps the earliest entry.
        let ties = [entry(256, 256, 32, 9442, 1), entry(256, 256, 32, 9442, 2)];
        assert_eq!(select_group_icon_entry(&ties, 256), Some(ties[0]));
        // The longer side counts as the edge.
        let oblong = [entry(64, 128, 32, 100, 1), entry(96, 96, 32, 100, 2)];
        assert_eq!(select_group_icon_entry(&oblong, 256), Some(oblong[0]));
        assert_eq!(select_group_icon_entry(&oblong, 100), Some(oblong[1]));
    }

    #[test]
    fn group_icon_errors_describe_themselves() {
        assert_eq!(
            GroupIconError::Truncated { len: 5, needed: 6 }.to_string(),
            "icon group directory is truncated (5 bytes, 6 needed)"
        );
        assert_eq!(
            GroupIconError::BadType(2).to_string(),
            "icon group directory has type 2, expected 1"
        );
    }
}
