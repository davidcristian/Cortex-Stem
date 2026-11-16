//! Icon extraction and caching.
//!
//! Icon key scheme (produced in `rules::identity` for `identity`/`enumerate`,
//! consumed by [`IconCache::get_or_load`]):
//! - `hwnd:<id>:<pid>`: the window's own icon. The candidates `WM_GETICON`
//!   `ICON_BIG` / `ICON_SMALL` (via `SendMessageTimeoutW`) and `GCLP_HICON` /
//!   `GCLP_HICONSM` are gathered and the largest (by `GetIconInfo`
//!   dimensions) wins; when it is smaller than the requested size and the
//!   window's exe is known, the exe chain is tried and its result is used
//!   if it is larger than the window icon. The process id is part of the
//!   key because HWND values are recycled: a handle whose window now
//!   belongs to another process (`GetWindowThreadProcessId`) gives no icon
//!   (`rules::identity::hwnd_icon_owner_matches`); the older
//!   two-part `hwnd:<id>` form is still accepted and cannot be checked.
//! - `exe:<path>` / `file:<path>`: the file's icon. A PE image (`.exe`,
//!   `.dll`, `.scr`; `rules::icons::is_pe_image_path`) is mapped as a data
//!   file and its first `RT_GROUP_ICON` (what `ExtractIconW(.., 0)` and the
//!   shell show as the application icon) is read directly: chain
//!   `PE resources`. The directory's best entry of at most 256 px (largest
//!   edge, then bit depth, then resource size:
//!   `rules::icons::select_group_icon_entry`) is created at its native size
//!   with `CreateIconFromResourceEx` (PNG-compressed 256 px entries
//!   included), so no OS resampling happens. Any other file, and a PE image
//!   whose resources cannot be read (the mapping fails, no icon group,
//!   malformed directory; each logged at debug), goes to
//!   `IShellItemImageFactory`, then `PrivateExtractIconsW`, then
//!   `SHGetFileInfoW`. (Packaged exes under `WindowsApps` mapped fine on
//!   a Windows 11 PC; the fallback is for whatever does not.)
//! - `aumid:<id>`: a packaged app's logo (`shell:AppsFolder\<aumid>` through
//!   `IShellItemImageFactory`).
//!
//! Output is always 32-bit RGBA PNG bytes whose longer edge is **at most**
//! the requested size, and the downscale from the source is done here, by
//! the area-averaging filter in `rules::icons` (`IconImage::fit_within`),
//! which averages premultiplied colour so alpha edges get no light or dark
//! halo. The PE chain hands it the icon's own largest image. The shell
//! image factory resamples to exactly the size it is asked for, whatever
//! `SIIGBF_BIGGERSIZEOK` suggests (measured on Windows 11: every request
//! from 32 to 256 px came back at that size, with a light halo around
//! opaque shapes), so it is always asked for `MAX_SIZE` (256 px) and the
//! result is shrunk here. An icon whose largest image is smaller than that
//! is **upscaled by the shell** and accepted as it comes (for a `.lnk`
//! target or a document type the shell's upscale is still better than its
//! downscale; the source size in the log line is then 256x256 whatever the
//! icon holds). `E_PENDING` from `GetImage` (the shell has not finished
//! extracting the image; it was seen for `chrome.exe` and
//! `WindowsTerminal.exe` on first access, when exes still went through the
//! shell) is retried once after `E_PENDING_RETRY`.
//! `PrivateExtractIconsW` and `SHGetFileInfoW`, the last fallbacks, ask for
//! the requested size directly. Class or window icons come at whatever size
//! the app registered. Nothing is upscaled by this module: an image smaller
//! than the request is returned at its own size (the UI decides how to draw
//! it). `HICON` -> pixels goes through `GetIconInfo` + `GetDIBits` with
//! alpha preserved; all GDI objects and module mappings are freed (RAII
//! guards in `util`). The pixel maths and the icon-directory parser live in
//! `rules::icons`, unit-tested on every host. One `debug` line per real
//! load (never per cache hit) names the key, the requested size, the chain
//! that won and the source and output dimensions.
//!
//! `exe:`, `file:` and `aumid:` results are cached per (key, size) in
//! memory; `hwnd:` results are **not** cached (window icons are cheap to
//! read, the window may change its icon, and a stale entry could outlive the
//! window), the UI caches them per key for the session. Across sessions,
//! `stem_service::icon_cache` keeps the results for `exe:` and `file:` keys
//! of local PE images on disk.
//!
//! Ownership rules (see `docs/development/windows-rules.md`):
//! - icons from `WM_GETICON` / `GCLP_HICON` are **shared** with the window and
//!   are never destroyed here;
//! - icons from `CreateIconFromResourceEx` / `PrivateExtractIconsW` /
//!   `SHGetFileInfoW` and bitmaps from `GetIconInfo` / `GetImage` are
//!   **owned** and wrapped in guards; a module mapped for its resources is
//!   freed by `util::LoadedLibrary`, and resource bytes are copied out of
//!   the mapping before it goes.
//!
//! Bounds: each `WM_GETICON` uses a 200 ms `SMTO_ABORTIFHUNG` timeout and
//! the second one is skipped when the first got no answer; the PE chain is
//! file reads and in-process GDI calls (no messages to other processes),
//! with each resource capped at `MAX_RESOURCE_BYTES`; dimensions are capped
//! by `rules::icons::MAX_EDGE`.
//!
//! Elevated and protected windows. UIPI refuses `WM_GETICON` to a
//! window of higher integrity at once (`ERROR_ACCESS_DENIED`, no 200 ms
//! spent; the trace line names the reason, a hung window's says timed
//! out), so the `hwnd:` chain falls back to the class icons
//! (`GetClassLongPtrW` reads class data and is not a message) and then, as
//! for any window, to the exe's icon when the process could be opened,
//! which it can for an elevated process. An elevated window with a
//! readable process goes through this chain once at most (the engine shows
//! the window's `hwnd:` key until its identity resolves); after that
//! `identity` gives it the same `exe:` or `aumid:` key as its unelevated
//! twin. A protected process's window with no class icon yields
//! `Ok(None)`, which the UI renders as its neutral placeholder tile and
//! does not ask for again in the session.

mod bitmap;
mod pe;
mod shell;
mod window;

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use stem_core::IconKey;

use super::util;
use crate::PlatformResult;
use crate::rules::icons::{self as pixels, IconImage};
use crate::rules::identity::{WINDOW_ICON_NAMESPACE, apps_folder_parsing_name};
use bitmap::convert_error;
use pe::pe_resource_icon;
use shell::{extract_icon, shell_file_info_icon, shell_item_image};
use window::load_window_icon;

/// Requested sizes are clamped to this range.
const MIN_SIZE: u32 = 16;
const MAX_SIZE: u32 = 256;

type CacheKey = (IconKey, u32);

#[derive(Debug, Default)]
pub struct IconCache {
    entries: Mutex<HashMap<CacheKey, Arc<Vec<u8>>>>,
}

impl IconCache {
    pub fn get_or_load(&self, key: &IconKey, size: u32) -> PlatformResult<Option<Vec<u8>>> {
        // `hwnd:` keys are tied to a live window and are not cached.
        if !pixels::is_cacheable(key) {
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
        WINDOW_ICON_NAMESPACE => load_window_icon(value, size)?,
        "exe" | "file" => load_file_icon(value, size)?,
        "aumid" => shell_item_image(&apps_folder_parsing_name(value))?,
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

/// `exe:` / `file:` chain: the PE icon resources (the icon's own largest
/// image, no OS resampling) when the path names a PE image, then the shell
/// image factory at `MAX_SIZE`, then `PrivateExtractIconsW` at the
/// requested size, then `SHGetFileInfoW`. Each failure is logged at debug
/// and falls through to the next link.
fn load_file_icon(path: &str, size: u32) -> PlatformResult<Option<Loaded>> {
    if pixels::is_pe_image_path(path) {
        match pe_resource_icon(path) {
            Ok(Some(image)) => return Ok(Some(Loaded::new("PE resources", image))),
            Ok(None) => {}
            Err(e) => log::debug!("{path}: PE resources failed: {e}"),
        }
    }
    match shell_item_image(path) {
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

/// These run on Windows hosts only (this module is `cfg(windows)`) and use
/// files every Windows installation has. They exercise the `PE resources`
/// chain end to end (mapping, enumeration, directory parsing,
/// `CreateIconFromResourceEx`, `GetDIBits`); the shell chain is not tested
/// here because its answers depend on the shell's caches.
#[cfg(test)]
mod tests;
