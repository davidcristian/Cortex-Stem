//! The loop of the hit-test thread: it reads the cursor on every wake,
//! timer tick and raw mouse input, and runs the rule for each tracked
//! window.

use std::sync::Arc;

use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetCursorPos, GetMessageW, IsWindow, MSG, PM_REMOVE, PeekMessageW,
    TranslateMessage, WM_INPUT, WM_TIMER,
};

use super::window::TrackerWindow;
use super::{FALLBACK_POLL, MSG_EVALUATE, SAFETY_POLL, Shared, Tracked};
use crate::rules::hit_region;
use crate::windows::dock_window;
use crate::{NativeWindowHandle, PlatformResult};

/// Everything the loop owns.
pub(super) struct Tracker {
    shared: Arc<Shared>,
    pub(super) thread_id: u32,
    /// Screen position at the last evaluation; unchanged → nothing to do.
    last_cursor: Option<(i32, i32)>,
    _window: TrackerWindow,
}

impl Tracker {
    pub(super) fn setup(shared: Arc<Shared>) -> PlatformResult<Self> {
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

    pub(super) fn run(&mut self) {
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
