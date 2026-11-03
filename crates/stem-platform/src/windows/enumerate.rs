//! Window enumeration and eligibility.
//!
//! Contract:
//! - [`enumerate_taskbar_windows`] returns every top-level window that the
//!   stock taskbar would show, as [`RawWindow`] facts, in Z order (top first).
//! - [`query_window`] returns `Ok(None)` for windows that do not qualify.
//! - [`is_taskbar_candidate`] is the pure eligibility rule (visible, not a
//!   tool window, no owner unless `WS_EX_APPWINDOW`, not cloaked unless we
//!   want to report it, has a title or is a known UWP frame, ...).
//! - [`window_app_user_model_id`] reads `PKEY_AppUserModel_ID` from the
//!   window's property store (used for grouping and Store apps).
//!
//! All calls must be bounded: use `SendMessageTimeoutW`, never `SendMessageW`.
//!
//! Implementation notes:
//! - The decision itself lives in `rules::eligibility` (pure, unit-tested);
//!   this module only gathers [`WindowFacts`] with `IsWindowVisible`,
//!   `GetWindowLongPtrW(GWL_EXSTYLE)`, `GetWindow(GW_OWNER)`,
//!   `GetAncestor(GA_ROOT)`, `GetClassNameW`, `GetWindowTextW`,
//!   `GetWindowThreadProcessId` and `DwmGetWindowAttribute(DWMWA_CLOAKED)`.
//! - No window message is ever sent to another process here. `GetWindowTextW`
//!   / `GetWindowTextLengthW` read the cached caption of windows that belong
//!   to other processes without sending `WM_GETTEXT`, so a hung app cannot
//!   stall enumeration. Windows of our own process (where those calls *would*
//!   send a message) are rejected before the title is read.
//! - The process image path comes from `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)`
//!   and `QueryFullProcessImageNameW`; access denied (protected processes)
//!   yields `exe_path = None`, never an error.
//! - The AUMID read needs a COM apartment on the calling thread
//!   (`util::ComApartment::sta`); the `PROPVARIANT` is read only when its
//!   type is `VT_LPWSTR` and is always cleared.
//! - `icon_key` is `hwnd:<id>` (resolved by `icons`); `monitor` comes from
//!   `monitors::monitor_for_window`.

use std::path::PathBuf;

use stem_core::{IconKey, RawWindow};
use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{
    DWM_CLOAKED_APP, DWM_CLOAKED_INHERITED, DWM_CLOAKED_SHELL, DWMWA_CLOAKED, DwmGetWindowAttribute,
};
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::System::Com::StructuredStorage::{
    PROPVARIANT, PROPVARIANT_0_0, PropVariantClear,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, PROCESS_NAME_WIN32, QueryFullProcessImageNameW,
};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GA_ROOT, GW_OWNER, GWL_EXSTYLE, GetAncestor, GetClassNameW, GetWindow,
    GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindow, IsWindowVisible, WINDOW_EX_STYLE, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, PWSTR};

use super::{monitors, util};
use crate::rules::eligibility::{self, Owner, WindowFacts};
use crate::{PlatformError, PlatformResult};

// The pure rule table mirrors dwmapi.h; prove it against the SDK.
const _: () = {
    assert!(eligibility::CLOAKED_APP == DWM_CLOAKED_APP);
    assert!(eligibility::CLOAKED_SHELL == DWM_CLOAKED_SHELL);
    assert!(eligibility::CLOAKED_INHERITED == DWM_CLOAKED_INHERITED);
};

/// Longest window title we copy; longer captions are truncated.
const MAX_TITLE_CHARS: usize = 1024;
/// `GetClassNameW` caps class names at 256 characters.
const MAX_CLASS_CHARS: usize = 256;
/// Initial / maximum buffer for `QueryFullProcessImageNameW` (NT path limit).
const PATH_BUF_INITIAL: usize = 1024;
const PATH_BUF_MAX: usize = 32 * 1024;

pub fn enumerate_taskbar_windows() -> PlatformResult<Vec<RawWindow>> {
    let mut handles: Vec<HWND> = Vec::new();
    let lparam = LPARAM(std::ptr::from_mut(&mut handles) as isize);
    // SAFETY: `collect_window` only pushes onto the Vec addressed by `lparam`,
    // which outlives the (synchronous) enumeration.
    unsafe { EnumWindows(Some(collect_window), lparam) }?;

    let ctx = QueryContext::new();
    let mut out = Vec::new();
    for hwnd in &handles {
        if let Some(raw) = ctx.query(*hwnd) {
            out.push(raw);
        }
    }
    log::debug!(
        "enumerated {} taskbar windows out of {} top-level windows",
        out.len(),
        handles.len()
    );
    Ok(out)
}

pub fn query_window(hwnd: HWND) -> PlatformResult<Option<RawWindow>> {
    Ok(QueryContext::new().query(hwnd))
}

pub fn is_taskbar_candidate(hwnd: HWND) -> bool {
    // SAFETY: GetCurrentProcessId has no preconditions.
    let own_pid = unsafe { GetCurrentProcessId() };
    probe(hwnd, own_pid).is_some_and(|p| eligibility::is_taskbar_candidate(&p.facts))
}

pub fn window_app_user_model_id(hwnd: HWND) -> Option<String> {
    let _com = util::ComApartment::sta();
    read_app_user_model_id(hwnd)
}

unsafe extern "system" fn collect_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let list = lparam.0 as *mut Vec<HWND>;
    // SAFETY: `lparam` is the pointer `enumerate_taskbar_windows` passed to
    // EnumWindows; the Vec is alive and not otherwise borrowed while the
    // enumeration runs on this thread.
    let list = unsafe { &mut *list };
    list.push(hwnd);
    BOOL(1)
}

/// Per-call context: our own pid and a COM apartment for the property-store
/// read, initialised once per enumeration instead of once per window.
struct QueryContext {
    own_pid: u32,
    _com: util::ComApartment,
}

impl QueryContext {
    fn new() -> Self {
        Self {
            // SAFETY: GetCurrentProcessId has no preconditions.
            own_pid: unsafe { GetCurrentProcessId() },
            _com: util::ComApartment::sta(),
        }
    }

    /// Full facts for an eligible window, `None` otherwise.
    fn query(&self, hwnd: HWND) -> Option<RawWindow> {
        let probe = probe(hwnd, self.own_pid)?;
        if let Err(reason) = eligibility::evaluate(&probe.facts) {
            log::trace!(
                "window {:?} ({:?}) rejected: {reason:?}",
                hwnd.0,
                probe.facts.class_name
            );
            return None;
        }
        let id = util::window_id(hwnd);
        let is_cloaked = probe.facts.is_cloaked();
        Some(RawWindow {
            id,
            title: probe.facts.title,
            class_name: probe.facts.class_name,
            process_id: probe.facts.process_id,
            exe_path: process_image_path(probe.facts.process_id),
            app_user_model_id: read_app_user_model_id(hwnd),
            is_visible: probe.facts.is_visible,
            is_minimized: probe.is_minimized,
            is_cloaked,
            monitor: monitors::monitor_for_window(hwnd),
            icon_key: Some(IconKey(format!("hwnd:{}", id.0))),
        })
    }
}

/// The cheap facts (no process handle, no COM).
struct Probe {
    facts: WindowFacts,
    is_minimized: bool,
}

/// Gather [`WindowFacts`] for a live window; `None` when the handle is dead.
fn probe(hwnd: HWND, own_pid: u32) -> Option<Probe> {
    // SAFETY: IsWindow accepts any handle value.
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return None;
    }
    let mut process_id = 0u32;
    // SAFETY: `process_id` is a live out-pointer for the duration of the call.
    let thread_id = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    if thread_id == 0 {
        // The window vanished between IsWindow and here.
        return None;
    }
    let mut facts = WindowFacts {
        process_id,
        own_process_id: own_pid,
        ..WindowFacts::default()
    };
    if process_id == own_pid {
        // Our own windows never qualify; stop before anything that would
        // send WM_GETTEXT to ourselves.
        return Some(Probe {
            facts,
            is_minimized: false,
        });
    }

    // SAFETY: GetAncestor accepts any handle value and returns NULL for dead ones.
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    facts.is_root = root == hwnd;
    // SAFETY: IsWindowVisible accepts any handle value.
    facts.is_visible = unsafe { IsWindowVisible(hwnd) }.as_bool();
    facts.class_name = class_name(hwnd);
    // SAFETY: GetWindowLongPtrW reads a cached window property; 0 is both a
    // valid style and the failure value, and a zero style is fine either way.
    let ex_style = WINDOW_EX_STYLE(unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32);
    facts.is_tool_window = ex_style.contains(WS_EX_TOOLWINDOW);
    facts.is_app_window = ex_style.contains(WS_EX_APPWINDOW);
    facts.owner = owner_of(hwnd);
    facts.cloak_flags = cloak_flags(hwnd);
    facts.title = window_title(hwnd);
    // SAFETY: IsIconic accepts any handle value.
    let is_minimized = unsafe { IsIconic(hwnd) }.as_bool();
    Some(Probe {
        facts,
        is_minimized,
    })
}

fn owner_of(hwnd: HWND) -> Owner {
    // SAFETY: GetWindow reads the owner relation; it fails (Err) when there is none.
    let owner = unsafe { GetWindow(hwnd, GW_OWNER) }
        .ok()
        .filter(|h| !h.is_invalid());
    match owner {
        None => Owner::None,
        // SAFETY: IsWindowVisible accepts any handle value.
        Some(owner) if unsafe { IsWindowVisible(owner) }.as_bool() => Owner::Visible,
        Some(_) => Owner::Hidden,
    }
}

/// `DWMWA_CLOAKED` flags; `0` when DWM refuses (e.g. not composited).
fn cloak_flags(hwnd: HWND) -> u32 {
    let mut flags = 0u32;
    let out = std::ptr::from_mut(&mut flags).cast::<core::ffi::c_void>();
    // SAFETY: `out` addresses a u32 and the size passed is exactly that of a u32.
    match unsafe { DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, out, size_of::<u32>() as u32) } {
        Ok(()) => flags,
        Err(_) => 0,
    }
}

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; MAX_CLASS_CHARS];
    // SAFETY: `buf` is a writable slice; the API writes at most its length
    // (including the NUL) and returns the number of characters copied.
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..(len as usize).min(buf.len())])
}

/// The window caption. For windows of other processes this reads the cached
/// text and cannot block on a hung application (see the module notes).
fn window_title(hwnd: HWND) -> String {
    // SAFETY: GetWindowTextLengthW accepts any handle value.
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    if len <= 0 {
        return String::new();
    }
    let capacity = (len as usize + 1).min(MAX_TITLE_CHARS);
    let mut buf = vec![0u16; capacity];
    // SAFETY: `buf` is a writable slice; the API writes at most its length
    // (including the NUL) and returns the number of characters copied.
    let copied = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if copied <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..(copied as usize).min(buf.len())])
}

/// Full image path of a process, or `None` when it cannot be read (protected
/// or exited process). Never an error: identity degrades to title and class.
pub(crate) fn process_image_path(process_id: u32) -> Option<PathBuf> {
    let handle = match util::ProcessHandle::open_limited(process_id) {
        Ok(h) => h,
        Err(PlatformError::AccessDenied) => {
            log::debug!("process {process_id}: access denied, no image path");
            return None;
        }
        Err(e) => {
            log::debug!("process {process_id}: OpenProcess failed: {e}");
            return None;
        }
    };
    let mut buf = vec![0u16; PATH_BUF_INITIAL];
    loop {
        let mut len = buf.len() as u32;
        // SAFETY: `buf` is writable for `len` UTF-16 units and `len` is a
        // live in/out pointer; the handle is open for the guard's lifetime.
        let result = unsafe {
            QueryFullProcessImageNameW(
                handle.raw(),
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
        };
        match result {
            Ok(()) => {
                let path = String::from_utf16_lossy(&buf[..(len as usize).min(buf.len())]);
                return (!path.is_empty()).then(|| PathBuf::from(path));
            }
            Err(e)
                if e.code() == util::hresult_of(ERROR_INSUFFICIENT_BUFFER)
                    && buf.len() < PATH_BUF_MAX =>
            {
                buf.resize(PATH_BUF_MAX, 0);
            }
            Err(e) => {
                log::debug!("process {process_id}: QueryFullProcessImageNameW failed: {e}");
                return None;
            }
        }
    }
}

/// `PKEY_AppUserModel_ID` from the window's property store. The caller must
/// hold a COM apartment on this thread.
fn read_app_user_model_id(hwnd: HWND) -> Option<String> {
    // SAFETY: SHGetPropertyStoreForWindow accepts any handle value and
    // reports invalid ones through the Result; the interface is owned by us.
    let store: IPropertyStore = match unsafe { SHGetPropertyStoreForWindow(hwnd) } {
        Ok(store) => store,
        Err(e) => {
            log::trace!("no property store for window {:?}: {e}", hwnd.0);
            return None;
        }
    };
    // SAFETY: the key is a static PROPERTYKEY; the returned PROPVARIANT is
    // owned by us and cleared below.
    let mut value: PROPVARIANT = match unsafe { store.GetValue(&PKEY_AppUserModel_ID) } {
        Ok(value) => value,
        Err(e) => {
            log::trace!("PKEY_AppUserModel_ID read failed for {:?}: {e}", hwnd.0);
            return None;
        }
    };
    let aumid = propvariant_lpwstr(&value);
    // SAFETY: `value` was produced by GetValue and is cleared exactly once.
    if let Err(e) = unsafe { PropVariantClear(&mut value) } {
        log::debug!("PropVariantClear failed: {e}");
    }
    aumid.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

/// The string payload of a `VT_LPWSTR` `PROPVARIANT`; `None` for any other
/// type (including `VT_EMPTY`, the usual answer for windows without an AUMID).
fn propvariant_lpwstr(value: &PROPVARIANT) -> Option<String> {
    // SAFETY: a PROPVARIANT produced by COM always uses the tagged layout
    // (`vt` + payload union), never the DECIMAL overlay, so this member is
    // the initialised one.
    let tagged: &PROPVARIANT_0_0 = unsafe { &value.Anonymous.Anonymous };
    if tagged.vt != VT_LPWSTR {
        return None;
    }
    // SAFETY: vt == VT_LPWSTR guarantees `pwszVal` is the active member.
    let ptr = unsafe { tagged.Anonymous.pwszVal };
    if ptr.is_null() {
        return None;
    }
    // SAFETY: a non-null VT_LPWSTR payload is a NUL-terminated UTF-16 string
    // owned by the PROPVARIANT, which outlives this read.
    unsafe { ptr.to_string() }.ok()
}
