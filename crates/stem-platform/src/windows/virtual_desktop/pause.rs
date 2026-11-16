//! The pause after a slow `IsWindowOnCurrentVirtualDesktop` call (*Not
//! bounded, only paused* in the parent module).

use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use crate::rules::backoff::{SlowCallBackoff, Verdict};

/// The pause after a slow call (see *Not bounded, only paused*), shared by
/// every thread: a slow Explorer is slow for all of them.
static BACKOFF: Mutex<SlowCallBackoff> = Mutex::new(SlowCallBackoff::new());

/// The origin of [`BACKOFF`]'s millisecond clock.
static CLOCK: OnceLock<Instant> = OnceLock::new();

/// The shared pause; a poisoned lock still yields it.
pub(super) fn backoff() -> MutexGuard<'static, SlowCallBackoff> {
    BACKOFF.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whether the shell is not being asked right now, after a slow call (for
/// the enumeration's log line).
pub fn is_paused() -> bool {
    backoff().is_paused(now_ms())
}

/// Milliseconds since the first use of [`BACKOFF`]'s clock.
pub(super) fn now_ms() -> u64 {
    let elapsed = CLOCK.get_or_init(Instant::now).elapsed();
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
}

/// Feed one call's duration to the pause and log a pause that it starts.
pub(super) fn note_duration(id: u64, elapsed: Duration) {
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
