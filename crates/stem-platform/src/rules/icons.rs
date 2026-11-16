//! Pixel conversion helpers for icons.
//!
//! BGRA → RGBA, mask-derived alpha for icons whose colour bitmap carries no
//! alpha, un-premultiplication, an area-averaging downscale, and PNG encoding
//! of an RGBA buffer. Pure and tested here with synthetic buffers. The
//! `RT_GROUP_ICON` directory parser and the rule that picks the image to
//! load from it ([`parse_group_icon_dir`], [`select_group_icon_entry`]) live
//! here too, with the rule for which paths are PE images
//! ([`is_pe_image_path`]); the Windows side only moves bytes. So does the
//! rule for which keys the in-memory cache keeps ([`is_cacheable`], over
//! [`is_window_icon_key`]).
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

mod image;
mod pixels;
mod resources;
mod scale;

use stem_core::IconKey;

use super::identity::is_window_icon_key;

pub use image::{IconImage, convert_icon, decode_icon};
pub use pixels::{
    AlphaMode, BYTES_PER_PIXEL, IconConvertError, MAX_EDGE, apply_mask_alpha, bgra_to_rgba,
    buffer_len, encode_png, has_any_alpha, set_opaque, split_stacked_mask, unpremultiply,
};
pub use resources::{
    GROUP_ICON_ENTRY_LEN, GROUP_ICON_HEADER_LEN, GroupIconEntry, GroupIconError, is_pe_image_path,
    parse_group_icon_dir, select_group_icon_entry,
};
pub use scale::{downscale_rgba, fit_dimensions};

/// Whether `windows::icons::IconCache` keeps the PNG of the icon key `key`:
/// every key but a window's own icon ([`is_window_icon_key`], the one
/// `hwnd:` rule), which is tied to a live window that may change its
/// icon, so an entry could outlive the window or go stale.
pub fn is_cacheable(key: &IconKey) -> bool {
    !is_window_icon_key(key)
}

#[cfg(test)]
mod tests;
