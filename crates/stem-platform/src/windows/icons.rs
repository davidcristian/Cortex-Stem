//! Icon extraction and caching.
//!
//! Icon key scheme (produced in `rules::identity` for `identity`/`enumerate`,
//! consumed by [`IconCache::get_or_load`]):
//! - `hwnd:<id>:<pid>`: the window's own icon. The candidates `WM_GETICON`
//!   `ICON_BIG` / `ICON_SMALL` (via `SendMessageTimeoutW`) and `GCLP_HICON` /
//!   `GCLP_HICONSM` are gathered and the largest (by `GetIconInfo`
//!   dimensions) wins; when it is smaller than the requested size and the
//!   window's exe is known, the exe chain is tried first and its result is
//!   used if it is larger than the window icon. The process id is part of
//!   the key because HWND values are recycled; the older two-part
//!   `hwnd:<id>` form is still accepted (the pid is ignored when loading).
//! - `exe:<path>`: the executable's main icon (`IShellItemImageFactory`,
//!   then `PrivateExtractIconsW`, then `SHGetFileInfoW`)
//! - `file:<path>`: any file's shell icon (shortcuts, documents); same chain
//! - `aumid:<id>`: a packaged app's logo (`shell:AppsFolder\<aumid>` through
//!   `IShellItemImageFactory`)
//!
//! Output is always 32-bit RGBA PNG bytes whose longer edge is **at most**
//! the requested size: the shell is asked with `SIIGBF_BIGGERSIZEOK`, so it
//! hands back the next larger image an icon has (often 256 px) instead of
//! resampling itself, and class or window icons come at whatever size the
//! app registered; everything larger than requested is shrunk here with the
//! area-averaging filter in `rules::icons` (`IconImage::fit_within`), which
//! averages premultiplied colour so alpha edges stay clean. Nothing is ever
//! upscaled by this module (an icon whose largest image is smaller than the
//! request is returned at its own size; the UI decides how to draw it).
//! `HICON` -> pixels goes through `GetIconInfo` + `GetDIBits` with alpha
//! preserved; all GDI objects are freed (RAII guards in `util`). The pixel
//! maths live in `rules::icons`, which is unit-tested on every host. One
//! `debug` line per real load (never per cache hit) names the key, the
//! requested size, the chain that won and the source and output dimensions.
//!
//! `exe:`, `file:` and `aumid:` results are cached per (key, size) in
//! memory; `hwnd:` results are **not** cached (window icons are cheap to
//! read, the window may change its icon, and a stale entry could outlive the
//! window), the UI caches them per key for the session. Nothing is cached
//! on disk.
//!
//! Ownership rules (see `docs/development/windows-rules.md`):
//! - icons from `WM_GETICON` / `GCLP_HICON` are **shared** with the window and
//!   are never destroyed here;
//! - icons from `PrivateExtractIconsW` / `SHGetFileInfoW` and bitmaps from
//!   `GetIconInfo` / `GetImage` are **owned** and wrapped in guards.
//!
//! Bounds: each `WM_GETICON` uses a 200 ms `SMTO_ABORTIFHUNG` timeout and
//! the second one is skipped when the first timed out; dimensions are capped
//! by `rules::icons::MAX_EDGE`.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use stem_core::{IconKey, WindowId};
use windows::Win32::Foundation::{HWND, LPARAM, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, GetDIBits, GetObjectW, HBITMAP,
    HGDIOBJ, RGBQUAD,
};
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON,
    SHGFI_SMALLICON, SHGetFileInfoW, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GCLP_HICON, GCLP_HICONSM, GET_CLASS_LONG_INDEX, GetClassLongPtrW, GetIconInfo, HICON, ICON_BIG,
    ICON_SMALL, ICONINFO, IsWindow, PrivateExtractIconsW, SMTO_ABORTIFHUNG, SendMessageTimeoutW,
    WM_GETICON,
};

use super::util::{self, OwnedBitmap, OwnedIcon, ScreenDc};
use crate::rules::icons::{self as pixels, AlphaMode, IconConvertError, IconImage};
use crate::rules::identity::{apps_folder_parsing_name, parse_hwnd_icon_value};
use crate::{PlatformError, PlatformResult};

/// Cross-process `WM_GETICON` budget (`docs/development/windows-rules.md`: ≤ 200 ms).
const GETICON_TIMEOUT_MS: u32 = 200;
/// Requested sizes are clamped to this range.
const MIN_SIZE: u32 = 16;
const MAX_SIZE: u32 = 256;
/// `PrivateExtractIconsW` takes a fixed `MAX_PATH` buffer.
const MAX_PATH: usize = 260;

type CacheKey = (IconKey, u32);

/// Namespace of keys that are never cached here (see the module docs).
const UNCACHED_NAMESPACE: &str = "hwnd";

#[derive(Debug, Default)]
pub struct IconCache {
    entries: Mutex<HashMap<CacheKey, Arc<Vec<u8>>>>,
}

impl IconCache {
    pub fn get_or_load(&self, key: &IconKey, size: u32) -> PlatformResult<Option<Vec<u8>>> {
        if !Self::is_cacheable(key) {
            return load_icon_png(key, size);
        }
        if let Some(png) = self.entries.lock().get(&(key.clone(), size)) {
            return Ok(Some(png.as_ref().clone()));
        }
        let Some(png) = load_icon_png(key, size)? else {
            return Ok(None);
        };
        self.entries
            .lock()
            .insert((key.clone(), size), Arc::new(png.clone()));
        Ok(Some(png))
    }

    /// `hwnd:` keys are tied to a live window and are not cached.
    fn is_cacheable(key: &IconKey) -> bool {
        key.0
            .split_once(':')
            .is_none_or(|(namespace, _)| namespace != UNCACHED_NAMESPACE)
    }
}

/// What a chain produced: the decoded image at its source size, and the
/// chain's name for the log line.
struct Loaded {
    chain: &'static str,
    image: IconImage,
}

impl Loaded {
    fn new(chain: &'static str, image: IconImage) -> Self {
        Self { chain, image }
    }
}

/// Resolve a key to PNG bytes without caching. `Ok(None)` for keys of an
/// unknown namespace, dead windows, missing files and icon-less files. The
/// PNG's longer edge is at most `size` (clamped to 16..=256).
pub fn load_icon_png(key: &IconKey, size: u32) -> PlatformResult<Option<Vec<u8>>> {
    let size = size.clamp(MIN_SIZE, MAX_SIZE);
    let Some((namespace, value)) = key.0.split_once(':') else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    let _com = util::ComApartment::sta();
    let loaded = match namespace {
        UNCACHED_NAMESPACE => load_window_icon(value, size)?,
        "exe" | "file" => load_file_icon(value, size)?,
        "aumid" => shell_item_image(&apps_folder_parsing_name(value), size)?,
        _ => None,
    };
    let Some(Loaded { chain, image }) = loaded else {
        return Ok(None);
    };
    let (source_w, source_h) = (image.width, image.height);
    let image = image.fit_within(size);
    let png = image.encode_png().map_err(convert_error)?;
    log::debug!(
        "icon {}: {size} px via {chain}: source {source_w}x{source_h}, output {}x{}",
        key.0,
        image.width,
        image.height
    );
    Ok(Some(png))
}

/// `hwnd:` chain (see the module docs). `value` is `<id>:<pid>` or the older
/// bare `<id>`.
fn load_window_icon(value: &str, size: u32) -> PlatformResult<Option<Loaded>> {
    let Some(id) = parse_hwnd_icon_value(value) else {
        return Ok(None);
    };
    let hwnd = util::hwnd(WindowId(id));
    // SAFETY: IsWindow accepts any handle value and only reads.
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        log::debug!("hwnd:{id}: window is gone");
        return Ok(None);
    }
    let window =
        largest_window_icon(hwnd).and_then(|candidate| match icon_to_image(candidate.icon) {
            Ok(Some(image)) => Some(Loaded::new(candidate.source, image)),
            Ok(None) => None,
            Err(e) => {
                log::debug!("hwnd:{id}: converting the window icon failed: {e}");
                None
            }
        });
    // A window icon at least as large as requested is the answer.
    if let Some(found) = &window
        && found.image.edge() >= size
    {
        return Ok(window);
    }
    // Otherwise the exe's icon (large, high quality) is preferred, when the
    // process is known and it really is larger than what the window offers.
    if let Some(path) = window_exe_path(hwnd, id) {
        match load_file_icon(&path, size) {
            Ok(Some(from_exe))
                if window
                    .as_ref()
                    .is_none_or(|w| from_exe.image.edge() > w.image.edge()) =>
            {
                return Ok(Some(from_exe));
            }
            Ok(Some(from_exe)) => log::trace!(
                "hwnd:{id}: exe icon ({}x{}) is no larger than the window icon; keeping the window icon",
                from_exe.image.width,
                from_exe.image.height
            ),
            Ok(None) => {}
            Err(e) => log::debug!("hwnd:{id}: exe icon failed: {e}"),
        }
    }
    Ok(window)
}

/// A shared window or class icon with its source and measured edge.
#[derive(Debug, Clone, Copy)]
struct WindowIconCandidate {
    icon: HICON,
    source: &'static str,
    edge: u32,
}

/// Gather the window's icon candidates and pick the largest by
/// `GetIconInfo` dimensions (ties keep the earlier one: `ICON_BIG` first).
/// Shared handles; nothing here is destroyed. `None` when the window offers
/// no icon at all.
fn largest_window_icon(hwnd: HWND) -> Option<WindowIconCandidate> {
    let mut candidates: Vec<(HICON, &'static str)> = Vec::with_capacity(4);
    let mut push = |icon: Option<HICON>, source: &'static str| {
        if let Some(icon) = icon
            && !candidates.iter().any(|(seen, _)| seen.0 == icon.0)
        {
            candidates.push((icon, source));
        }
    };
    let (big, answered) = window_message_icon(hwnd, ICON_BIG);
    push(big, "WM_GETICON(ICON_BIG)");
    if answered {
        push(
            window_message_icon(hwnd, ICON_SMALL).0,
            "WM_GETICON(ICON_SMALL)",
        );
    } else {
        log::trace!(
            "window {:?}: WM_GETICON(ICON_BIG) got no answer; skipping ICON_SMALL",
            hwnd.0
        );
    }
    push(class_icon(hwnd, GCLP_HICON), "GCLP_HICON");
    push(class_icon(hwnd, GCLP_HICONSM), "GCLP_HICONSM");

    let mut best: Option<WindowIconCandidate> = None;
    for (icon, source) in candidates {
        let Some((w, h)) = icon_dimensions(icon) else {
            continue;
        };
        let edge = w.max(h);
        if best.is_none_or(|b| edge > b.edge) {
            best = Some(WindowIconCandidate { icon, source, edge });
        }
    }
    best
}

/// `WM_GETICON` with a bounded timeout. Returns the icon (which belongs to
/// the window) and whether the window answered at all; a `false` means the
/// target is hung (or gone) and further messages are pointless.
fn window_message_icon(hwnd: HWND, which: u32) -> (Option<HICON>, bool) {
    let mut result: usize = 0;
    // SAFETY: WM_GETICON carries no pointers; `result` is a valid out-pointer;
    // a hung or elevated target makes the call return 0 within the timeout.
    let sent = unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_GETICON,
            WPARAM(which as usize),
            LPARAM(0),
            SMTO_ABORTIFHUNG,
            GETICON_TIMEOUT_MS,
            Some(&mut result),
        )
    };
    if sent.0 == 0 {
        return (None, false);
    }
    if result == 0 {
        return (None, true);
    }
    (Some(HICON(result as *mut core::ffi::c_void)), true)
}

/// The window class icon. Shared; not destroyed.
fn class_icon(hwnd: HWND, index: GET_CLASS_LONG_INDEX) -> Option<HICON> {
    // SAFETY: GetClassLongPtrW only reads class data; it works across
    // processes and integrity levels.
    let value = unsafe { GetClassLongPtrW(hwnd, index) };
    (value != 0).then_some(HICON(value as *mut core::ffi::c_void))
}

/// The window's process image path (as a string), for the exe fallback of
/// the `hwnd:` chain. `None` when the window no longer qualifies or the
/// process cannot be read.
fn window_exe_path(hwnd: HWND, id: u64) -> Option<String> {
    match super::enumerate::query_window(hwnd) {
        Ok(Some(raw)) => raw.exe_path.map(|p| p.to_string_lossy().into_owned()),
        Ok(None) => None,
        Err(e) => {
            log::debug!("hwnd:{id}: no exe icon fallback (query_window: {e})");
            None
        }
    }
}

/// `exe:` / `file:` chain: shell image factory (large, high quality), then
/// `PrivateExtractIconsW` at the requested size, then `SHGetFileInfoW`.
fn load_file_icon(path: &str, size: u32) -> PlatformResult<Option<Loaded>> {
    match shell_item_image(path, size) {
        Ok(Some(loaded)) => return Ok(Some(loaded)),
        Ok(None) => {}
        Err(e) => log::debug!("{path}: shell image factory failed: {e}"),
    }
    match extract_icon(path, size) {
        Ok(Some(image)) => return Ok(Some(Loaded::new("PrivateExtractIconsW", image))),
        Ok(None) => {}
        Err(e) => log::debug!("{path}: PrivateExtractIconsW failed: {e}"),
    }
    Ok(shell_file_info_icon(path, size)?.map(|image| Loaded::new("SHGetFileInfoW", image)))
}

/// `IShellItemImageFactory::GetImage` on a parsing name (a path or
/// `shell:AppsFolder\<aumid>`), asking for `size` but accepting a bigger
/// image (`SIIGBF_BIGGERSIZEOK`) so the shell does not resample; the
/// caller shrinks it. The bitmap is 32-bit premultiplied ARGB.
fn shell_item_image(parsing_name: &str, size: u32) -> PlatformResult<Option<Loaded>> {
    let name = util::to_wide(parsing_name);
    // SAFETY: `name` is NUL-terminated and outlives the call; no bind context.
    let factory: IShellItemImageFactory =
        unsafe { SHCreateItemFromParsingName(util::pcwstr(&name), None) }?;
    let edge = size as i32;
    // SAFETY: plain COM call; the returned bitmap is owned by us and wrapped
    // in a guard below.
    let hbm = unsafe {
        factory.GetImage(
            SIZE { cx: edge, cy: edge },
            SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK,
        )
    }?;
    let Some(bitmap) = OwnedBitmap::new(hbm) else {
        return Ok(None);
    };
    let image = bitmap_to_image(&bitmap, AlphaMode::Premultiplied)?;
    Ok(Some(Loaded::new("IShellItemImageFactory", image)))
}

/// `PrivateExtractIconsW` at exactly `size`. Owned icon.
fn extract_icon(path: &str, size: u32) -> PlatformResult<Option<IconImage>> {
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
fn shell_file_info_icon(path: &str, size: u32) -> PlatformResult<Option<IconImage>> {
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
fn icon_dimensions(icon: HICON) -> Option<(u32, u32)> {
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
fn icon_to_image(icon: HICON) -> PlatformResult<Option<IconImage>> {
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
fn bitmap_to_image(bitmap: &OwnedBitmap, alpha: AlphaMode) -> PlatformResult<IconImage> {
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

fn convert_error(e: IconConvertError) -> PlatformError {
    PlatformError::Other(e.to_string())
}
