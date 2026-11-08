//! Window enumeration and eligibility.
//!
//! Contract:
//! - [`enumerate_taskbar_windows`] returns every top-level window that the
//!   stock taskbar would show, as [`RawWindow`] facts, in Z order (top first).
//! - [`query_window`] returns `Ok(None)` for windows that do not qualify;
//!   [`query_window_checked`] also says whether such a window may qualify
//!   later (`rules::recheck`, used by the events thread's re-checks).
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
//! - The process is opened once per window with
//!   `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)`; the handle serves both
//!   `QueryFullProcessImageNameW` (the image path) and
//!   `GetApplicationUserModelId` (the package application identity).
//!   Access denied (protected processes) yields `exe_path = None` and no
//!   process AUMID, never an error.
//! - Elevated and protected processes. Everything in the probe
//!   (`IsWindowVisible`, `GetClassNameW`, `GetWindowTextW`, the styles, the
//!   owner, the cloak flags, the frame) reads win32k state and is not
//!   subject to UIPI, so an elevated window is listed with its title and
//!   class like any other. The limited open is granted for an elevated
//!   process of the same user, so its `exe_path` and package identity are
//!   normally filled in and it groups, is named and gets its icon like its
//!   unelevated twin. Only a *protected* process (`csrss`, some antivirus
//!   UIs) refuses the open: then `exe_path` and the process AUMID are
//!   `None`, the engine groups the window by `class:<class>`, `identity`
//!   names it after its title and `icons` gets an `hwnd:` key. The refusal
//!   is logged once per process at debug (`rules::process_access`), not on
//!   every event about the window; the open itself is still attempted on
//!   each query, since a process id is reused once its process exits.
//! - `app_user_model_id` is the window's `System.AppUserModel.ID` property
//!   when set (UWP frames, apps that opt in), else the process's AUMID when
//!   the process runs with package identity (packaged Win32 apps such as the
//!   Windows 11 Notepad, Terminal, Paint; `APPMODEL_ERROR_NO_APPLICATION`
//!   for everything else), else `None`. `stem_core::app_id_for` then groups
//!   packaged apps by `aumid:` and `identity` names them from the AppsFolder.
//! - The property-store read needs a COM apartment on the calling thread
//!   (`util::ComApartment::sta`); the `PROPVARIANT` goes through
//!   `util::property_store_string` (shared with `identity`, so both accept
//!   the same variant types) and is always cleared.
//! - `icon_key` is `hwnd:<id>:<pid>` (`rules::identity::hwnd_icon_key`,
//!   resolved by `icons`); `monitor` comes from `monitors::monitor_for_window`.
//! - `rect` is [`window_frame`]: the DWM extended frame bounds, the
//!   visible frame without the invisible resize borders `GetWindowRect`
//!   includes on Windows 10+, with `GetWindowRect` as the fallback when DWM
//!   refuses. Physical pixels of the virtual desktop. It is read whenever
//!   the window is queried. The events thread hooks location
//!   changes of every top-level window (`EVENT_OBJECT_LOCATIONCHANGE`) and
//!   re-queries each tracked window that moved, frame included, once per
//!   150 ms burst (`windows::events`, `LOCATION_SETTLE`), reporting it only
//!   when its facts changed; moves of the foreground window also
//!   re-evaluate fullscreen (`windows::fullscreen`).
//! - `on_current_desktop` is the shell's answer to
//!   `IVirtualDesktopManager::IsWindowOnCurrentVirtualDesktop`
//!   (`windows::virtual_desktop`), asked for every eligible window through
//!   the calling thread's manager: the events thread has one; a thread
//!   without one, or a window the shell refuses, gives `None`, and the
//!   engine falls back to the cloak state. A full enumeration renews the
//!   manager; a single query reuses it. Eligibility itself does not look
//!   at it: a shell-cloaked window stays listed, `is_cloaked = true`.

use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use stem_core::{RawWindow, Rect};
use windows::Win32::Foundation::{
    APPMODEL_ERROR_NO_APPLICATION, ERROR_INSUFFICIENT_BUFFER, HWND, LPARAM, RECT,
};
use windows::Win32::Graphics::Dwm::{
    DWM_CLOAKED_APP, DWM_CLOAKED_INHERITED, DWM_CLOAKED_SHELL, DWMWA_CLOAKED,
    DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::Storage::Packaging::Appx::{
    APPLICATION_USER_MODEL_ID_MAX_LENGTH, GetApplicationUserModelId,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, PROCESS_NAME_WIN32, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GA_ROOT, GW_OWNER, GWL_EXSTYLE, GetAncestor, GetClassNameW, GetWindow,
    GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, WINDOW_EX_STYLE,
    WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, PWSTR};

use super::{monitors, util, virtual_desktop};
use crate::rules::eligibility::{self, Owner, WindowFacts};
use crate::rules::identity::hwnd_icon_key;
use crate::rules::process_access::DeniedProcesses;
use crate::rules::recheck::{self, Check};
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
/// Largest AUMID buffer we grow to on `ERROR_INSUFFICIENT_BUFFER` (the
/// documented maximum is `APPLICATION_USER_MODEL_ID_MAX_LENGTH`, 130).
const AUMID_BUF_MAX: usize = 1024;

/// Processes whose `OpenProcess` refusal has been logged, shared by
/// every thread that queries windows (events, identity, icons) so the line
/// appears once per process whichever of them meets the window first.
static DENIED_PROCESSES: Mutex<DeniedProcesses> = Mutex::new(DeniedProcesses::new());

pub fn enumerate_taskbar_windows() -> PlatformResult<Vec<RawWindow>> {
    let mut handles: Vec<HWND> = Vec::new();
    let lparam = LPARAM(std::ptr::from_mut(&mut handles) as isize);
    // SAFETY: `collect_window` only pushes onto the Vec addressed by `lparam`,
    // which outlives the (synchronous) enumeration.
    unsafe { EnumWindows(Some(collect_window), lparam) }?;

    let ctx = QueryContext::renewed();
    let mut out = Vec::new();
    for hwnd in &handles {
        if let Some(raw) = ctx.query(*hwnd) {
            out.push(raw);
        }
    }
    log::debug!(
        "enumerated {} taskbar windows out of {} top-level windows; virtual desktop facts {}",
        out.len(),
        handles.len(),
        if ctx.desktops.is_available() {
            "asked"
        } else {
            "unavailable on this thread"
        }
    );
    Ok(out)
}

pub fn query_window(hwnd: HWND) -> PlatformResult<Option<RawWindow>> {
    Ok(QueryContext::new().query(hwnd))
}

/// [`query_window`] that also says how a window that does not qualify
/// stands for the re-check schedule: `Err` is `Check::Transient`,
/// `Check::Final` or `Check::Gone`, never `Check::Eligible`.
pub fn query_window_checked(hwnd: HWND) -> Result<RawWindow, Check> {
    QueryContext::new().query_checked(hwnd)
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

/// Per-call context: our own pid, a COM apartment for the property-store
/// read, and the thread's virtual desktop manager, initialised once
/// per enumeration instead of once per window. Field order is drop order:
/// the manager reference goes before the apartment.
struct QueryContext {
    own_pid: u32,
    desktops: virtual_desktop::DesktopQuery,
    _com: util::ComApartment,
}

impl QueryContext {
    /// For one window: the thread's manager as it is.
    fn new() -> Self {
        Self::with(virtual_desktop::DesktopQuery::for_this_thread)
    }

    /// For a full enumeration: a fresh manager on a thread that has one.
    fn renewed() -> Self {
        Self::with(virtual_desktop::DesktopQuery::renewed)
    }

    fn with(desktops: fn() -> virtual_desktop::DesktopQuery) -> Self {
        // The apartment first: the manager is created inside it.
        let com = util::ComApartment::sta();
        Self {
            // SAFETY: GetCurrentProcessId has no preconditions.
            own_pid: unsafe { GetCurrentProcessId() },
            desktops: desktops(),
            _com: com,
        }
    }

    /// Full facts for an eligible window, `None` otherwise.
    fn query(&self, hwnd: HWND) -> Option<RawWindow> {
        self.query_checked(hwnd).ok()
    }

    /// Full facts for an eligible window, else how it stands
    /// (`rules::recheck::classify`; `Gone` for a dead handle).
    fn query_checked(&self, hwnd: HWND) -> Result<RawWindow, Check> {
        let probe = probe(hwnd, self.own_pid).ok_or(Check::Gone)?;
        let check = recheck::classify(&probe.facts);
        if check != Check::Eligible {
            log::trace!(
                "window {:?} ({:?}) rejected: {check:?}",
                hwnd.0,
                probe.facts.class_name
            );
            return Err(check);
        }
        let id = util::window_id(hwnd);
        let is_cloaked = probe.facts.is_cloaked();
        let process_id = probe.facts.process_id;
        // One handle for everything the process can tell us.
        let process = open_process(process_id);
        let exe_path = process
            .as_ref()
            .and_then(|h| process_image_path(h, process_id));
        let app_user_model_id = read_app_user_model_id(hwnd).or_else(|| {
            process
                .as_ref()
                .and_then(|h| process_app_user_model_id(h, process_id))
        });
        Ok(RawWindow {
            id,
            title: probe.facts.title,
            class_name: probe.facts.class_name,
            process_id,
            exe_path,
            app_user_model_id,
            is_visible: probe.facts.is_visible,
            is_minimized: probe.is_minimized,
            is_cloaked,
            on_current_desktop: self.desktops.is_window_on_current_desktop(hwnd),
            monitor: monitors::monitor_for_window(hwnd),
            rect: window_frame(hwnd),
            icon_key: Some(hwnd_icon_key(id.0, probe.facts.process_id)),
        })
    }
}

/// The window's frame in screen coordinates (physical pixels): the DWM
/// extended frame bounds, else `GetWindowRect` when DWM refuses (desktop
/// composition off, some cloaked windows). `None` when both fail, which
/// means a dead handle. Reads cached state only; nothing is sent to the
/// window's thread.
pub(super) fn window_frame(hwnd: HWND) -> Option<Rect> {
    let mut rect = RECT::default();
    let out = std::ptr::from_mut(&mut rect).cast::<core::ffi::c_void>();
    // SAFETY: `out` addresses a RECT and the size passed is exactly that of a RECT.
    let dwm = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            out,
            size_of::<RECT>() as u32,
        )
    };
    if dwm.is_err() {
        // SAFETY: `rect` is a live out-pointer for the duration of the call.
        if let Err(e) = unsafe { GetWindowRect(hwnd, &mut rect) } {
            log::trace!("no frame for window {:#x}: {e}", hwnd.0 as isize);
            return None;
        }
    }
    Some(Rect::new(
        rect.left,
        rect.top,
        rect.right - rect.left,
        rect.bottom - rect.top,
    ))
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

/// `GetClassNameW`; empty for a dead handle. Shared with `fullscreen`.
pub(super) fn class_name(hwnd: HWND) -> String {
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
/// text and cannot block on a hung application (see the module notes); do
/// not call it for windows of our own process. Shared with `fullscreen`.
pub(super) fn window_title(hwnd: HWND) -> String {
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

/// A limited-query handle to a process, or `None` when it cannot be opened
/// (protected or exited process). Never an error: identity degrades to
/// title and class. A refusal is logged at debug once per process
/// ([`DENIED_PROCESSES`]) and at trace afterwards, so a protected process's
/// window, queried again on every event about it, does not fill the log.
fn open_process(process_id: u32) -> Option<util::ProcessHandle> {
    match util::ProcessHandle::open_limited(process_id) {
        Ok(h) => {
            denied_processes().clear(process_id);
            Some(h)
        }
        Err(PlatformError::AccessDenied) => {
            if denied_processes().note(process_id) {
                log::debug!(
                    "process {process_id}: access denied, no image path or AUMID (its windows \
                     keep title, class and `class:` identity; logged once per process)"
                );
            } else {
                log::trace!("process {process_id}: access denied (already logged)");
            }
            None
        }
        Err(e) => {
            log::debug!("process {process_id}: OpenProcess failed: {e}");
            None
        }
    }
}

/// The shared refusal memory; a poisoned lock (a panic while held, which
/// nothing here can cause) still yields the set.
fn denied_processes() -> std::sync::MutexGuard<'static, DeniedProcesses> {
    DENIED_PROCESSES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Full image path of a process through an open handle, or `None` when it
/// cannot be read.
fn process_image_path(handle: &util::ProcessHandle, process_id: u32) -> Option<PathBuf> {
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

/// The AppUserModelID of a process that runs with package identity
/// (`GetApplicationUserModelId`), or `None` for a plain desktop process
/// (`APPMODEL_ERROR_NO_APPLICATION`) and on any other failure. A buffer
/// too small is retried once with the length the API asks for, within
/// [`AUMID_BUF_MAX`]. Sends no messages; cannot block on a hung target.
fn process_app_user_model_id(handle: &util::ProcessHandle, process_id: u32) -> Option<String> {
    let mut buf = vec![0u16; APPLICATION_USER_MODEL_ID_MAX_LENGTH as usize];
    loop {
        let mut len = buf.len() as u32;
        // SAFETY: `buf` is writable for `len` UTF-16 units and `len` is a
        // live in/out pointer; the handle is open for the guard's lifetime.
        let err = unsafe {
            GetApplicationUserModelId(handle.raw(), &mut len, Some(PWSTR(buf.as_mut_ptr())))
        };
        if err.is_ok() {
            let aumid = util::from_wide(&buf[..(len as usize).min(buf.len())]);
            let aumid = aumid.trim();
            return (!aumid.is_empty()).then(|| aumid.to_owned());
        }
        let needed = len as usize;
        if err == ERROR_INSUFFICIENT_BUFFER && needed > buf.len() && needed <= AUMID_BUF_MAX {
            buf.resize(needed, 0);
            continue;
        }
        if err == APPMODEL_ERROR_NO_APPLICATION {
            log::trace!("process {process_id}: no package identity");
        } else {
            log::trace!("process {process_id}: GetApplicationUserModelId failed: {err:?}");
        }
        return None;
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
    util::property_store_string(&store, &PKEY_AppUserModel_ID)
}
