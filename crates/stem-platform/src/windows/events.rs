//! The event thread.
//!
//! Contract:
//! - [`EventHookThread::start`] spawns a thread that: creates a hidden
//!   top-level window, installs `SetWinEventHook` out-of-context hooks for
//!   `EVENT_OBJECT_CREATE/DESTROY/SHOW/HIDE/CLOAKED/UNCLOAKED/NAMECHANGE`,
//!   `EVENT_SYSTEM_FOREGROUND`, `EVENT_SYSTEM_MINIMIZESTART/END`, registers a
//!   shell hook window (`RegisterShellHookWindow`) for `HSHELL_WINDOWCREATED`,
//!   `HSHELL_FLASH` and activations (the second source; see *Sources*
//!   below), and then runs a `GetMessageW` loop.
//! - Hook callbacks only translate the event and push a [`stem_core::PlatformEvent`]
//!   into `sink`; all filtering goes through `enumerate::query_window_checked`.
//! - Foreground reports for windows of our own process (the dock after a
//!   click, the settings window) are dropped without emitting anything, so
//!   the app that was active stays active for the engine; the
//!   WinEvent hook already skips them (`WINEVENT_SKIPOWNPROCESS`), the shell
//!   hook does not.
//! - The thread emits an initial `WindowsSnapshot`, `MonitorsChanged`,
//!   `ThemeChanged`, `ForegroundChanged` after setup; the initial
//!   `RecycleBinChanged` comes from the poll thread (`recycle_bin_poll`).
//! - `WM_DISPLAYCHANGE`/`WM_DPICHANGED` -> `MonitorsChanged` + `WindowsSnapshot`.
//! - `WM_SETTINGCHANGE` ("ImmersiveColorSet") and `WM_DWMCOLORIZATIONCOLORCHANGED`
//!   -> `ThemeChanged` (via `theme::current_theme`).
//! - The other parts of the contract are in the submodule docs: late-visible
//!   windows in `rechecks`, the clock format in `system_settings`, window
//!   moves in `moves`, fullscreen in `fullscreen_check`, virtual desktops in
//!   `desktop_switch`, power in `power_status` and the Recycle Bin thread in
//!   `recycle_bin_poll`.
//! - [`EventHookThread::stop`] stops the poller, posts `WM_QUIT`, unhooks,
//!   and joins (bounded).
//!
//! # Design
//!
//! One OS thread (`stem-platform-events`) owns everything: a hidden window,
//! the WinEvent hooks, the power-setting registrations, the timers and the
//! message loop. All of it is released on that thread, in reverse order, when
//! the loop exits (RAII guards: [`WinEventHook`], [`PowerNotification`],
//! [`EventWindow`], [`ThreadTimer`]).
//!
//! **The hook callback and the window procedure do the minimum.** Both run
//! on this thread (out-of-context hooks are delivered through the installing
//! thread's message loop; a window procedure runs on the thread that owns the
//! window). They only filter and translate (`rules::events`) and then post a
//! `WM_APP`-range *thread* message to their own queue with
//! `PostThreadMessageW(GetCurrentThreadId())`. The loop handles those
//! messages with full access to [`ThreadState`] (sink, debouncer, the set of
//! windows already reported). `WINEVENTPROC` has no user pointer, which is
//! usually solved with a process-wide static holding the sink; routing
//! through the thread's own queue makes that static unnecessary, so nothing
//! here assumes a single instance (each thread has its own queue). The only
//! process-wide value is the registered `SHELLHOOK` message id, which is a
//! per-process constant by definition.
//!
//! **Translation.** The event → intent table and the title debounce policy
//! live in `rules::events` and are unit-tested on Linux. `WindowRemoved` is
//! only emitted for windows this thread reported earlier (`known`, which
//! also keeps the facts last reported about each, for the move path's
//! comparison), which keeps child-control `DESTROY` storms off the engine's
//! channel. A window that stops qualifying (hidden, cloaked by its app,
//! title emptied) is reported as removed; one that starts qualifying on any
//! update is reported as added, so a missed `SHOW` is healed by the next
//! event about it.
//!
//! **Thread safety.** [`EventHookThread`] stores only the thread id, the
//! `JoinHandle`s and channel ends; raw handles (`HWND`, `HWINEVENTHOOK`,
//! timer ids) are `!Send` and never leave the events thread.
//!
//! # Sources
//!
//! Two OS mechanisms feed the loop, through the same queue and the same
//! `query_window_checked` path. Which drives which `PlatformEvent`:
//!
//! | `PlatformEvent`      | WinEvent (primary)                                    | Shell hook (second source) |
//! |----------------------|-------------------------------------------------------|----------------------------|
//! | `WindowAdded`        | `EVENT_OBJECT_CREATE`, `EVENT_OBJECT_SHOW` (+ re-checks) | `HSHELL_WINDOWCREATED` |
//! | `WindowRemoved`      | `EVENT_OBJECT_DESTROY`, `EVENT_OBJECT_HIDE`, any query that finds the window no longer eligible | — (`HSHELL_WINDOWDESTROYED` ignored, below) |
//! | `WindowUpdated`      | `EVENT_OBJECT_NAMECHANGE` (debounced), `EVENT_SYSTEM_MINIMIZESTART/END`, `EVENT_OBJECT_CLOAKED/UNCLOAKED`, `EVENT_OBJECT_LOCATIONCHANGE` (coalesced) | — (`HSHELL_REDRAW` ignored, below) |
//! | `ForegroundChanged`  | `EVENT_SYSTEM_FOREGROUND`                             | `HSHELL_WINDOWACTIVATED`, `HSHELL_RUDEAPPACTIVATED` |
//! | `WindowAttention`    | — (no WinEvent exists for a flash)                    | `HSHELL_FLASH` |
//!
//! **Duplicates.** The two sources report the same facts about the same
//! windows a few milliseconds apart, in no fixed order. They are coalesced
//! by `known`: the first source to find a new window eligible produces the
//! one `WindowAdded` (and the debug line `window 0x… added by <source>`);
//! the other finds it tracked and produces a `WindowUpdated` with the same
//! facts, which the engine's upsert absorbs (no fact changed, so nothing
//! downstream moves). A foreground switch reported by both yields two
//! `ForegroundChanged` with the same id, likewise absorbed, and arms the
//! fullscreen timer once. No duplicate `WindowAdded` can reach the engine
//! from this thread: `report_queried` consults `known` for every emission.
//!
//! **When one source is silent.** The WinEvent hooks are required:
//! `start` fails if one cannot be installed. The shell hook is optional: a
//! failed `RegisterShellHookWindow` logs a warning and the dock runs on
//! WinEvents alone, losing only the attention cue (`HSHELL_FLASH` has no
//! WinEvent equivalent; the other three shell codes duplicate a WinEvent).
//! A WinEvent missed for one window (for example a Store app's frame
//! judged too early) is healed by `HSHELL_WINDOWCREATED`, by the re-checks,
//! and by the foreground path, which reports an untracked candidate before
//! `ForegroundChanged`; a window missed by every event is listed by the next
//! snapshot (display change). Which source gets to a window first is what
//! the trace log shows: every received event is logged once
//! as `<EVENT_…> for window 0x…` or `<HSHELL_…> for window 0x…`, and every
//! shell code that is dropped as `<HSHELL_…> (0x…) ignored`.
//!
//! **Deliberately not translated.** `HSHELL_WINDOWDESTROYED` is left to
//! `EVENT_OBJECT_DESTROY`: the WinEvent is delivered for every window (the
//! callback accepts it even when `GetAncestor` can no longer see the
//! window), `EVENT_OBJECT_HIDE` covers a window that hides rather than
//! dies, and a removal is idempotent (`remove_window` emits only for a
//! tracked window), so the shell's code could only repeat a removal that
//! has happened or is about to. `HSHELL_REDRAW` (the taskbar's "title or
//! icon changed") is left out because titles already arrive as
//! `EVENT_OBJECT_NAMECHANGE` through the debouncer and window icons are
//! read once per session by the UI (`hwnd:` keys are not re-fetched), so it
//! would add a `WindowUpdated` with unchanged facts per repaint. The
//! remaining codes (`HSHELL_GETMINRECT`, `HSHELL_ACTIVATESHELLWINDOW`,
//! `HSHELL_TASKMAN`, `HSHELL_LANGUAGE`, `HSHELL_SYSMENU`, `HSHELL_ENDTASK`,
//! `HSHELL_ACCESSIBILITYSTATE`, `HSHELL_APPCOMMAND`,
//! `HSHELL_WINDOWREPLACED/REPLACING`, `HSHELL_MONITORCHANGED`) are about
//! the shell's own UI or carry no window; all are named by
//! `rules::events::shell_hook_name` and dropped after their trace line.
//!
//! [`WinEventHook`]: hooks::WinEventHook
//! [`PowerNotification`]: power_status::PowerNotification
//! [`EventWindow`]: window::EventWindow
//! [`ThreadTimer`]: timer::ThreadTimer

mod desktop_switch;
mod dispatch;
mod fullscreen_check;
mod hooks;
mod moves;
mod power_status;
mod rechecks;
mod recycle_bin_poll;
mod state;
mod system_settings;
mod timer;
mod tracking;
mod window;

use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};

use super::monitors;
use crate::{EventSink, PlatformError, PlatformResult};
pub use desktop_switch::DESKTOP_SWITCH_SETTLE;
pub use fullscreen_check::FULLSCREEN_SETTLE;
pub use moves::LOCATION_SETTLE;
pub use recycle_bin_poll::RECYCLE_BIN_POLL;
use recycle_bin_poll::RecycleBinPoller;
use state::ThreadState;
pub use system_settings::CLOCK_FORMAT_RECHECK;

const START_TIMEOUT: Duration = Duration::from_secs(5);
const STOP_TIMEOUT: Duration = Duration::from_secs(2);

/// Handle to the running event thread and its recycle-bin poller. Dropping
/// it without `stop` asks both threads to quit but does not wait for them;
/// prefer `stop`.
#[derive(Debug)]
pub struct EventHookThread {
    thread_id: u32,
    join: Option<JoinHandle<()>>,
    done: Receiver<()>,
    recycle_bin: Option<RecycleBinPoller>,
}

impl EventHookThread {
    pub fn start(sink: EventSink) -> PlatformResult<Self> {
        monitors::ensure_dpi_awareness();
        let (ready_tx, ready_rx) = bounded::<PlatformResult<u32>>(1);
        let (done_tx, done_rx) = bounded::<()>(1);
        let events_sink = Arc::clone(&sink);
        let join = std::thread::Builder::new()
            .name("stem-platform-events".into())
            .spawn(move || thread_main(events_sink, &ready_tx, &done_tx))
            .map_err(|e| PlatformError::Other(format!("spawn events thread: {e}")))?;
        match ready_rx.recv_timeout(START_TIMEOUT) {
            Ok(Ok(thread_id)) => {
                log::info!("events thread started (id {thread_id})");
                let mut this = Self {
                    thread_id,
                    join: Some(join),
                    done: done_rx,
                    recycle_bin: None,
                };
                match RecycleBinPoller::start(sink) {
                    Ok(poller) => this.recycle_bin = Some(poller),
                    // The dock works without the bin state; it just never
                    // flips the icon.
                    Err(e) => log::warn!("recycle-bin poll thread unavailable: {e}"),
                }
                Ok(this)
            }
            Ok(Err(e)) => {
                if join.join().is_err() {
                    log::error!("events thread panicked during setup");
                }
                Err(e)
            }
            Err(_) => {
                log::error!("events thread did not report readiness within {START_TIMEOUT:?}");
                Err(PlatformError::Timeout)
            }
        }
    }

    pub fn stop(mut self) {
        if let Some(poller) = self.recycle_bin.take() {
            poller.stop();
        }
        self.post_quit();
        match self.done.recv_timeout(STOP_TIMEOUT) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                if let Some(join) = self.join.take()
                    && join.join().is_err()
                {
                    log::error!("events thread panicked");
                }
                log::info!("events thread stopped");
            }
            Err(RecvTimeoutError::Timeout) => {
                log::warn!("events thread did not stop within {STOP_TIMEOUT:?}; detaching it");
            }
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

impl Drop for EventHookThread {
    fn drop(&mut self) {
        // The poller, if still present, is dropped with us, which drops its
        // stop sender and ends its loop.
        if self.join.is_some() {
            self.post_quit();
        }
    }
}

fn thread_main(sink: EventSink, ready: &Sender<PlatformResult<u32>>, done: &Sender<()>) {
    match ThreadState::setup(sink) {
        Ok(mut state) => {
            if ready.send(Ok(state.thread_id)).is_ok() {
                state.emit_initial();
                state.run();
            }
            log::debug!("events thread releasing hooks, timers and window");
            drop(state);
        }
        Err(e) => {
            let _ = ready.send(Err(e));
        }
    }
    let _ = done.send(());
}

#[cfg(test)]
mod tests;
