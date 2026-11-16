//! The native backdrop window under a dock window (prototype:
//! `docs/research/backdrop/backdrop-window.md`).
//!
//! Contract:
//! - [`supported`] decides once per process whether the compositor has
//!   `DWMWA_SYSTEMBACKDROP_TYPE`: Windows 11 build
//!   [`MIN_BUILD`](rules::MIN_BUILD) (22621) or later, as
//!   `CurrentBuildNumber` under
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
//!   whatever is beneath (A4). In the fallback below (not layered) the
//!   window is never activated (`WM_MOUSEACTIVATE → MA_NOACTIVATE`), but it
//!   does take a click that reaches it: `WM_NCHITTEST → HTTRANSPARENT`
//!   passes a click on only to windows of the same thread beneath, and
//!   every window under the backdrop belongs to another thread. Such a click
//!   is lost, as the research note says the fallback can do.
//! - Z-order: unowned (an owned window is always above its owner), and
//!   always placed with `hWndInsertAfter` = the dock window, never with
//!   `HWND_TOP` / `HWND_TOPMOST`, and shown the same way (`SWP_SHOWWINDOW`),
//!   never with `ShowWindow`. [`restack`] puts it back under the dock when
//!   it is not the window right below it (`GetWindow(GW_HWNDNEXT)`).
//! - Shape: a `CreateRoundRectRgn` region inset by
//!   `rules::backdrop::REGION_INSET` px, with corners concentric to the
//!   panel's (`rules::backdrop::region_args`), and DWM's own
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
//! The build check's parsing, the switches, the region and the material's
//! attribute value are `rules::backdrop` (pure, unit-tested on every host).
//!
//! Thread safety: the pending requests live in a `Mutex` shared between the
//! platform object and the thread; the windows, the class and every raw
//! handle stay on the thread; only `isize` handle values, the plain
//! [`DockBackdrop`] values and the thread id cross threads.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, bounded};
use parking_lot::Mutex;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Dwm::{DWMSBT_MAINWINDOW, DWMSBT_TRANSIENTWINDOW};
use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_APP, WM_QUIT};

use crate::rules::backdrop::{self as rules, Variant};
use crate::{DockBackdrop, NativeWindowHandle, PlatformError, PlatformResult};
use thread::thread_main;

mod build_check;
mod class;
mod placement;
mod region;
mod thread;
mod window;

pub use build_check::supported;

// The host-independent values in `rules::backdrop` mirror dwmapi.h; prove
// them against the SDK here, where the `windows` crate is available.
const _: () = {
    assert!(rules::DWMSBT_MAINWINDOW == DWMSBT_MAINWINDOW.0 as u32);
    assert!(rules::DWMSBT_TRANSIENTWINDOW == DWMSBT_TRANSIENTWINDOW.0 as u32);
};

/// Thread message: apply the pending requests.
const MSG_APPLY: u32 = WM_APP + 0x212;
/// Thread message: put the backdrop of the dock window in `wParam` back
/// under it, if it is not there.
const MSG_RESTACK: u32 = WM_APP + 0x213;

const STOP_TIMEOUT: Duration = Duration::from_secs(2);

/// The running backdrop thread's id; 0 while there is none (before it has a
/// message queue, and after it ended). One backdrop thread per process.
static THREAD_ID: AtomicU32 = AtomicU32::new(0);

/// The window variant the environment asks for (see the module docs).
fn variant_from_env() -> Variant {
    Variant::from_switches(
        std::env::var("STEM_BACKDROP_LAYERED").ok().as_deref(),
        std::env::var("STEM_BACKDROP_EXTEND_FRAME").ok().as_deref(),
    )
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
