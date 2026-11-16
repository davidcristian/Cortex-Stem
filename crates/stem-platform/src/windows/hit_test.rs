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

mod tracker;
mod window;

use std::collections::HashMap;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded};
use parking_lot::Mutex;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_APP, WM_QUIT};

use super::dock_window;
use crate::{HitRegion, NativeWindowHandle, PlatformError, PlatformResult};
use tracker::Tracker;

/// Thread message (`MSG::hwnd == NULL`): re-evaluate every tracked window
/// even if the cursor did not move (a region was added or changed).
const MSG_EVALUATE: u32 = WM_APP + 0x303;

/// Safety poll while raw input works (touch, pen, missed events).
pub const SAFETY_POLL: Duration = Duration::from_millis(250);
/// Poll interval when raw input could not be registered.
pub const FALLBACK_POLL: Duration = Duration::from_millis(33);
const START_TIMEOUT: Duration = Duration::from_secs(5);
const STOP_TIMEOUT: Duration = Duration::from_secs(2);

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
