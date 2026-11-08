//! Which virtual desktop a window is on.
//!
//! Contract:
//! - [`DesktopQuery::is_window_on_current_desktop`] asks the shell's
//!   `IVirtualDesktopManager::IsWindowOnCurrentVirtualDesktop` about a
//!   top-level window: `Some(true)` / `Some(false)`, or `None` when the
//!   question cannot be asked (no manager on this thread, no shell) or the
//!   shell refuses the window (another session, a protected process, a
//!   handle that died meanwhile). `enumerate` puts the answer in
//!   `RawWindow::on_current_desktop`; the engine lets it decide
//!   `behavior.showOnlyCurrentDesktop` and falls back to the cloak state
//!   for `None`.
//! - Eligibility does not change: a shell-cloaked window (parked on another
//!   desktop) is still reported, with `is_cloaked = true`. This module only
//!   says *where* it is, which the cloak flags cannot: the shell also cloaks
//!   windows during a desktop-switch animation, suspended Store apps, and
//!   the `ApplicationFrameWindow` ghost a closed Store app can leave.
//!
//! # Design
//!
//! The manager is a COM object (`CoCreateInstance(VirtualDesktopManager)`)
//! and COM objects are bound to the apartment that created them, so one is
//! kept **per thread**, in a thread-local slot, and only on threads that
//! opted in with [`ThreadManager::enable`] inside a COM apartment that
//! outlives the guard: the events thread (`windows::events`), which runs
//! every query behind `WindowAdded`, `WindowUpdated` and `WindowsSnapshot`.
//! The guard empties the slot when dropped, before the thread's
//! `ComApartment` goes (field order in `ThreadState`), so the manager is
//! never released after `CoUninitialize` and no COM call runs in a
//! thread-local destructor (Windows runs those under the loader lock). The
//! manager is created lazily, by the first query on the thread; a failure
//! to create it is logged once per thread at debug and tried again by the
//! next query context (one fast failing call each), so a shell that was
//! not ready at start is picked up later. A full enumeration
//! ([`DesktopQuery::renewed`], the snapshots) replaces the thread's manager
//! with a fresh one, so a manager left behind by a restarted Explorer does
//! not outlive the next snapshot. A thread without a guard (the identity
//! thread's `identify_window`, `Platform::windows()` from anywhere else)
//! gets `None` for every window: the facts it needs do not include this
//! one, and the engine's fallback covers the rest.
//!
//! The per-window call is made on every query that needs it (an `HWND` is
//! reused once its window is destroyed, so an old answer cannot be kept
//! for good), except the events thread's move path: a window that
//! moves does not change desktop (a move to another desktop is a cloak
//! event, which asks), so `enumerate::query_moved_window` reuses the
//! answer last reported about it and the shell is not asked at all while a
//! window is dragged. A refusal is logged at debug once per window
//! (`rules::events::DesktopQueryFailures`, shared by every thread like
//! `rules::process_access`) and at trace after that; a later success
//! forgets the refusal.
//!
//! # Not bounded, only paused
//!
//! `IsWindowOnCurrentVirtualDesktop` is a COM call into Explorer and COM
//! offers no timeout for it: a call that Explorer does not answer blocks
//! the calling thread (the events thread) until Explorer answers or exits.
//! What is bounded is the cost of a *slow* Explorer: each call is timed
//! (`Instant` around it), and one that took longer than 200 ms stops the
//! calls for the next 30 s (`rules::backoff::SlowCallBackoff`, shared by
//! every thread): every answer is then `None`, so the engine falls back to
//! the cloak state, and the first query after the pause asks again. The
//! first pause is logged at warn (`IsWindowOnCurrentVirtualDesktop took …;
//! not asking for 30 s`), later ones at debug, the end of each at debug
//! (`asking the shell again`). So a slow Explorer costs at most one slow
//! call per 30 s; a hung one still blocks the events thread once, for as
//! long as it hangs. The manager's creation (`CoCreateInstance`, at each
//! snapshot and after a failed creation) is neither timed nor paused.
//! Running the calls on a helper thread with a timeout would bound the
//! hung case too.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::Win32::UI::Shell::{IVirtualDesktopManager, VirtualDesktopManager};

use super::util;
use crate::rules::backoff::{Gate, SlowCallBackoff, Verdict};
use crate::rules::events::DesktopQueryFailures;

/// The calling thread's manager slot. See the module documentation.
enum Slot {
    /// No [`ThreadManager`] on this thread: every answer is `None`.
    Off,
    /// Enabled; the next query context creates the manager.
    Pending {
        /// A creation failure has been logged on this thread.
        failure_logged: bool,
    },
    Ready(IVirtualDesktopManager),
}

thread_local! {
    static SLOT: RefCell<Slot> = const { RefCell::new(Slot::Off) };
}

/// Windows whose refusal has been logged, shared by every thread
/// that queries windows so the line appears once per window.
static FAILURES: Mutex<DesktopQueryFailures> = Mutex::new(DesktopQueryFailures::new());

/// The pause after a slow call (see *Not bounded, only paused*), shared by
/// every thread: a slow Explorer is slow for all of them.
static BACKOFF: Mutex<SlowCallBackoff> = Mutex::new(SlowCallBackoff::new());

/// The origin of [`BACKOFF`]'s millisecond clock.
static CLOCK: OnceLock<Instant> = OnceLock::new();

/// Keeps the calling thread's manager slot open while alive. Create it on a
/// thread that holds a COM apartment (`util::ComApartment`) for longer than
/// this guard, and drop it before the apartment. `!Send`: the slot is the
/// thread's own.
pub struct ThreadManager {
    _not_send: PhantomData<*const ()>,
}

impl ThreadManager {
    pub fn enable() -> Self {
        let _ = SLOT.try_with(|slot| {
            let mut slot = slot.borrow_mut();
            if matches!(*slot, Slot::Off) {
                *slot = Slot::Pending {
                    failure_logged: false,
                };
            }
        });
        Self {
            _not_send: PhantomData,
        }
    }
}

impl Drop for ThreadManager {
    fn drop(&mut self) {
        // Releases a `Ready` manager now, inside the thread's apartment.
        let _ = SLOT.try_with(|slot| *slot.borrow_mut() = Slot::Off);
    }
}

impl std::fmt::Debug for ThreadManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ThreadManager")
    }
}

/// The answers for one query context (`enumerate::QueryContext`): a
/// reference (`AddRef`) to the thread's manager when the thread has one,
/// released with this value.
pub struct DesktopQuery {
    manager: Option<IVirtualDesktopManager>,
}

impl DesktopQuery {
    /// For the calling thread; creates the thread's manager when it has a
    /// [`ThreadManager`] and none yet.
    pub fn for_this_thread() -> Self {
        Self::take(false)
    }

    /// Like [`for_this_thread`](Self::for_this_thread), but a thread that
    /// has a manager gets a fresh one (for a full enumeration).
    pub fn renewed() -> Self {
        Self::take(true)
    }

    fn take(renew: bool) -> Self {
        let manager = SLOT
            .try_with(|slot| {
                let mut slot = slot.borrow_mut();
                if renew && matches!(*slot, Slot::Ready(_)) {
                    *slot = Slot::Pending {
                        failure_logged: false,
                    };
                }
                if let Slot::Pending { failure_logged } = *slot {
                    match create_manager() {
                        Ok(manager) => {
                            log::debug!("virtual desktop manager created for this thread");
                            *slot = Slot::Ready(manager);
                        }
                        Err(e) => {
                            if failure_logged {
                                log::trace!("virtual desktop manager still unavailable: {e}");
                            } else {
                                log::debug!(
                                    "virtual desktop manager unavailable ({e}); windows on \
                                     other desktops are told apart by their cloak state until \
                                     it can be created"
                                );
                            }
                            *slot = Slot::Pending {
                                failure_logged: true,
                            };
                        }
                    }
                }
                match &*slot {
                    Slot::Ready(manager) => Some(manager.clone()),
                    Slot::Off | Slot::Pending { .. } => None,
                }
            })
            .ok()
            .flatten();
        Self { manager }
    }

    /// Whether this context can ask the shell at all.
    pub fn is_available(&self) -> bool {
        self.manager.is_some()
    }

    /// `Some(true)` / `Some(false)` from the shell; `None` without a manager
    /// on this thread, while a pause after a slow call runs, or when the
    /// shell refuses the window (logged once per window, see the module
    /// documentation).
    pub fn is_window_on_current_desktop(&self, hwnd: HWND) -> Option<bool> {
        let manager = self.manager.as_ref()?;
        let id = util::window_id(hwnd).0;
        match backoff().gate(now_ms()) {
            Gate::Call => {}
            Gate::Resume => log::debug!(
                "the pause after a slow IsWindowOnCurrentVirtualDesktop is over; asking the \
                 shell again"
            ),
            Gate::Skip => {
                log::trace!("window {id:#x}: not asking the shell (paused after a slow call)");
                return None;
            }
        }
        let started = Instant::now();
        // SAFETY: `manager` is a live interface created in this thread's
        // apartment; the call accepts any handle value and reports a dead
        // or refused one through the Result.
        let answer = unsafe { manager.IsWindowOnCurrentVirtualDesktop(hwnd) };
        note_duration(id, started.elapsed());
        match answer {
            Ok(on_current) => {
                failures().clear(id);
                Some(on_current.as_bool())
            }
            Err(e) => {
                if failures().note(id) {
                    log::debug!(
                        "window {id:#x}: IsWindowOnCurrentVirtualDesktop failed ({e}); its \
                         desktop is unknown and its cloak state decides (logged once per window)"
                    );
                } else {
                    log::trace!(
                        "window {id:#x}: IsWindowOnCurrentVirtualDesktop failed ({e}), already \
                         logged"
                    );
                }
                None
            }
        }
    }
}

impl std::fmt::Debug for DesktopQuery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DesktopQuery")
            .field("available", &self.is_available())
            .finish()
    }
}

/// `CoCreateInstance(VirtualDesktopManager)`. The caller's thread holds a
/// COM apartment (the [`ThreadManager`] contract).
fn create_manager() -> windows::core::Result<IVirtualDesktopManager> {
    // SAFETY: a valid CLSID, no aggregation; the interface is owned by us
    // and released by the slot that stores it, on this thread.
    unsafe { CoCreateInstance(&VirtualDesktopManager, None, CLSCTX_ALL) }
}

/// The shared refusal memory; a poisoned lock (a panic while held, which
/// nothing here can cause) still yields the set.
fn failures() -> MutexGuard<'static, DesktopQueryFailures> {
    FAILURES.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The shared pause; a poisoned lock still yields it.
fn backoff() -> MutexGuard<'static, SlowCallBackoff> {
    BACKOFF.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whether the shell is not being asked right now, after a slow call (for
/// the enumeration's log line).
pub fn is_paused() -> bool {
    backoff().is_paused(now_ms())
}

/// Milliseconds since the first use of [`BACKOFF`]'s clock.
fn now_ms() -> u64 {
    let elapsed = CLOCK.get_or_init(Instant::now).elapsed();
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
}

/// Feed one call's duration to the pause and log a pause that it starts.
fn note_duration(id: u64, elapsed: Duration) {
    let elapsed_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
    let Verdict::Paused { first, .. } = backoff().record(now_ms(), elapsed_ms) else {
        return;
    };
    let pause = Duration::from_millis(SlowCallBackoff::PAUSE_MS);
    if first {
        log::warn!(
            "IsWindowOnCurrentVirtualDesktop took {elapsed:?} for window {id:#x} (Explorer is \
             slow to answer); not asking for {pause:?}: windows on other desktops are told \
             apart by their cloak state meanwhile (logged once at warn; later pauses at debug)"
        );
    } else {
        log::debug!(
            "IsWindowOnCurrentVirtualDesktop took {elapsed:?} for window {id:#x}; not asking \
             for {pause:?}"
        );
    }
}
