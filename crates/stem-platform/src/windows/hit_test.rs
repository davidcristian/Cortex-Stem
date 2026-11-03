//! The cursor-tracking thread behind `Platform::set_dock_hit_region`.
//!
//! Contract:
//! - [`HitTestThread::start`] spawns `stem-platform-hit-test`, which creates
//!   a message-only window, registers for raw mouse input
//!   (`RegisterRawInputDevices`, usage page 1 / usage 2, `RIDEV_INPUTSINK`,
//!   so `WM_INPUT` arrives whether or not we own the foreground), starts a
//!   window timer as a safety poll and runs a `GetMessageW` loop.
//! - [`HitTestThread::track`] registers or updates a dock window with its
//!   [`HitRegion`]; [`HitTestThread::untrack`] forgets it and makes it
//!   clickable again. Both return at once; the thread re-evaluates right
//!   away (a thread message wakes it).
//! - On every `WM_INPUT`, every timer tick and every wake the thread reads
//!   `GetCursorPos`, converts the point to each tracked window's client
//!   space (`ScreenToClient`; neither call sends a message) and runs the
//!   pure rule `rules::hit_region::next_inside`. A transition posts
//!   [`dock_window::MSG_SET_PASS_THROUGH`] to the dock window
//!   ([`dock_window::post_pass_through`]), whose subclass applies it on the
//!   window's own thread. The first evaluation of a newly tracked window
//!   always posts, so a window becomes click-through at startup when the
//!   cursor is elsewhere.
//! - Windows whose handle is gone (`IsWindow`) are dropped.
//! - [`HitTestThread::stop`] posts `WM_QUIT` and joins (bounded).
//! - Switching the feature off is the app layer's business
//!   (`behavior.clickThrough`): it passes `None`, which untracks the
//!   window and makes it fully clickable; there is no environment variable.
//!
//! # Design
//!
//! Raw input instead of a low-level mouse hook: `WH_MOUSE_LL` adds latency
//! to every mouse event in the system and is silently removed when the
//! hook procedure is slow; raw input is delivered to our own queue
//! asynchronously and costs nothing while the mouse is still. The payload
//! of `WM_INPUT` is not read (`DefWindowProcW` releases it): only "the
//! mouse moved" matters, and `GetCursorPos` gives the position after
//! acceleration and clipping. Touch and pen produce no mouse raw input and
//! a `WM_INPUT` can be missed during a modal loop, so a slow timer
//! ([`SAFETY_POLL`], 4 Hz) re-evaluates anyway. When registration fails
//! the timer runs at [`FALLBACK_POLL`] (30 Hz) instead; the mode is logged
//! once at info ("raw input + 4 Hz poll" / "polling at 30 Hz").
//!
//! An evaluation is skipped when the cursor has not moved since the last
//! one (the common case for the timer), unless a wake asked for it.
//!
//! Registering raw mouse input replaces tao's own registration for the
//! mouse (`RIDEV_DEVNOTIFY` on its thread-message window; a process holds
//! one registration per device class): tao's `DeviceEvent::Mouse*` events
//! stop, which Tauri does not expose and the frontend does not use (it gets
//! pointer events from WebView2).
//!
//! Thread safety: the tracked windows live in a `Mutex` shared between the
//! platform object and the thread; raw handles (the hidden window, the
//! timer) never leave the thread; only `isize` handle values and the thread
//! id cross threads.

use std::collections::HashMap;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded};
use parking_lot::Mutex;
use windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::{
    RAWINPUTDEVICE, RIDEV_INPUTSINK, RIDEV_REMOVE, RegisterRawInputDevices,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW,
    HWND_MESSAGE, IsWindow, KillTimer, MSG, PM_REMOVE, PeekMessageW, PostThreadMessageW,
    RegisterClassExW, SetTimer, TranslateMessage, UnregisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_APP, WM_INPUT, WM_QUIT, WM_TIMER, WNDCLASSEXW,
};

use super::{dock_window, util};
use crate::rules::hit_region;
use crate::{HitRegion, NativeWindowHandle, PlatformError, PlatformResult};

/// Thread message (`MSG::hwnd == NULL`): re-evaluate every tracked window
/// even if the cursor did not move (a region was added or changed).
const MSG_EVALUATE: u32 = WM_APP + 0x303;

const CLASS_NAME: &str = "DockPlatformHitTest";
const WINDOW_TITLE: &str = "Stem platform hit test";
/// Window timer id of the safety / fallback poll.
const POLL_TIMER_ID: usize = 1;
/// Safety poll while raw input works (touch, pen, missed events).
pub const SAFETY_POLL: Duration = Duration::from_millis(250);
/// Poll interval when raw input could not be registered.
pub const FALLBACK_POLL: Duration = Duration::from_millis(33);
const START_TIMEOUT: Duration = Duration::from_secs(5);
const STOP_TIMEOUT: Duration = Duration::from_secs(2);
/// HID usage page "Generic Desktop Controls" and its "Mouse" usage (HID
/// Usage Tables §4); literal so no extra `windows` feature is needed.
const HID_USAGE_PAGE_GENERIC: u16 = 0x01;
const HID_USAGE_GENERIC_MOUSE: u16 = 0x02;

/// A dock window under tracking.
#[derive(Debug)]
struct Tracked {
    region: HitRegion,
    /// The hysteresis state: `true` while the window accepts mouse input.
    inside: bool,
    /// The pass-through value last posted to the window; `None` until the
    /// first evaluation, which therefore always posts.
    posted: Option<bool>,
}

/// State shared between the platform object and the thread.
#[derive(Debug, Default)]
struct Shared {
    /// Keyed by the raw `HWND` value.
    windows: Mutex<HashMap<isize, Tracked>>,
}

/// Handle to the running tracker thread. Dropping it without `stop` asks
/// the thread to quit but does not wait for it; prefer `stop`.
#[derive(Debug)]
pub struct HitTestThread {
    thread_id: u32,
    join: Option<JoinHandle<()>>,
    done: Receiver<()>,
    shared: Arc<Shared>,
}

impl HitTestThread {
    pub fn start() -> PlatformResult<Self> {
        let shared = Arc::new(Shared::default());
        let (ready_tx, ready_rx) = bounded::<PlatformResult<u32>>(1);
        let (done_tx, done_rx) = bounded::<()>(1);
        let thread_shared = Arc::clone(&shared);
        let join = std::thread::Builder::new()
            .name("stem-platform-hit-test".into())
            .spawn(move || thread_main(thread_shared, &ready_tx, &done_tx))
            .map_err(|e| PlatformError::Other(format!("spawn hit-test thread: {e}")))?;
        match ready_rx.recv_timeout(START_TIMEOUT) {
            Ok(Ok(thread_id)) => {
                log::debug!("hit-test thread started (id {thread_id})");
                Ok(Self {
                    thread_id,
                    join: Some(join),
                    done: done_rx,
                    shared,
                })
            }
            Ok(Err(e)) => {
                if join.join().is_err() {
                    log::error!("hit-test thread panicked during setup");
                }
                Err(e)
            }
            Err(_) => {
                log::error!("hit-test thread did not report readiness within {START_TIMEOUT:?}");
                Err(PlatformError::Timeout)
            }
        }
    }

    /// Track `handle` with `region` (or replace its region) and evaluate at
    /// once. Returns immediately.
    pub fn track(&self, handle: NativeWindowHandle, region: HitRegion) {
        log::debug!(
            "hit region of window {:#x}: panel {:?}, envelope {:?} (physical px, window-local)",
            handle.0,
            region.panel,
            region.envelope
        );
        {
            let mut windows = self.shared.windows.lock();
            windows
                .entry(handle.0)
                .and_modify(|tracked| tracked.region = region)
                .or_insert_with(|| Tracked {
                    region,
                    inside: false,
                    posted: None,
                });
        }
        self.wake();
    }

    /// Stop tracking `handle` and make it fully clickable again.
    pub fn untrack(&self, handle: NativeWindowHandle) {
        let mut windows = self.shared.windows.lock();
        if windows.remove(&handle.0).is_some() {
            // Posted under the lock so an evaluation running on the thread
            // cannot slip a stale "click-through" in after it.
            if dock_window::post_pass_through(handle, false) {
                log::debug!(
                    "hit region of window {:#x} cleared; fully clickable",
                    handle.0
                );
            }
        }
    }

    pub fn stop(mut self) {
        self.post_quit();
        match self.done.recv_timeout(STOP_TIMEOUT) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                if let Some(join) = self.join.take()
                    && join.join().is_err()
                {
                    log::error!("hit-test thread panicked");
                }
                log::debug!("hit-test thread stopped");
            }
            Err(RecvTimeoutError::Timeout) => {
                log::warn!("hit-test thread did not stop within {STOP_TIMEOUT:?}; detaching it");
            }
        }
    }

    fn wake(&self) {
        // SAFETY: posting plain integers to a thread id we own; the call
        // fails harmlessly if the thread has already gone.
        let posted =
            unsafe { PostThreadMessageW(self.thread_id, MSG_EVALUATE, WPARAM(0), LPARAM(0)) };
        if let Err(e) = posted {
            log::debug!("PostThreadMessageW(MSG_EVALUATE) failed: {e}");
        }
    }

    fn post_quit(&self) {
        // SAFETY: WM_QUIT with zero parameters to a thread id we own; the
        // call fails harmlessly if the thread has already gone.
        if let Err(e) = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) }
        {
            log::debug!("PostThreadMessageW(WM_QUIT) failed: {e}");
        }
    }
}

impl Drop for HitTestThread {
    fn drop(&mut self) {
        if self.join.is_some() {
            self.post_quit();
        }
    }
}

fn thread_main(shared: Arc<Shared>, ready: &Sender<PlatformResult<u32>>, done: &Sender<()>) {
    match Tracker::setup(shared) {
        Ok(mut tracker) => {
            if ready.send(Ok(tracker.thread_id)).is_ok() {
                tracker.run();
            }
            log::debug!("hit-test thread releasing its window");
            drop(tracker);
        }
        Err(e) => {
            let _ = ready.send(Err(e));
        }
    }
    let _ = done.send(());
}

/// Everything the loop owns.
struct Tracker {
    shared: Arc<Shared>,
    thread_id: u32,
    /// Screen position at the last evaluation; unchanged → nothing to do.
    last_cursor: Option<(i32, i32)>,
    _window: TrackerWindow,
}

impl Tracker {
    fn setup(shared: Arc<Shared>) -> PlatformResult<Self> {
        let window = TrackerWindow::create()?;
        if window.raw_input {
            log::info!(
                "hit-test tracker: raw input + {} Hz poll",
                1000 / SAFETY_POLL.as_millis().max(1)
            );
        } else {
            log::info!(
                "hit-test tracker: polling at {} Hz (raw mouse input unavailable)",
                1000 / FALLBACK_POLL.as_millis().max(1)
            );
        }
        // SAFETY: GetCurrentThreadId has no preconditions.
        let thread_id = unsafe { GetCurrentThreadId() };
        Ok(Self {
            shared,
            thread_id,
            last_cursor: None,
            _window: window,
        })
    }

    fn run(&mut self) {
        loop {
            let mut msg = MSG::default();
            // SAFETY: `msg` is a live out-pointer; a NULL filter window asks
            // for every message of this thread.
            let result = unsafe { GetMessageW(&mut msg, None, 0, 0) };
            match result.0 {
                0 => break,
                -1 => {
                    log::error!(
                        "GetMessageW failed: {}; stopping the hit-test thread",
                        windows::core::Error::from_thread()
                    );
                    break;
                }
                _ => {}
            }
            if msg.hwnd.is_invalid() {
                if msg.message == MSG_EVALUATE {
                    self.drain_evaluate_requests();
                    self.evaluate(true);
                }
                continue;
            }
            if msg.message == WM_INPUT || msg.message == WM_TIMER {
                self.evaluate(false);
                // Dispatched anyway: DefWindowProcW releases the WM_INPUT
                // payload.
            }
            // SAFETY: `msg` was filled in by GetMessageW.
            let _translated = unsafe { TranslateMessage(&msg) };
            // SAFETY: `msg` was filled in by GetMessageW.
            let _result = unsafe { DispatchMessageW(&msg) };
        }
    }

    /// Several wakes in a row collapse into one evaluation.
    fn drain_evaluate_requests(&self) {
        let mut msg = MSG::default();
        loop {
            // SAFETY: `msg` is a live out-pointer; the range selects only
            // our own thread message.
            let more =
                unsafe { PeekMessageW(&mut msg, None, MSG_EVALUATE, MSG_EVALUATE, PM_REMOVE) };
            if !more.as_bool() {
                break;
            }
        }
    }

    /// Read the cursor and run the rule for every tracked window. With
    /// `force` the unchanged-cursor shortcut is skipped.
    fn evaluate(&mut self, force: bool) {
        let mut cursor = POINT::default();
        // SAFETY: `cursor` is a live out-pointer.
        if let Err(e) = unsafe { GetCursorPos(&mut cursor) } {
            // Fails on the secure desktop (UAC prompt, lock screen): there
            // is nothing to track there.
            log::trace!("GetCursorPos failed: {e}");
            return;
        }
        let position = (cursor.x, cursor.y);
        if !force && self.last_cursor == Some(position) {
            return;
        }
        self.last_cursor = Some(position);
        let mut windows = self.shared.windows.lock();
        windows.retain(|&raw, tracked| evaluate_window(raw, tracked, cursor));
    }
}

/// One window's turn; `false` drops it from the map (the handle is gone).
fn evaluate_window(raw: isize, tracked: &mut Tracked, cursor: POINT) -> bool {
    let hwnd = HWND(raw as *mut core::ffi::c_void);
    // SAFETY: IsWindow accepts any handle value.
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        log::debug!("hit region: window {raw:#x} is gone; no longer tracked");
        return false;
    }
    let mut local = cursor;
    // SAFETY: `local` is a live in/out POINT; the call reads the window's
    // placement from the window manager and sends no message.
    if !unsafe { ScreenToClient(hwnd, &mut local) }.as_bool() {
        return true;
    }
    let inside = hit_region::next_inside(tracked.inside, (local.x, local.y), &tracked.region);
    tracked.inside = inside;
    let pass_through = !inside;
    if tracked.posted != Some(pass_through)
        && dock_window::post_pass_through(NativeWindowHandle(raw), pass_through)
    {
        tracked.posted = Some(pass_through);
        // Debug, not trace: one line per crossing of the panel edge is rare
        // and is what a human needs to see when click-through misbehaves.
        log::debug!(
            "hit region: window {raw:#x} {} at ({}, {})",
            if pass_through {
                "click-through"
            } else {
                "clickable"
            },
            local.x,
            local.y
        );
    }
    true
}

/// The message-only window that receives raw input and the poll timer.
struct TrackerWindow {
    hwnd: HWND,
    class_name: Vec<u16>,
    instance: HINSTANCE,
    /// Raw mouse input is registered on this window.
    raw_input: bool,
}

impl TrackerWindow {
    fn create() -> PlatformResult<Self> {
        // SAFETY: a NULL module name asks for our own executable's handle.
        let module = unsafe { GetModuleHandleW(None) }?;
        let instance = HINSTANCE::from(module);
        let class_name = util::to_wide(CLASS_NAME);
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(tracker_wndproc),
            hInstance: instance,
            lpszClassName: util::pcwstr(&class_name),
            ..Default::default()
        };
        // SAFETY: `class` is fully initialised and the name buffer outlives the call.
        if unsafe { RegisterClassExW(&class) } == 0 {
            let err = windows::core::Error::from_thread();
            if err.code() != util::hresult_of(ERROR_CLASS_ALREADY_EXISTS) {
                return Err(err.into());
            }
            // Left over from a previous start in this process; the procedure
            // is the same static function, so reusing the class is fine.
        }
        let title = util::to_wide(WINDOW_TITLE);
        // SAFETY: the class is registered; the string buffers outlive the
        // call; HWND_MESSAGE makes it a message-only window.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                util::pcwstr(&class_name),
                util::pcwstr(&title),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance),
                None,
            )
        }?;
        let mut window = Self {
            hwnd,
            class_name,
            instance,
            raw_input: false,
        };
        window.raw_input = window.register_raw_input();
        let poll = if window.raw_input {
            SAFETY_POLL
        } else {
            FALLBACK_POLL
        };
        let millis = u32::try_from(poll.as_millis()).unwrap_or(u32::MAX).max(1);
        // SAFETY: a timer on a window of this thread, without a callback:
        // WM_TIMER is delivered to this thread's queue.
        if unsafe { SetTimer(Some(hwnd), POLL_TIMER_ID, millis, None) } == 0 {
            let err = util::last_error();
            if window.raw_input {
                log::warn!(
                    "hit-test tracker: SetTimer({millis} ms) failed ({err}); raw input only"
                );
            } else {
                return Err(PlatformError::Other(format!(
                    "hit-test tracker has neither raw input nor a timer: {err}"
                )));
            }
        }
        Ok(window)
    }

    /// Mouse raw input delivered to this window even in the background.
    fn register_raw_input(&self) -> bool {
        let devices = [RAWINPUTDEVICE {
            usUsagePage: HID_USAGE_PAGE_GENERIC,
            usUsage: HID_USAGE_GENERIC_MOUSE,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: self.hwnd,
        }];
        // SAFETY: the array is live for the call and cbSize is the size of
        // its element type.
        match unsafe { RegisterRawInputDevices(&devices, size_of::<RAWINPUTDEVICE>() as u32) } {
            Ok(()) => true,
            Err(e) => {
                log::warn!("RegisterRawInputDevices(mouse, INPUTSINK) failed: {e}");
                false
            }
        }
    }
}

impl Drop for TrackerWindow {
    fn drop(&mut self) {
        if self.raw_input {
            let devices = [RAWINPUTDEVICE {
                usUsagePage: HID_USAGE_PAGE_GENERIC,
                usUsage: HID_USAGE_GENERIC_MOUSE,
                dwFlags: RIDEV_REMOVE,
                hwndTarget: HWND::default(),
            }];
            // SAFETY: as in `register_raw_input`; RIDEV_REMOVE requires a
            // NULL target window.
            let removed =
                unsafe { RegisterRawInputDevices(&devices, size_of::<RAWINPUTDEVICE>() as u32) };
            if let Err(e) = removed {
                log::debug!("RegisterRawInputDevices(RIDEV_REMOVE) failed: {e}");
            }
        }
        // SAFETY: the timer belongs to this window; a missing timer only
        // makes the call fail.
        if let Err(e) = unsafe { KillTimer(Some(self.hwnd), POLL_TIMER_ID) } {
            log::trace!("KillTimer(hit-test poll) failed: {e}");
        }
        // SAFETY: the window was created by this thread and is destroyed once.
        if let Err(e) = unsafe { DestroyWindow(self.hwnd) } {
            log::warn!("DestroyWindow(hit-test window) failed: {e}");
        }
        let class_name = util::pcwstr(&self.class_name);
        // SAFETY: the class name buffer is alive; the only window of the
        // class has just been destroyed.
        if let Err(e) = unsafe { UnregisterClassW(class_name, Some(self.instance)) } {
            log::debug!("UnregisterClassW(hit-test) failed: {e}");
        }
    }
}

/// Window procedure of the message-only window. The loop already acted on
/// `WM_INPUT` and `WM_TIMER` before dispatching; the default procedure
/// releases the raw-input payload.
unsafe extern "system" fn tracker_wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: forwarding the arguments we were given to the default procedure.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}
