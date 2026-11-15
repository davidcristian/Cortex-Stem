//! The native backdrop window under a dock window (prototype:
//! `docs/research/backdrop/backdrop-window.md`).
//!
//! Contract:
//! - [`supported`] decides once per process whether the compositor has
//!   `DWMWA_SYSTEMBACKDROP_TYPE`: Windows 11 build [`MIN_BUILD`] (22621) or
//!   later, as `CurrentBuildNumber` under
//!   `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion` says. Below it one
//!   `info` line says so and `Platform::set_dock_backdrop` does nothing; an
//!   unreadable build number is logged at debug and the window is tried.
//! - [`BackdropThread::start`] spawns `stem-platform-backdrop`, which owns
//!   every backdrop window and runs a `GetMessageW` loop. It returns at once:
//!   the caller may be a Tauri command handler on the main thread, so it
//!   never waits for the thread, and a request made before the thread is up
//!   is applied as soon as it is.
//! - [`BackdropThread::set`] records the latest wanted state of one dock
//!   window's backdrop and wakes the thread; requests collapse (the latest
//!   wins). `None` hides the window; it is destroyed with the thread, or
//!   when its dock window is gone.
//! - [`restack`] is what the dock window's subclass calls (on the dock's
//!   thread) after every z-order change of the dock and after the topmost
//!   re-assert: it posts, so it never waits on this thread.
//! - [`BackdropThread::stop`] posts `WM_QUIT` and joins (bounded); the
//!   windows are destroyed on their own thread.
//!
//! # The window ("What the window needs" in the research note)
//!
//! - Styles: `WS_POPUP | WS_CAPTION` (DWM is reported to ignore the
//!   backdrop type on a frameless popup, A3) with a `WM_NCCALCSIZE` answer
//!   that gives the whole window to the client area, and `WS_EX_TOOLWINDOW
//!   | WS_EX_NOACTIVATE | WS_EX_TOPMOST`. The class paints the client area
//!   black, the "sheet of glass" recipe of `DwmExtendFrameIntoClientArea`
//!   with −1 margins, which the window calls (A2).
//! - Attributes: `DWMWA_SYSTEMBACKDROP_TYPE` (`DWMSBT_TRANSIENTWINDOW` for
//!   Acrylic, `DWMSBT_MAINWINDOW` for Mica), `DWMWA_USE_IMMERSIVE_DARK_MODE`
//!   from [`DockBackdrop::dark`], `DWMWA_BORDER_COLOR = DWMWA_COLOR_NONE`,
//!   `DWMWA_TRANSITIONS_FORCEDISABLED = TRUE`,
//!   `DWMWA_WINDOW_CORNER_PREFERENCE = DWMWCP_DONOTROUND`.
//! - Activation: `WM_NCACTIVATE` is always forwarded to `DefWindowProcW` as
//!   `TRUE`, and the window gets one `WM_NCACTIVATE(TRUE)` after creation,
//!   so DWM draws the live material on a window that is never active (A6).
//! - Input: `WS_EX_LAYERED | WS_EX_TRANSPARENT` with
//!   `SetLayeredWindowAttributes(alpha 255)`: mouse input passes through to
//!   whatever is beneath (A4). `WM_MOUSEACTIVATE → MA_NOACTIVATE` and
//!   `WM_NCHITTEST → HTTRANSPARENT` cover the fallback below.
//! - Z-order: unowned (an owned window is always above its owner), and
//!   always placed with `hWndInsertAfter` = the dock window, never with
//!   `HWND_TOP` / `HWND_TOPMOST`, and shown the same way (`SWP_SHOWWINDOW`),
//!   never with `ShowWindow`. [`restack`] puts it back under the dock when
//!   it is not the window right below it (`GetWindow(GW_HWNDNEXT)`).
//! - Shape: a `CreateRoundRectRgn` region inset by [`REGION_INSET`] px, with
//!   corners concentric to the panel's ([`region_args`]), and DWM's own
//!   rounding off (never both, A7).
//! - Size and place: [`DockBackdrop::rect`] as given (the app layer rounds
//!   it inwards); `WM_GETMINMAXINFO` lowers the minimum tracking size a
//!   captioned window would otherwise have.
//!
//! Two switches for testing, read once when the thread
//! starts and named in the `applied` log line: `STEM_BACKDROP_LAYERED=0`
//! creates the window without `WS_EX_LAYERED | WS_EX_TRANSPARENT` (the
//! research's fallback for A4), `STEM_BACKDROP_EXTEND_FRAME=0` skips
//! `DwmExtendFrameIntoClientArea` (A2). Any other value keeps the default.
//!
//! # Why a thread of its own
//!
//! The research note planned the window on the main thread, next to the
//! dock window. Nothing requires that: `SetWindowPos` with a window of
//! another thread as `hWndInsertAfter` moves only the backdrop, and every
//! cross-thread signal here is a posted thread message, so neither thread
//! ever waits on the other (docs/development/windows-rules.md). The price is
//! that a move of the dock window and the backdrop's following move are not
//! one synchronous step; at rest that is invisible.
//!
//! Thread safety: the pending requests live in a `Mutex` shared between the
//! platform object and the thread; the windows, the class and every raw
//! handle stay on the thread; only `isize` handle values, the plain
//! [`DockBackdrop`] values and the thread id cross threads.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded};
use parking_lot::Mutex;
use stem_core::Rect;
use windows::Win32::Foundation::{
    COLORREF, ERROR_CLASS_ALREADY_EXISTS, ERROR_SUCCESS, HINSTANCE, HWND, LPARAM, LRESULT, POINT,
    WPARAM,
};
use windows::Win32::Graphics::Dwm::{
    DWMSBT_MAINWINDOW, DWMSBT_TRANSIENTWINDOW, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE,
    DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_TRANSITIONS_FORCEDISABLED, DWMWA_USE_IMMERSIVE_DARK_MODE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWMWINDOWATTRIBUTE,
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    BLACK_BRUSH, CreateRectRgn, CreateRoundRectRgn, DeleteObject, GetStockObject, HBRUSH, HRGN,
    SetWindowRgn,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GW_HWNDNEXT, GetMessageW,
    GetWindow, HTTRANSPARENT, IsWindow, LWA_ALPHA, MA_NOACTIVATE, MINMAXINFO, MSG, PM_NOREMOVE,
    PM_REMOVE, PeekMessageW, PostThreadMessageW, RegisterClassExW, SWP_FRAMECHANGED,
    SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
    SendMessageW, SetLayeredWindowAttributes, SetWindowPos, TranslateMessage, UnregisterClassW,
    WINDOW_EX_STYLE, WM_APP, WM_DPICHANGED, WM_GETMINMAXINFO, WM_MOUSEACTIVATE, WM_NCACTIVATE,
    WM_NCCALCSIZE, WM_NCHITTEST, WM_QUIT, WM_USER, WNDCLASSEXW, WS_CAPTION, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::w;

use super::util;
use crate::{BackdropMaterial, DockBackdrop, NativeWindowHandle, PlatformError, PlatformResult};

/// The first build with `DWMWA_SYSTEMBACKDROP_TYPE` (`DWM_SYSTEMBACKDROP_TYPE`
/// documentation: Windows 11 version 22H2).
pub const MIN_BUILD: u32 = 22621;

/// How far the window region stays inside the window on every side, so the
/// region's stair steps sit under the panel's 1 px hairline.
pub const REGION_INSET: i32 = 1;

/// Thread message: apply the pending requests.
const MSG_APPLY: u32 = WM_APP + 0x212;
/// Thread message: put the backdrop of the dock window in `wParam` back
/// under it, if it is not there.
const MSG_RESTACK: u32 = WM_APP + 0x213;

const CLASS_NAME: &str = "StemDockBackdrop";
const WINDOW_TITLE: &str = "Stem backdrop";
const STOP_TIMEOUT: Duration = Duration::from_secs(2);

/// The running backdrop thread's id; 0 while there is none (before it has a
/// message queue, and after it ended). One backdrop thread per process.
static THREAD_ID: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// Build check
// ---------------------------------------------------------------------------

/// Whether this Windows build has `DWMWA_SYSTEMBACKDROP_TYPE` (see the module
/// docs). Decided and logged once per process.
pub fn supported() -> bool {
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(|| match os_build() {
        Some(build) if build >= MIN_BUILD => {
            log::debug!("Windows build {build}: DWMWA_SYSTEMBACKDROP_TYPE available");
            true
        }
        Some(build) => {
            log::info!(
                "native backdrop refused: Windows build {build} has no DWMWA_SYSTEMBACKDROP_TYPE (it needs build {MIN_BUILD}); the panel keeps its CSS look"
            );
            false
        }
        None => {
            log::debug!("the Windows build number could not be read; trying the native backdrop");
            true
        }
    })
}

/// `CurrentBuildNumber` from the registry; `None` when it cannot be read.
fn os_build() -> Option<u32> {
    let mut buf = [0u16; 32];
    let mut size = u32::try_from(size_of_val(&buf)).unwrap_or(0);
    // SAFETY: `buf` is a live, writable buffer of `size` bytes; RRF_RT_REG_SZ
    // makes the call fail instead of writing a value of another type; the
    // key and value names are static NUL-terminated literals.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"),
            w!("CurrentBuildNumber"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS {
        log::debug!("RegGetValueW(CurrentBuildNumber) failed: {}", status.0);
        return None;
    }
    let chars = (size as usize / size_of::<u16>()).min(buf.len());
    parse_build(&util::from_wide(&buf[..chars]))
}

/// `CurrentBuildNumber`'s text as a number (`"26100"` → 26100). Pure.
fn parse_build(text: &str) -> Option<u32> {
    text.trim().parse().ok()
}

// ---------------------------------------------------------------------------
// The thread, as the platform object sees it
// ---------------------------------------------------------------------------

/// The window variant for testing (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Variant {
    /// `WS_EX_LAYERED | WS_EX_TRANSPARENT` (default) or an ordinary window.
    layered: bool,
    /// `DwmExtendFrameIntoClientArea(−1)` (default) or not.
    extend_frame: bool,
}

impl Variant {
    fn from_env() -> Self {
        Self {
            layered: flag(std::env::var("STEM_BACKDROP_LAYERED").ok().as_deref(), true),
            extend_frame: flag(
                std::env::var("STEM_BACKDROP_EXTEND_FRAME").ok().as_deref(),
                true,
            ),
        }
    }

    fn describe(self) -> String {
        format!(
            "{}, frame {}",
            if self.layered {
                "layered click-through"
            } else {
                "not layered"
            },
            if self.extend_frame {
                "extended"
            } else {
                "not extended"
            }
        )
    }
}

/// An environment switch: `"0"` is off, `"1"` is on, anything else (or
/// nothing) is `default`. Pure.
fn flag(value: Option<&str>, default: bool) -> bool {
    match value.map(str::trim) {
        Some("0") => false,
        Some("1") => true,
        _ => default,
    }
}

/// State shared between the platform object and the thread.
#[derive(Debug, Default)]
struct Shared {
    /// The latest wanted state per dock window (raw `HWND` value), not yet
    /// applied. The thread takes all of it at once.
    pending: Mutex<HashMap<isize, Option<DockBackdrop>>>,
    /// Set by [`BackdropThread::stop`] (or drop) so a thread that had no
    /// message queue yet when `WM_QUIT` would have been posted ends anyway.
    quit: AtomicBool,
}

/// Handle to the backdrop thread. Dropping it without `stop` asks the
/// thread to quit but does not wait for it; prefer `stop`.
#[derive(Debug)]
pub struct BackdropThread {
    join: Option<JoinHandle<()>>,
    done: Receiver<()>,
    shared: Arc<Shared>,
}

impl BackdropThread {
    /// Spawn the thread; returns without waiting for it (see the module docs).
    pub fn start() -> PlatformResult<Self> {
        let shared = Arc::new(Shared::default());
        let (done_tx, done_rx) = bounded::<()>(1);
        let thread_shared = Arc::clone(&shared);
        let join = std::thread::Builder::new()
            .name("stem-platform-backdrop".into())
            .spawn(move || thread_main(&thread_shared, &done_tx))
            .map_err(|e| PlatformError::Other(format!("spawn backdrop thread: {e}")))?;
        log::debug!("backdrop thread spawned");
        Ok(Self {
            join: Some(join),
            done: done_rx,
            shared,
        })
    }

    /// The backdrop of the dock window `dock` should look like `backdrop`
    /// (`None`: hidden). Returns at once.
    pub fn set(&self, dock: NativeWindowHandle, backdrop: Option<DockBackdrop>) {
        self.shared.pending.lock().insert(dock.0, backdrop);
        post_to_thread(MSG_APPLY, WPARAM(0));
    }

    pub fn stop(mut self) {
        self.request_quit();
        match self.done.recv_timeout(STOP_TIMEOUT) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                if let Some(join) = self.join.take()
                    && join.join().is_err()
                {
                    log::error!("backdrop thread panicked");
                }
                log::debug!("backdrop thread stopped");
            }
            Err(RecvTimeoutError::Timeout) => {
                log::warn!("backdrop thread did not stop within {STOP_TIMEOUT:?}; detaching it");
            }
        }
    }

    fn request_quit(&self) {
        self.shared.quit.store(true, Ordering::SeqCst);
        post_to_thread(WM_QUIT, WPARAM(0));
    }
}

impl Drop for BackdropThread {
    fn drop(&mut self) {
        if self.join.is_some() {
            self.request_quit();
        }
    }
}

/// Ask the backdrop thread, if one runs, to put the backdrop of `dock` back
/// directly under it (see the module docs). Called by the dock window's
/// subclass; posts, never waits.
pub fn restack(dock: HWND) {
    post_to_thread(MSG_RESTACK, WPARAM(dock.0 as usize));
}

/// Post a thread message to the backdrop thread; nothing while there is
/// none (a request then waits in `Shared::pending` for the thread's first
/// pass).
fn post_to_thread(message: u32, wparam: WPARAM) {
    let thread = THREAD_ID.load(Ordering::SeqCst);
    if thread == 0 {
        return;
    }
    // SAFETY: posts plain integers to a thread of this process; the call
    // fails harmlessly if the thread has already gone.
    let posted = unsafe { PostThreadMessageW(thread, message, wparam, LPARAM(0)) };
    if let Err(e) = posted {
        log::trace!("PostThreadMessageW({message:#x}) to the backdrop thread failed: {e}");
    }
}

// ---------------------------------------------------------------------------
// The thread itself
// ---------------------------------------------------------------------------

fn thread_main(shared: &Shared, done: &Sender<()>) {
    let mut msg = MSG::default();
    // SAFETY: `msg` is a live out-pointer; the call only makes the system
    // create this thread's message queue (nothing is removed).
    let _ = unsafe { PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE) };
    match Backdrops::new(Variant::from_env()) {
        Ok(mut backdrops) => {
            // SAFETY: GetCurrentThreadId has no preconditions.
            let id = unsafe { GetCurrentThreadId() };
            THREAD_ID.store(id, Ordering::SeqCst);
            if !shared.quit.load(Ordering::SeqCst) {
                log::debug!(
                    "backdrop thread running (id {id}; {})",
                    backdrops.variant.describe()
                );
                backdrops.apply_pending(shared);
                backdrops.run(shared);
            }
            let _ = THREAD_ID.compare_exchange(id, 0, Ordering::SeqCst, Ordering::SeqCst);
            log::debug!("backdrop thread releasing its windows");
            drop(backdrops);
        }
        Err(e) => log::warn!("backdrop thread: the window class could not be registered: {e}"),
    }
    let _ = done.send(());
}

/// Everything the thread owns. Field order is drop order: the windows go
/// before their class.
struct Backdrops {
    /// Keyed by the dock window's raw `HWND` value.
    windows: HashMap<isize, BackdropWindow>,
    /// Dock windows whose backdrop could not be created; warned once each.
    failed: HashSet<isize>,
    class: WindowClass,
    variant: Variant,
}

impl Backdrops {
    fn new(variant: Variant) -> PlatformResult<Self> {
        Ok(Self {
            windows: HashMap::new(),
            failed: HashSet::new(),
            class: WindowClass::register()?,
            variant,
        })
    }

    fn run(&mut self, shared: &Shared) {
        loop {
            let mut msg = MSG::default();
            // SAFETY: `msg` is a live out-pointer; a NULL filter window asks
            // for every message of this thread.
            let result = unsafe { GetMessageW(&mut msg, None, 0, 0) };
            match result.0 {
                0 => break,
                -1 => {
                    log::error!(
                        "GetMessageW failed: {}; stopping the backdrop thread",
                        windows::core::Error::from_thread()
                    );
                    break;
                }
                _ => {}
            }
            if msg.hwnd.is_invalid() {
                match msg.message {
                    MSG_APPLY => {
                        drain_thread_messages(MSG_APPLY);
                        self.apply_pending(shared);
                    }
                    MSG_RESTACK => self.restack(msg.wParam.0 as isize),
                    _ => {}
                }
                continue;
            }
            // SAFETY: `msg` was filled in by GetMessageW.
            let _translated = unsafe { TranslateMessage(&msg) };
            // SAFETY: `msg` was filled in by GetMessageW.
            let _result = unsafe { DispatchMessageW(&msg) };
        }
    }

    fn apply_pending(&mut self, shared: &Shared) {
        let requests: Vec<(isize, Option<DockBackdrop>)> = shared.pending.lock().drain().collect();
        for (dock, wanted) in requests {
            self.apply(dock, wanted);
        }
    }

    fn apply(&mut self, dock_raw: isize, wanted: Option<DockBackdrop>) {
        let dock = HWND(dock_raw as *mut core::ffi::c_void);
        // SAFETY: IsWindow accepts any handle value.
        if !unsafe { IsWindow(Some(dock)) }.as_bool() {
            if self.windows.remove(&dock_raw).is_some() {
                log::debug!("dock window {dock_raw:#x} is gone; its backdrop window is destroyed");
            }
            return;
        }
        let Some(wanted) = wanted else {
            if let Some(window) = self.windows.get_mut(&dock_raw) {
                window.hide();
            }
            return;
        };
        if !self.windows.contains_key(&dock_raw) {
            if self.failed.contains(&dock_raw) {
                return;
            }
            match BackdropWindow::create(&self.class, dock, self.variant) {
                Ok(window) => {
                    self.windows.insert(dock_raw, window);
                }
                Err(e) => {
                    log::warn!(
                        "the backdrop window for dock window {dock_raw:#x} could not be created: {e}; the panel keeps its CSS look"
                    );
                    self.failed.insert(dock_raw);
                    return;
                }
            }
        }
        if let Some(window) = self.windows.get_mut(&dock_raw) {
            window.apply(&wanted, self.variant);
        }
    }

    fn restack(&mut self, dock_raw: isize) {
        if let Some(window) = self.windows.get_mut(&dock_raw) {
            window.restack("z-order change of the dock");
        }
    }
}

/// Remove further copies of a thread message already queued: several
/// requests in a row collapse into one pass.
fn drain_thread_messages(message: u32) {
    let mut msg = MSG::default();
    loop {
        // SAFETY: `msg` is a live out-pointer; the range selects only the
        // one thread message.
        let more = unsafe { PeekMessageW(&mut msg, None, message, message, PM_REMOVE) };
        if !more.as_bool() {
            break;
        }
    }
}

/// The registered window class; unregistered on drop.
struct WindowClass {
    name: Vec<u16>,
    instance: HINSTANCE,
}

impl WindowClass {
    fn register() -> PlatformResult<Self> {
        // SAFETY: a NULL module name asks for our own executable's handle.
        let module = unsafe { GetModuleHandleW(None) }?;
        let instance = HINSTANCE::from(module);
        let name = util::to_wide(CLASS_NAME);
        // SAFETY: GetStockObject has no preconditions; a stock brush is
        // never deleted, so the class may keep it for its lifetime.
        let black = unsafe { GetStockObject(BLACK_BRUSH) };
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(backdrop_wndproc),
            hInstance: instance,
            // The "sheet of glass": a black client area under an extended
            // frame lets the material through.
            hbrBackground: HBRUSH(black.0),
            lpszClassName: util::pcwstr(&name),
            ..Default::default()
        };
        // SAFETY: `class` is fully initialised and the name buffer outlives
        // the call.
        if unsafe { RegisterClassExW(&class) } == 0 {
            let err = windows::core::Error::from_thread();
            if err.code() != util::hresult_of(ERROR_CLASS_ALREADY_EXISTS) {
                return Err(err.into());
            }
            // Left over from an earlier thread in this process; the
            // procedure is the same static function, so reusing it is fine.
        }
        Ok(Self { name, instance })
    }
}

impl Drop for WindowClass {
    fn drop(&mut self) {
        let name = util::pcwstr(&self.name);
        // SAFETY: the name buffer is alive; every window of the class was
        // destroyed before (field order of `Backdrops`).
        if let Err(e) = unsafe { UnregisterClassW(name, Some(self.instance)) } {
            log::debug!("UnregisterClassW(backdrop) failed: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// One backdrop window
// ---------------------------------------------------------------------------

/// One dock window's backdrop. Lives and dies on the backdrop thread.
struct BackdropWindow {
    hwnd: HWND,
    dock: HWND,
    /// The material last asked of DWM; `None` before the first request.
    attempted: Option<BackdropMaterial>,
    /// DWM refused [`Self::attempted`]: the window stays hidden, since
    /// without the material it would only be a black rectangle.
    refused: bool,
    dark: Option<bool>,
    /// Where the window was last placed.
    placed: Option<Rect>,
    /// The region it has; `None` before the first.
    region: Option<RegionArgs>,
    visible: bool,
}

impl BackdropWindow {
    fn create(class: &WindowClass, dock: HWND, variant: Variant) -> PlatformResult<Self> {
        let mut ex_style = WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST;
        if variant.layered {
            ex_style |= WINDOW_EX_STYLE(WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0);
        }
        let title = util::to_wide(WINDOW_TITLE);
        // SAFETY: the class is registered on this thread; the string buffers
        // outlive the call; the window is created hidden, unowned and with
        // no menu.
        let hwnd = unsafe {
            CreateWindowExW(
                ex_style,
                util::pcwstr(&class.name),
                util::pcwstr(&title),
                WS_POPUP | WS_CAPTION,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(class.instance),
                None,
            )
        }?;
        // From here on `Drop` destroys the window, whatever fails below.
        let window = Self {
            hwnd,
            dock,
            attempted: None,
            refused: false,
            dark: None,
            placed: None,
            region: None,
            visible: false,
        };
        if variant.layered {
            // SAFETY: a layered window of this thread; alpha 255 and no
            // colour key: the window is drawn as it paints itself.
            unsafe { SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA) }?;
        }
        for (attribute, value, what) in [
            (DWMWA_TRANSITIONS_FORCEDISABLED, 1, "transitions off"),
            (DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, "no border"),
            (
                DWMWA_WINDOW_CORNER_PREFERENCE,
                DWMWCP_DONOTROUND.0 as u32,
                "no DWM rounding",
            ),
        ] {
            // Warn, not debug: each one is part of the recipe, and none
            // should fail on a supported build.
            if let Err(e) = set_attribute(hwnd, attribute, value) {
                log::warn!(
                    "backdrop window {:#x}: DwmSetWindowAttribute({what}) failed: {e}",
                    hwnd.0 as isize
                );
            }
        }
        if variant.extend_frame {
            let margins = MARGINS {
                cxLeftWidth: -1,
                cxRightWidth: -1,
                cyTopHeight: -1,
                cyBottomHeight: -1,
            };
            // SAFETY: `margins` is a live MARGINS for the duration of the
            // call; the window belongs to this thread.
            if let Err(e) = unsafe { DwmExtendFrameIntoClientArea(hwnd, &margins) } {
                log::warn!(
                    "backdrop window {:#x}: DwmExtendFrameIntoClientArea(-1) failed: {e}",
                    hwnd.0 as isize
                );
            }
        }
        // The window procedure's WM_NCCALCSIZE answer applies from here.
        frame_changed(hwnd);
        // One WM_NCACTIVATE(TRUE), so DWM treats the frame as active (A6).
        // Sent to a window of this very thread: a direct call of its
        // procedure, no cross-thread or cross-process wait.
        // SAFETY: plain integers to a live window of this thread.
        let _ = unsafe { SendMessageW(hwnd, WM_NCACTIVATE, Some(WPARAM(1)), Some(LPARAM(0))) };
        log::debug!(
            "backdrop window {:#x} created for dock window {:#x} ({})",
            hwnd.0 as isize,
            dock.0 as isize,
            variant.describe()
        );
        Ok(window)
    }

    fn apply(&mut self, wanted: &DockBackdrop, variant: Variant) {
        if self.attempted != Some(wanted.material) {
            self.attempted = Some(wanted.material);
            let (name, value, constant) = material_attribute(wanted.material);
            match set_attribute(self.hwnd, DWMWA_SYSTEMBACKDROP_TYPE, value) {
                Ok(()) => {
                    self.refused = false;
                    log::info!(
                        "native backdrop applied: {name} ({constant}) on window {:#x} under dock window {:#x} ({})",
                        self.hwnd.0 as isize,
                        self.dock.0 as isize,
                        variant.describe()
                    );
                }
                Err(e) => {
                    self.refused = true;
                    log::warn!(
                        "native backdrop refused by DWM: {name} ({constant}) on window {:#x}: {e}; the panel keeps its CSS look",
                        self.hwnd.0 as isize
                    );
                }
            }
        }
        if self.refused {
            self.hide();
            return;
        }
        if self.dark != Some(wanted.dark) {
            // Tried once per change: a refusal is not retried on every move.
            self.dark = Some(wanted.dark);
            match set_attribute(
                self.hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                u32::from(wanted.dark),
            ) {
                Ok(()) => {
                    log::debug!(
                        "backdrop window {:#x}: {} material",
                        self.hwnd.0 as isize,
                        if wanted.dark { "dark" } else { "light" }
                    );
                }
                Err(e) => log::warn!(
                    "backdrop window {:#x}: DWMWA_USE_IMMERSIVE_DARK_MODE failed: {e}",
                    self.hwnd.0 as isize
                ),
            }
        }
        let rect = wanted.rect;
        let Some(region) = region_args(rect.width, rect.height, wanted.corner_radius) else {
            log::debug!(
                "backdrop window {:#x}: panel {rect:?} too small for a region; hidden",
                self.hwnd.0 as isize
            );
            self.hide();
            return;
        };
        if self.region != Some(region) {
            match set_region(self.hwnd, region) {
                Ok(()) => self.region = Some(region),
                Err(e) => {
                    log::warn!(
                        "backdrop window {:#x}: SetWindowRgn failed: {e}; hidden",
                        self.hwnd.0 as isize
                    );
                    self.region = None;
                    self.hide();
                    return;
                }
            }
        }
        if self.visible && self.placed == Some(rect) {
            self.restack("unchanged request");
            return;
        }
        let mut flags = SWP_NOACTIVATE;
        if !self.visible {
            flags |= SWP_SHOWWINDOW;
        }
        // SAFETY: a window of this thread, placed directly under the dock
        // window (any thread's window is a valid insert-after handle);
        // failure is reported in the Result.
        let placed = unsafe {
            SetWindowPos(
                self.hwnd,
                Some(self.dock),
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                flags,
            )
        };
        match placed {
            Ok(()) => {
                self.placed = Some(rect);
                if !self.visible {
                    self.visible = true;
                    log::debug!(
                        "backdrop window {:#x} shown at {rect:?} under dock window {:#x}, region {region:?}",
                        self.hwnd.0 as isize,
                        self.dock.0 as isize
                    );
                } else {
                    log::trace!(
                        "backdrop window {:#x} moved to {rect:?}",
                        self.hwnd.0 as isize
                    );
                }
            }
            Err(e) => log::warn!(
                "backdrop window {:#x}: SetWindowPos to {rect:?} failed: {e}",
                self.hwnd.0 as isize
            ),
        }
    }

    fn hide(&mut self) {
        if !self.visible {
            return;
        }
        // SAFETY: hides a window of this thread; no move, size, z-order or
        // activation change; failure is reported in the Result.
        let hidden = unsafe {
            SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_HIDEWINDOW | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
        match hidden {
            Ok(()) => {
                self.visible = false;
                log::debug!("backdrop window {:#x} hidden", self.hwnd.0 as isize);
            }
            Err(e) => log::warn!(
                "backdrop window {:#x}: hiding it failed: {e}",
                self.hwnd.0 as isize
            ),
        }
    }

    /// Put the window back directly under its dock window unless it is
    /// already there. Nothing while it is hidden.
    fn restack(&mut self, why: &str) {
        if !self.visible {
            return;
        }
        // SAFETY: GetWindow only reads window-manager state; a handle that
        // is gone makes the call fail.
        if let Ok(next) = unsafe { GetWindow(self.dock, GW_HWNDNEXT) }
            && next == self.hwnd
        {
            return;
        }
        // SAFETY: the flags change the z-order of a window of this thread
        // only; failure is reported in the Result.
        let result = unsafe {
            SetWindowPos(
                self.hwnd,
                Some(self.dock),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
        };
        match result {
            Ok(()) => log::debug!(
                "backdrop window {:#x} restacked under dock window {:#x} ({why})",
                self.hwnd.0 as isize,
                self.dock.0 as isize
            ),
            Err(e) => log::warn!(
                "backdrop window {:#x}: restacking under dock window {:#x} failed: {e}",
                self.hwnd.0 as isize,
                self.dock.0 as isize
            ),
        }
    }
}

impl Drop for BackdropWindow {
    fn drop(&mut self) {
        // SAFETY: the window was created by this thread and is destroyed once.
        if let Err(e) = unsafe { DestroyWindow(self.hwnd) } {
            log::debug!(
                "DestroyWindow(backdrop {:#x}) failed: {e}",
                self.hwnd.0 as isize
            );
        }
    }
}

/// Name, `DWM_SYSTEMBACKDROP_TYPE` value and constant name of a material.
fn material_attribute(material: BackdropMaterial) -> (&'static str, u32, &'static str) {
    match material {
        BackdropMaterial::Acrylic => (
            "Acrylic",
            DWMSBT_TRANSIENTWINDOW.0 as u32,
            "DWMSBT_TRANSIENTWINDOW",
        ),
        BackdropMaterial::Mica => ("Mica", DWMSBT_MAINWINDOW.0 as u32, "DWMSBT_MAINWINDOW"),
    }
}

/// `DwmSetWindowAttribute` with a 4-byte payload (every attribute used
/// here: `BOOL`, `COLORREF` and the two enums are all 32-bit).
fn set_attribute(hwnd: HWND, attribute: DWMWINDOWATTRIBUTE, value: u32) -> PlatformResult<()> {
    let ptr = std::ptr::from_ref(&value).cast::<core::ffi::c_void>();
    // SAFETY: `ptr` addresses a live u32 and the size passed is exactly that
    // of a u32, the documented payload size of each attribute used here.
    unsafe { DwmSetWindowAttribute(hwnd, attribute, ptr, size_of::<u32>() as u32) }?;
    Ok(())
}

/// Make the window manager send `WM_NCCALCSIZE` again: no move, size,
/// z-order or activation change. Failure is logged.
fn frame_changed(hwnd: HWND) {
    // SAFETY: the flags request a frame refresh only.
    let result = unsafe {
        SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        )
    };
    if let Err(e) = result {
        log::debug!(
            "SetWindowPos(SWP_FRAMECHANGED) on backdrop {:#x} failed: {e}",
            hwnd.0 as isize
        );
    }
}

// ---------------------------------------------------------------------------
// The region
// ---------------------------------------------------------------------------

/// Arguments of `CreateRoundRectRgn` (window-relative; `right` and `bottom`
/// exclusive, as for `CreateRectRgn`), or of `CreateRectRgn` when `ellipse`
/// is 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RegionArgs {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    /// Width and height of the corner ellipse: twice the region's radius.
    ellipse: i32,
}

/// The region of a backdrop window `width` × `height` physical px under a
/// panel with corners of `radius` px: the window inset by [`REGION_INSET`]
/// on every side, with corners concentric to the panel's (radius minus the
/// inset), never rounder than the inset box allows. `None` when nothing is
/// left. Pure.
fn region_args(width: i32, height: i32, radius: u32) -> Option<RegionArgs> {
    let right = width.saturating_sub(REGION_INSET);
    let bottom = height.saturating_sub(REGION_INSET);
    let inner_width = right.saturating_sub(REGION_INSET);
    let inner_height = bottom.saturating_sub(REGION_INSET);
    if inner_width <= 0 || inner_height <= 0 {
        return None;
    }
    let radius = i32::try_from(radius)
        .unwrap_or(i32::MAX)
        .saturating_sub(REGION_INSET)
        .max(0);
    let ellipse = radius.saturating_mul(2).min(inner_width).min(inner_height);
    Some(RegionArgs {
        left: REGION_INSET,
        top: REGION_INSET,
        right,
        bottom,
        ellipse,
    })
}

/// Give `hwnd` the region `args`. On success the system owns the region.
fn set_region(hwnd: HWND, args: RegionArgs) -> PlatformResult<()> {
    let region: HRGN = if args.ellipse > 0 {
        // SAFETY: plain integers; the call returns a null handle on failure.
        unsafe {
            CreateRoundRectRgn(
                args.left,
                args.top,
                args.right,
                args.bottom,
                args.ellipse,
                args.ellipse,
            )
        }
    } else {
        // SAFETY: plain integers; the call returns a null handle on failure.
        unsafe { CreateRectRgn(args.left, args.top, args.right, args.bottom) }
    };
    if region.is_invalid() {
        return Err(PlatformError::Other(format!(
            "the region {args:?} could not be created"
        )));
    }
    // SAFETY: a window of this thread and a region this function created;
    // on success the system owns the region and it must not be used again.
    if unsafe { SetWindowRgn(hwnd, Some(region), true) } == 0 {
        // SAFETY: the call failed, so the region is still ours; it is
        // deleted exactly once, here.
        let _ = unsafe { DeleteObject(region.into()) };
        return Err(PlatformError::Other(format!(
            "SetWindowRgn({args:?}) failed"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The window procedure
// ---------------------------------------------------------------------------

unsafe extern "system" fn backdrop_wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        // The whole window is client area: the caption style is only there
        // so DWM accepts the backdrop type (A3).
        WM_NCCALCSIZE if wparam.0 != 0 => LRESULT(0),
        // Always "active" for DWM, so the material stays live on a window
        // that is never activated (A6). lParam is passed on unchanged.
        // SAFETY: forwarding our own arguments, with wParam forced to TRUE,
        // to the default procedure.
        WM_NCACTIVATE => unsafe { DefWindowProcW(hwnd, message, WPARAM(1), lparam) },
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        // The app layer sends the panel again at the new scale; the window
        // must not resize itself to keep a logical size.
        WM_DPICHANGED => LRESULT(0),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_GETMINMAXINFO => {
            let info = lparam.0 as *mut MINMAXINFO;
            if !info.is_null() {
                // SAFETY: for WM_GETMINMAXINFO the system passes a MINMAXINFO
                // that is valid for the duration of the message and is meant
                // to be modified by the window.
                let info = unsafe { &mut *info };
                info.ptMinTrackSize = POINT { x: 1, y: 1 };
            }
            LRESULT(0)
        }
        // SAFETY: forwarding the arguments we were given to the default
        // procedure.
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_build_number_parses_from_the_registry_text() {
        assert_eq!(parse_build("26100"), Some(26100));
        assert_eq!(parse_build(" 22621 "), Some(22621));
        assert_eq!(parse_build(""), None);
        assert_eq!(parse_build("22621.1"), None);
    }

    #[test]
    fn a_switch_is_off_only_for_0_and_on_only_for_1() {
        assert!(flag(None, true));
        assert!(!flag(Some("0"), true));
        assert!(!flag(Some(" 0 "), true));
        assert!(flag(Some("1"), false));
        assert!(flag(Some("no"), true));
        assert!(!flag(Some("yes"), false));
    }

    #[test]
    fn the_region_is_inset_by_one_pixel_with_concentric_corners() {
        // A 960 x 96 panel with 33 px corners (22 css at 150 %).
        assert_eq!(
            region_args(960, 96, 33),
            Some(RegionArgs {
                left: 1,
                top: 1,
                right: 959,
                bottom: 95,
                ellipse: 64,
            })
        );
        // Square corners: a plain rectangle.
        assert_eq!(region_args(100, 50, 0).map(|r| r.ellipse), Some(0));
        assert_eq!(region_args(100, 50, 1).map(|r| r.ellipse), Some(0));
    }

    #[test]
    fn the_corner_never_exceeds_the_inset_box_and_a_tiny_panel_has_none() {
        // A radius larger than half the height gives a pill, not a bulge.
        assert_eq!(region_args(400, 40, 64).map(|r| r.ellipse), Some(38));
        assert_eq!(region_args(400, 40, u32::MAX).map(|r| r.ellipse), Some(38));
        assert_eq!(region_args(2, 40, 8), None);
        assert_eq!(region_args(40, 2, 8), None);
        assert_eq!(region_args(0, 0, 0), None);
        assert_eq!(region_args(-5, 40, 0), None);
        assert_eq!(region_args(3, 3, 8).map(|r| r.ellipse), Some(1));
    }

    #[test]
    fn the_materials_map_to_the_documented_backdrop_types() {
        assert_eq!(material_attribute(BackdropMaterial::Acrylic).1, 3);
        assert_eq!(material_attribute(BackdropMaterial::Mica).1, 2);
    }
}
