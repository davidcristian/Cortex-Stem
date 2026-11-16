//! The shell chains: `IShellItemImageFactory`, `PrivateExtractIconsW` and
//! `SHGetFileInfoW`.

use std::time::Duration;

use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::HBITMAP;
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON,
    SHGFI_SMALLICON, SHGetFileInfoW, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
};
use windows::Win32::UI::WindowsAndMessaging::{HICON, PrivateExtractIconsW};
use windows::core::HRESULT;

use super::bitmap::{bitmap_to_image, icon_to_image};
use super::{Loaded, MAX_SIZE};
use crate::PlatformResult;
use crate::rules::icons::{AlphaMode, IconImage};
use crate::windows::util::{self, OwnedBitmap, OwnedIcon};

/// `PrivateExtractIconsW` takes a fixed `MAX_PATH` buffer.
const MAX_PATH: usize = 260;
/// `E_PENDING`: "the data necessary to complete this operation is not yet
/// available". `IShellItemImageFactory::GetImage` answers it while the
/// shell is still extracting an image it has not cached.
const E_PENDING: HRESULT = HRESULT(0x8000_000A_u32 as i32);
/// How long to wait before the single `E_PENDING` retry.
const E_PENDING_RETRY: Duration = Duration::from_millis(50);

/// `IShellItemImageFactory::GetImage` on a parsing name (a path or
/// `shell:AppsFolder\<aumid>`), always for `MAX_SIZE` so that the caller's
/// `fit_within` does the downscale: the shell resamples to exactly the size
/// it is asked for (`SIIGBF_BIGGERSIZEOK` notwithstanding), and its filter
/// leaves a halo. `SIIGBF_ICONONLY` keeps thumbnails out. A source smaller
/// than `MAX_SIZE` is upscaled by the shell and accepted as it comes.
/// `E_PENDING` is retried once after `E_PENDING_RETRY`. The bitmap is
/// 32-bit premultiplied ARGB.
pub(super) fn shell_item_image(parsing_name: &str) -> PlatformResult<Option<Loaded>> {
    let name = util::to_wide(parsing_name);
    // SAFETY: `name` is NUL-terminated and outlives the call; no bind context.
    let factory: IShellItemImageFactory =
        unsafe { SHCreateItemFromParsingName(util::pcwstr(&name), None) }?;
    let hbm = match shell_image_bitmap(&factory) {
        Err(e) if e.code() == E_PENDING => {
            log::debug!(
                "{parsing_name}: GetImage answered E_PENDING; retrying once after {} ms",
                E_PENDING_RETRY.as_millis()
            );
            std::thread::sleep(E_PENDING_RETRY);
            shell_image_bitmap(&factory)
        }
        other => other,
    }?;
    let Some(bitmap) = OwnedBitmap::new(hbm) else {
        return Ok(None);
    };
    let image = bitmap_to_image(&bitmap, AlphaMode::Premultiplied)?;
    Ok(Some(Loaded::new("IShellItemImageFactory", image)))
}

/// One `GetImage` call for `MAX_SIZE`; the bitmap is owned by the caller.
fn shell_image_bitmap(factory: &IShellItemImageFactory) -> windows::core::Result<HBITMAP> {
    let edge = MAX_SIZE as i32;
    // SAFETY: plain COM call; the returned bitmap is owned by the caller,
    // who wraps it in a guard.
    unsafe {
        factory.GetImage(
            SIZE { cx: edge, cy: edge },
            SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK,
        )
    }
}

/// `PrivateExtractIconsW` at exactly `size`. Owned icon.
pub(super) fn extract_icon(path: &str, size: u32) -> PlatformResult<Option<IconImage>> {
    let wide = util::to_wide(path);
    if wide.len() > MAX_PATH {
        return Ok(None);
    }
    let mut buffer = [0u16; MAX_PATH];
    buffer[..wide.len()].copy_from_slice(&wide);
    let mut icons = [HICON::default()];
    let edge = size as i32;
    // SAFETY: `buffer` is NUL-terminated; `icons` has room for the single
    // icon requested; no icon-id output is asked for.
    let count = unsafe { PrivateExtractIconsW(&buffer, 0, edge, edge, Some(&mut icons), None, 0) };
    // 0xFFFF_FFFF means "file not found"; 0 means "no icons".
    if count == 0 || count == u32::MAX {
        return Ok(None);
    }
    let Some(icon) = OwnedIcon::new(icons[0]) else {
        return Ok(None);
    };
    icon_to_image(icon.handle())
}

/// `SHGetFileInfoW(SHGFI_ICON)`: the shell's icon for any file, at the
/// system large (32) or small (16) size. Owned icon.
pub(super) fn shell_file_info_icon(path: &str, size: u32) -> PlatformResult<Option<IconImage>> {
    let wide = util::to_wide(path);
    let mut info = SHFILEINFOW::default();
    let size_flag = if size > 24 {
        SHGFI_LARGEICON
    } else {
        SHGFI_SMALLICON
    };
    // SAFETY: `info` is a valid out-structure of the size passed; the path is
    // NUL-terminated and outlives the call.
    let ok = unsafe {
        SHGetFileInfoW(
            util::pcwstr(&wide),
            FILE_ATTRIBUTE_NORMAL,
            Some(&mut info),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | size_flag,
        )
    };
    if ok == 0 {
        return Ok(None);
    }
    let Some(icon) = OwnedIcon::new(info.hIcon) else {
        return Ok(None);
    };
    icon_to_image(icon.handle())
}
