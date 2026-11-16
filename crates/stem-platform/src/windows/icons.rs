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

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use stem_core::{IconKey, WindowId};
use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, GetDIBits, GetObjectW, HBITMAP,
    HGDIOBJ, RGBQUAD,
};
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows::Win32::System::LibraryLoader::{
    EnumResourceNamesW, FindResourceW, LoadResource, LockResource, SizeofResource,
};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON,
    SHGFI_SMALLICON, SHGetFileInfoW, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, GCLP_HICON, GCLP_HICONSM, GET_CLASS_LONG_INDEX, GetClassLongPtrW,
    GetIconInfo, GetWindowThreadProcessId, HICON, ICON_BIG, ICON_SMALL, ICONINFO, IsWindow,
    LR_DEFAULTCOLOR, PrivateExtractIconsW, RT_GROUP_ICON, RT_ICON, SMTO_ABORTIFHUNG,
    SendMessageTimeoutW, WM_GETICON,
};
use windows::core::{BOOL, HRESULT, PCWSTR};

use super::util::{self, LoadedLibrary, OwnedBitmap, OwnedIcon, ScreenDc};
use crate::rules::icons::{self as pixels, AlphaMode, IconConvertError, IconImage};
use crate::rules::identity::{
    apps_folder_parsing_name, hwnd_icon_owner_matches, parse_hwnd_icon_value,
};
use crate::{PlatformError, PlatformResult};

/// Cross-process `WM_GETICON` budget (`docs/development/windows-rules.md`: ≤ 200 ms).
const GETICON_TIMEOUT_MS: u32 = 200;
/// Requested sizes are clamped to this range.
const MIN_SIZE: u32 = 16;
const MAX_SIZE: u32 = 256;
/// `PrivateExtractIconsW` takes a fixed `MAX_PATH` buffer.
const MAX_PATH: usize = 260;
/// `E_PENDING`: "the data necessary to complete this operation is not yet
/// available". `IShellItemImageFactory::GetImage` answers it while the
/// shell is still extracting an image it has not cached.
const E_PENDING: HRESULT = HRESULT(0x8000_000A_u32 as i32);
/// How long to wait before the single `E_PENDING` retry.
const E_PENDING_RETRY: Duration = Duration::from_millis(50);
/// An `RT_GROUP_ICON` or `RT_ICON` resource larger than this is not an icon
/// (a 256 px 32-bit uncompressed image is about 270 KB).
const MAX_RESOURCE_BYTES: u32 = 4 * 1024 * 1024;
/// `dwVer` for `CreateIconFromResourceEx`: the Windows 3.x icon format, the
/// only value the function accepts.
const ICON_RESOURCE_VERSION: u32 = 0x0003_0000;

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
    // HWND values are recycled: a key that outlived its window may name
    // another process's window now.
    let mut process_id = 0u32;
    // SAFETY: `process_id` is a live out-pointer for the duration of the call;
    // a handle that went away meanwhile makes it return 0 and leaves it 0.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    if !hwnd_icon_owner_matches(value, process_id) {
        log::debug!(
            "hwnd:{value}: the handle belongs to another process ({process_id}) now; no icon"
        );
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
    match window_message_icon(hwnd, ICON_BIG) {
        Ok(big) => {
            push(big, "WM_GETICON(ICON_BIG)");
            push(
                window_message_icon(hwnd, ICON_SMALL).ok().flatten(),
                "WM_GETICON(ICON_SMALL)",
            );
        }
        Err(e) => log::trace!(
            "window {:?}: WM_GETICON(ICON_BIG) got no answer ({e}); skipping ICON_SMALL",
            hwnd.0
        ),
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

/// `WM_GETICON` with a bounded timeout. `Ok` carries the window's answer
/// (an icon that belongs to the window, or none); `Err` means the window
/// did not answer at all, and says why: `AccessDenied` for a window of
/// higher integrity (UIPI refuses the message at once), `Os` with
/// `ERROR_TIMEOUT` for a hung one. Further messages are then pointless.
fn window_message_icon(hwnd: HWND, which: u32) -> PlatformResult<Option<HICON>> {
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
        return Err(util::last_error());
    }
    if result == 0 {
        return Ok(None);
    }
    Ok(Some(HICON(result as *mut core::ffi::c_void)))
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

/// A resource name copied out of an `EnumResourceNamesW` callback, where
/// the string form is only valid for the duration of the call.
#[derive(Debug)]
enum ResourceName {
    /// `MAKEINTRESOURCE` id: the high word of the "pointer" is zero.
    Id(u16),
    /// A NUL-terminated string name.
    Name(Vec<u16>),
}

impl ResourceName {
    /// Copy a callback's `lpName`.
    ///
    /// # Safety
    /// `name` must be an integer resource id or a pointer to a
    /// NUL-terminated string that is readable for the duration of the call.
    unsafe fn from_callback(name: PCWSTR) -> Self {
        let raw = name.0 as usize;
        if raw >> 16 == 0 {
            return Self::Id(raw as u16);
        }
        // SAFETY: by this function's contract, `name` addresses a
        // NUL-terminated string for the duration of the callback.
        let wide = unsafe { name.as_wide() };
        Self::Name(wide.iter().copied().chain(std::iter::once(0)).collect())
    }

    /// The `lpName` to pass to `FindResourceW`; the pointer borrows `self`.
    fn as_pcwstr(&self) -> PCWSTR {
        match self {
            Self::Id(id) => PCWSTR(usize::from(*id) as *const u16),
            Self::Name(wide) => util::pcwstr(wide),
        }
    }
}

impl fmt::Display for ResourceName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Id(id) => write!(f, "#{id}"),
            Self::Name(wide) => write!(f, "\"{}\"", util::from_wide(wide)),
        }
    }
}

/// `PE resources` chain: the first `RT_GROUP_ICON` of the mapped file, its
/// best entry of at most `MAX_SIZE` created at its native size. `Ok(None)`
/// when the file has no icon group; `Err` for a file that cannot be mapped
/// (missing, unreadable, not a PE image), a malformed directory, or a
/// resource that cannot be read or decoded. The icon is owned and
/// destroyed here; the mapping is freed on return.
fn pe_resource_icon(path: &str) -> PlatformResult<Option<IconImage>> {
    let module = LoadedLibrary::open_as_data(path)?;
    let Some(group) = first_group_icon_name(&module) else {
        log::debug!("{path}: no RT_GROUP_ICON resource");
        return Ok(None);
    };
    let directory = read_resource(&module, RT_GROUP_ICON, &group)?;
    let entries = pixels::parse_group_icon_dir(&directory)
        .map_err(|e| PlatformError::Other(format!("icon group {group}: {e}")))?;
    let Some(entry) = pixels::select_group_icon_entry(&entries, MAX_SIZE) else {
        log::debug!("{path}: icon group {group} has no image of at most {MAX_SIZE} px");
        return Ok(None);
    };
    let bytes = read_resource(&module, RT_ICON, &ResourceName::Id(entry.id))?;
    // SAFETY: `bytes` is a whole RT_ICON resource (SizeofResource's length);
    // the function reads it and returns an icon we own, wrapped in a guard.
    let icon = unsafe {
        CreateIconFromResourceEx(
            &bytes,
            true,
            ICON_RESOURCE_VERSION,
            entry.width as i32,
            entry.height as i32,
            LR_DEFAULTCOLOR,
        )
    }?;
    let Some(icon) = OwnedIcon::new(icon) else {
        return Ok(None);
    };
    log::trace!(
        "{path}: icon group {group}: entry #{} {}x{} {}-bit, {} bytes",
        entry.id,
        entry.width,
        entry.height,
        entry.bit_count,
        entry.bytes_in_res
    );
    icon_to_image(icon.handle())
}

/// The name of the first `RT_GROUP_ICON` resource of a mapped module (the
/// one `ExtractIconW(.., 0)` and the shell use as the application icon), or
/// `None` when it has none.
fn first_group_icon_name(module: &LoadedLibrary) -> Option<ResourceName> {
    let mut found: Option<ResourceName> = None;
    let lparam = std::ptr::from_mut(&mut found) as isize;
    // SAFETY: `take_first_name` only writes to the Option addressed by
    // `lparam`, which outlives the call, and stops the enumeration after the
    // first name; the module handle is live for `module`'s lifetime.
    let completed = unsafe {
        EnumResourceNamesW(
            Some(module.handle()),
            RT_GROUP_ICON,
            Some(take_first_name),
            lparam,
        )
    };
    if found.is_none() && !completed.as_bool() {
        log::trace!("EnumResourceNamesW(RT_GROUP_ICON): {}", util::last_error());
    }
    found
}

/// `EnumResourceNamesW` callback: copy the first name into the `Option`
/// behind `lparam` and stop.
unsafe extern "system" fn take_first_name(
    _module: HMODULE,
    _kind: PCWSTR,
    name: PCWSTR,
    lparam: isize,
) -> BOOL {
    // SAFETY: `name` is valid for the duration of this callback, as the
    // enumeration contract guarantees.
    let copied = unsafe { ResourceName::from_callback(name) };
    let slot = lparam as *mut Option<ResourceName>;
    // SAFETY: `lparam` is the pointer `first_group_icon_name` passed, to an
    // Option that outlives the enumeration and is not aliased meanwhile.
    unsafe { *slot = Some(copied) };
    BOOL(0)
}

/// The bytes of resource `name` of type `kind`, copied out of the mapping
/// so they outlive it. Bounded by `MAX_RESOURCE_BYTES`.
fn read_resource(
    module: &LoadedLibrary,
    kind: PCWSTR,
    name: &ResourceName,
) -> PlatformResult<Vec<u8>> {
    // SAFETY: the module handle is live for `module`'s lifetime; `kind` is
    // an RT_* id and `name` an id or a NUL-terminated string owned by the
    // caller for the duration of the call.
    let info = unsafe { FindResourceW(Some(module.handle()), name.as_pcwstr(), kind) };
    if info.is_invalid() {
        return Err(util::last_error());
    }
    // SAFETY: `info` is a valid resource handle of this module.
    let size = unsafe { SizeofResource(Some(module.handle()), info) };
    if size == 0 || size > MAX_RESOURCE_BYTES {
        return Err(PlatformError::Other(format!(
            "resource {name} is {size} bytes"
        )));
    }
    // SAFETY: as above; for a module mapped as a data file the "global"
    // handle is a pointer into the mapping and needs no freeing.
    let data = unsafe { LoadResource(Some(module.handle()), info) }?;
    // SAFETY: `data` came from LoadResource on a live mapping.
    let ptr = unsafe { LockResource(data) };
    if ptr.is_null() {
        return Err(PlatformError::Other(format!(
            "resource {name} could not be locked"
        )));
    }
    // SAFETY: `ptr` addresses `size` readable bytes inside the mapping, which
    // `module` keeps alive until after this copy.
    let bytes = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), size as usize) };
    Ok(bytes.to_vec())
}

/// `IShellItemImageFactory::GetImage` on a parsing name (a path or
/// `shell:AppsFolder\<aumid>`), always for `MAX_SIZE` so that the caller's
/// `fit_within` does the downscale: the shell resamples to exactly the size
/// it is asked for (`SIIGBF_BIGGERSIZEOK` notwithstanding), and its filter
/// leaves a halo. `SIIGBF_ICONONLY` keeps thumbnails out. A source smaller
/// than `MAX_SIZE` is upscaled by the shell and accepted as it comes.
/// `E_PENDING` is retried once after `E_PENDING_RETRY`. The bitmap is
/// 32-bit premultiplied ARGB.
fn shell_item_image(parsing_name: &str) -> PlatformResult<Option<Loaded>> {
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

/// These run on Windows hosts only (this module is `cfg(windows)`) and use
/// files every Windows installation has. They exercise the `PE resources`
/// chain end to end (mapping, enumeration, directory parsing,
/// `CreateIconFromResourceEx`, `GetDIBits`); the shell chain is not tested
/// here because its answers depend on the shell's caches.
#[cfg(test)]
mod tests {
    use super::*;

    fn system_file(relative: &str) -> String {
        let root = std::env::var("SystemRoot").expect("SystemRoot is set on Windows");
        format!("{root}\\{relative}")
    }

    fn decode(png: &[u8]) -> (u32, u32) {
        let decoder = png::Decoder::new(std::io::Cursor::new(png));
        let reader = decoder.read_info().expect("png header");
        (reader.info().width, reader.info().height)
    }

    #[test]
    fn pe_resources_yield_the_native_image_of_system_binaries() {
        for relative in ["explorer.exe", "System32\\shell32.dll"] {
            let path = system_file(relative);
            let image = pe_resource_icon(&path)
                .unwrap_or_else(|e| panic!("{relative}: {e}"))
                .unwrap_or_else(|| panic!("{relative}: no icon group"));
            assert_eq!(image.width, image.height, "{relative}");
            assert!(
                (16..=256).contains(&image.edge()),
                "{relative}: {}x{}",
                image.width,
                image.height
            );
            let alphas: Vec<u8> = image.rgba.chunks_exact(4).map(|px| px[3]).collect();
            assert!(alphas.contains(&255), "{relative}: no opaque pixel");
            assert!(alphas.contains(&0), "{relative}: no transparent pixel");
        }
    }

    #[test]
    fn pe_resources_win_the_file_chain_and_the_downscale_happens_here() {
        let path = system_file("explorer.exe");
        let loaded = load_file_icon(&path, 48).expect("load").expect("icon");
        assert_eq!(loaded.chain, "PE resources");
        assert!(loaded.image.edge() >= 48, "source {}", loaded.image.edge());
        let fitted = loaded.image.fit_within(48);
        assert_eq!((fitted.width, fitted.height), (48, 48));
    }

    #[test]
    fn exe_key_png_is_exactly_the_requested_size() {
        let key = IconKey(format!("exe:{}", system_file("explorer.exe")));
        for size in [32, 48, 96, 192] {
            let png = load_icon_png(&key, size).expect("load").expect("png");
            assert_eq!(decode(&png), (size, size));
        }
    }

    #[test]
    fn pe_resources_reject_what_is_not_a_pe_image() {
        assert!(pe_resource_icon(&system_file("win.ini")).is_err());
        assert!(pe_resource_icon(&system_file("does-not-exist.exe")).is_err());
    }
}
