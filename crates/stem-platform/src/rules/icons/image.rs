//! A decoded icon, and the pipeline from a Windows bitmap to the PNG bytes
//! the UI receives.

use super::pixels::{
    AlphaMode, IconConvertError, apply_mask_alpha, bgra_to_rgba, buffer_len, encode_png,
    has_any_alpha, set_opaque, unpremultiply,
};
use super::scale::{downscale_rgba, fit_dimensions};

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
