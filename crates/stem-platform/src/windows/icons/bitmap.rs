//! `HICON` and `HBITMAP` to pixels, through `GetIconInfo` and `GetDIBits`.

use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, GetDIBits, GetObjectW, HBITMAP,
    HGDIOBJ, RGBQUAD,
};
use windows::Win32::UI::WindowsAndMessaging::{GetIconInfo, HICON, ICONINFO};

use crate::rules::icons::{self as pixels, AlphaMode, IconConvertError, IconImage};
use crate::windows::util::{self, OwnedBitmap, ScreenDc};
use crate::{PlatformError, PlatformResult};

/// The bitmaps of an icon, as `GetIconInfo` hands them out: copies we own.
/// A monochrome icon has no colour bitmap and a mask twice the icon height.
struct IconBitmaps {
    color: Option<OwnedBitmap>,
    mask: OwnedBitmap,
}

fn icon_bitmaps(icon: HICON) -> PlatformResult<Option<IconBitmaps>> {
    let mut info = ICONINFO::default();
    // SAFETY: `info` is a valid out-pointer; GetIconInfo hands us copies of
    // the icon's bitmaps, which the guards delete.
    unsafe { GetIconInfo(icon, &mut info) }?;
    let color = OwnedBitmap::new(info.hbmColor);
    Ok(OwnedBitmap::new(info.hbmMask).map(|mask| IconBitmaps { color, mask }))
}

/// Width and height of an icon from its `GetIconInfo` bitmaps, or `None`
/// when they cannot be read. The bitmaps are deleted before returning.
pub(super) fn icon_dimensions(icon: HICON) -> Option<(u32, u32)> {
    let bitmaps = icon_bitmaps(icon).ok()??;
    match &bitmaps.color {
        Some(color) => bitmap_dimensions(color.handle()).ok(),
        None => {
            let (w, h) = bitmap_dimensions(bitmaps.mask.handle()).ok()?;
            Some((w, h / 2))
        }
    }
}

/// `HICON` -> straight-alpha image. Works for shared and owned icons alike;
/// the caller keeps ownership of `icon`. The bitmaps `GetIconInfo` creates
/// are ours and are deleted here.
pub(super) fn icon_to_image(icon: HICON) -> PlatformResult<Option<IconImage>> {
    let Some(IconBitmaps { color, mask }) = icon_bitmaps(icon)? else {
        return Ok(None);
    };
    let dc = ScreenDc::new()?;
    let (mask_w, mask_h) = bitmap_dimensions(mask.handle())?;
    match color {
        Some(color) => {
            let (w, h) = bitmap_dimensions(color.handle())?;
            let color_px = read_bgra(&dc, color.handle(), w, h)?;
            let mask_px = if (mask_w, mask_h) == (w, h) {
                Some(read_bgra(&dc, mask.handle(), w, h)?)
            } else {
                None
            };
            pixels::decode_icon(w, h, &color_px, mask_px.as_deref(), AlphaMode::Straight)
                .map(Some)
                .map_err(convert_error)
        }
        None => {
            // Monochrome icon: the mask bitmap stacks the AND mask (top) over
            // the XOR image (bottom).
            let h = mask_h / 2;
            let stacked = read_bgra(&dc, mask.handle(), mask_w, mask_h)?;
            let Some((and_mask, xor_image)) = pixels::split_stacked_mask(&stacked, mask_w, h)
            else {
                return Ok(None);
            };
            pixels::decode_icon(mask_w, h, xor_image, Some(and_mask), AlphaMode::Straight)
                .map(Some)
                .map_err(convert_error)
        }
    }
}

/// An owned 32-bit bitmap (image factory output) -> straight-alpha image.
pub(super) fn bitmap_to_image(bitmap: &OwnedBitmap, alpha: AlphaMode) -> PlatformResult<IconImage> {
    let dc = ScreenDc::new()?;
    let (w, h) = bitmap_dimensions(bitmap.handle())?;
    let bgra = read_bgra(&dc, bitmap.handle(), w, h)?;
    pixels::decode_icon(w, h, &bgra, None, alpha).map_err(convert_error)
}

/// Width and height of a bitmap via `GetObjectW`, validated against the
/// limits in `rules::icons`.
fn bitmap_dimensions(bitmap: HBITMAP) -> PlatformResult<(u32, u32)> {
    let mut bm = BITMAP::default();
    // SAFETY: `bm` is a valid out-structure of the size passed.
    let written = unsafe {
        GetObjectW(
            HGDIOBJ::from(bitmap),
            size_of::<BITMAP>() as i32,
            Some((&raw mut bm).cast()),
        )
    };
    if written == 0 {
        return Err(util::last_error());
    }
    let (w, h) = (
        u32::try_from(bm.bmWidth).unwrap_or(0),
        u32::try_from(bm.bmHeight).unwrap_or(0),
    );
    if pixels::buffer_len(w, h).is_none() {
        return Err(PlatformError::Other(format!(
            "unexpected bitmap dimensions {}x{}",
            bm.bmWidth, bm.bmHeight
        )));
    }
    Ok((w, h))
}

/// Read a bitmap as a top-down 32-bit BGRA buffer with `GetDIBits`. Works
/// for monochrome bitmaps too (GDI expands them to black/white pixels).
fn read_bgra(dc: &ScreenDc, bitmap: HBITMAP, width: u32, height: u32) -> PlatformResult<Vec<u8>> {
    let len = pixels::buffer_len(width, height).ok_or_else(|| {
        PlatformError::Other(format!("unexpected bitmap dimensions {width}x{height}"))
    })?;
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            // Negative height = top-down rows.
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        bmiColors: [RGBQUAD::default()],
    };
    let mut buffer = vec![0u8; len];
    // SAFETY: `buffer` holds exactly width*height*4 bytes, matching the
    // 32-bit top-down header; the bitmap is not selected into any DC.
    let lines = unsafe {
        GetDIBits(
            dc.handle(),
            bitmap,
            0,
            height,
            Some(buffer.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    if lines <= 0 || lines as u32 != height {
        return Err(PlatformError::Other(format!(
            "GetDIBits copied {lines} of {height} rows"
        )));
    }
    Ok(buffer)
}

pub(super) fn convert_error(e: IconConvertError) -> PlatformError {
    PlatformError::Other(e.to_string())
}
