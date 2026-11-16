//! Window enumeration and eligibility.
//!
//! Contract:
//! - [`enumerate_taskbar_windows`] returns every top-level window that the
//!   stock taskbar would show, as [`RawWindow`] facts, in Z order (top first).
//! - [`query_window`] returns `Ok(None)` for windows that do not qualify;
//!   [`query_window_checked`] also says whether such a window may qualify
//!   later (`rules::recheck`, used by the events thread's re-checks);
//!   [`query_moved_window`] is the same for a window that moved, with its
//!   desktop answer supplied instead of asked.
//! - [`is_taskbar_candidate`] is the pure eligibility rule (visible; not a
//!   tool window, not `WS_EX_NOACTIVATE` and unowned, unless
//!   `WS_EX_APPWINDOW`; not cloaked unless we want to report it; has a
//!   title or is a known UWP frame, ...).
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
//!   without one, a window the shell refuses, or a query during the pause
//!   that follows a slow call gives `None`, and the engine falls back to
//!   the cloak state. A full enumeration renews the manager; a single query
//!   reuses it; [`query_moved_window`] (the move path) does not ask at all
//!   and reuses the answer last reported about the window, since a move
//!   does not change the desktop. Eligibility itself does not look at it: a
//!   shell-cloaked window stays listed, `is_cloaked = true`.
//!
//! [`WindowFacts`]: crate::rules::eligibility::WindowFacts

mod facts;
mod process;
mod query;

use stem_core::RawWindow;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
use windows::Win32::UI::WindowsAndMessaging::EnumWindows;
use windows::core::BOOL;

use super::util;
use crate::PlatformResult;
use crate::rules::eligibility;
use crate::rules::recheck::Check;
use facts::probe;
pub(super) use facts::{class_name, window_frame, window_title};
use query::{DesktopSource, QueryContext};

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
        ctx.desktop_facts()
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

/// [`query_window_checked`] for a window that moved or resized (the events
/// thread's move path): `on_current_desktop` is the answer last
/// reported about the window and goes into the result as it is; the shell
/// is not asked (a move does not change the desktop, and the call
/// into Explorer has no timeout).
pub fn query_moved_window(
    hwnd: HWND,
    on_current_desktop: Option<bool>,
) -> Result<RawWindow, Check> {
    QueryContext::with(|| DesktopSource::Known(on_current_desktop)).query_checked(hwnd)
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
