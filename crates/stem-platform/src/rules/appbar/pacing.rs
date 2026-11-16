//! When a bar's messages go out: a request waits for its window to settle,
//! the repositions the shell's notifications cause are bounded, and a band
//! that could not be taken is asked for again after a delay.

use crate::AppBarRequest;

/// Requests waiting for their window to settle. A dock window's reports
/// come in bursts (an icon-size slider, a drag that grows the window, a
/// DPI change, a panel rect still laid out for the old edge for a frame):
/// each request for a window replaces the one waiting and starts the wait
/// again, and only a request that has waited [`Settle::DELAY_MS`] without a
/// newer one is applied, so a burst costs one `ABM_SETPOS` (each of which
/// resizes every maximised window on the monitor). Releases do not wait;
/// the caller applies them at once and [`Settle::cancel`]s what waits.
///
/// Keyed by the caller's window key; time is an opaque millisecond counter
/// supplied by the caller, as for `rules::backoff`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settle<K> {
    /// `(key, request, due)`, at most one per key.
    pending: Vec<(K, AppBarRequest, u64)>,
}

impl<K> Default for Settle<K> {
    fn default() -> Self {
        Self {
            pending: Vec::new(),
        }
    }
}

impl<K: Copy + PartialEq> Settle<K> {
    /// How long a request waits for a newer one.
    pub const DELAY_MS: u64 = 250;

    /// `request` for `key` at `now_ms`: it replaces the one waiting for
    /// `key` and is due [`Settle::DELAY_MS`] later. The same request again
    /// keeps the time it is due.
    pub fn push(&mut self, key: K, request: AppBarRequest, now_ms: u64) {
        let due = now_ms.saturating_add(Self::DELAY_MS);
        match self.pending.iter_mut().find(|(k, _, _)| *k == key) {
            Some(entry) if entry.1 == request => {}
            Some(entry) => *entry = (key, request, due),
            None => self.pending.push((key, request, due)),
        }
    }

    /// Drop what waits for `key`; whether anything did.
    pub fn cancel(&mut self, key: K) -> bool {
        let before = self.pending.len();
        self.pending.retain(|(k, _, _)| *k != key);
        self.pending.len() != before
    }

    /// The requests due at `now_ms`, oldest due first; they stop waiting.
    pub fn take_due(&mut self, now_ms: u64) -> Vec<(K, AppBarRequest)> {
        let mut due: Vec<(K, AppBarRequest, u64)> = Vec::new();
        self.pending.retain(|&(key, request, at)| {
            if at <= now_ms {
                due.push((key, request, at));
                false
            } else {
                true
            }
        });
        due.sort_by_key(|&(_, _, at)| at);
        due.into_iter()
            .map(|(key, request, _)| (key, request))
            .collect()
    }

    /// When the next request is due, if any waits.
    pub fn next_due(&self) -> Option<u64> {
        self.pending.iter().map(|&(_, _, at)| at).min()
    }

    /// Whether a request waits for `key`.
    pub fn is_waiting(&self, key: K) -> bool {
        self.pending.iter().any(|(k, _, _)| *k == key)
    }
}

/// What [`RequeryGuard::allow`] says about one reposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requery {
    /// Go ahead.
    Go,
    /// No: too many in a row; a pause starts now (log it at warn).
    Pause,
    /// No: a pause is running.
    Paused,
}

/// A bound on the repositions (`ABM_SETPOS`) the shell's notifications
/// (`ABN_POSCHANGED`, `ABN_STATECHANGE`) cause for one bar. A notification
/// whose `ABM_QUERYPOS` answer is the band the bar already asked for is not
/// a reposition and does not count, which ends the ordinary exchange
/// between bars at once; this guard is for two bars that would keep moving
/// each other (another app's bar on the same edge that answers every
/// notification with a different band): at most [`RequeryGuard::BURST`]
/// repositions within [`RequeryGuard::WINDOW_MS`], then none for
/// [`RequeryGuard::PAUSE_MS`]. Requests of the app layer are not bounded by
/// it. Time is an opaque millisecond counter supplied by the caller.
///
/// A reposition refused while paused is not forgotten: the bar then owes
/// one query when the pause ends ([`RequeryGuard::owed_at`]), so a band the
/// shell moved during the pause is followed once it is over, even if no
/// notification comes after it. The guard is a circuit breaker that closes
/// again by itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequeryGuard {
    /// When the current window of counting started.
    window_start: u64,
    /// Repositions let through in the current window.
    count: u32,
    /// While paused: the time from which repositions may go again.
    paused_until: Option<u64>,
    /// Whether a reposition was refused during the pause and the band has
    /// not been seen back where the bar asked for it since.
    owed: bool,
}

impl RequeryGuard {
    pub const BURST: u32 = 8;
    pub const WINDOW_MS: u64 = 2_000;
    pub const PAUSE_MS: u64 = 10_000;

    /// Whether a reposition at `now_ms` may go ahead; counts the ones it
    /// lets through. A refusal ([`Requery::Pause`], [`Requery::Paused`])
    /// leaves a query owed for the end of the pause.
    pub fn allow(&mut self, now_ms: u64) -> Requery {
        if let Some(until) = self.paused_until {
            if now_ms < until {
                self.owed = true;
                return Requery::Paused;
            }
            self.paused_until = None;
            self.count = 0;
            self.owed = false;
        }
        if self.count == 0 || now_ms.saturating_sub(self.window_start) >= Self::WINDOW_MS {
            self.window_start = now_ms;
            self.count = 0;
        }
        if self.count >= Self::BURST {
            self.paused_until = Some(now_ms.saturating_add(Self::PAUSE_MS));
            self.owed = true;
            return Requery::Pause;
        }
        self.count += 1;
        Requery::Go
    }

    /// When the bar owes a query: the end of the pause, if a reposition was
    /// refused during it and the band has not caught up since; `None` when
    /// nothing is owed.
    pub fn owed_at(&self) -> Option<u64> {
        if self.owed { self.paused_until } else { None }
    }

    /// Whether the query owed is due at `now_ms`; it stops being owed (the
    /// caller queries now, and [`RequeryGuard::allow`] then ends the pause).
    pub fn take_owed(&mut self, now_ms: u64) -> bool {
        match self.owed_at() {
            Some(at) if at <= now_ms => {
                self.owed = false;
                true
            }
            _ => false,
        }
    }

    /// The band is where the bar asked for it (or the bar holds none):
    /// nothing is owed. The pause, if one runs, goes on.
    pub fn caught_up(&mut self) {
        self.owed = false;
    }
}

/// The wait before a bar that wants a band but could not take one tries
/// again: the shell refused `ABM_NEW`, was not running, or answered
/// `ABM_SETPOS` with a rect that is not a band on the request's monitor
/// ([`is_usable`]; a display change racing the request, say). Without it a
/// band given up would stay given up until something else changed the
/// request. [`Retry::DELAYS_MS`] grows with each failure in a row and stays
/// at its last value; a band taken, a new request or a release starts it
/// over ([`Retry::reset`]). Time is an opaque millisecond counter supplied
/// by the caller.
///
/// [`is_usable`]: super::is_usable
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Retry {
    /// Attempts that failed in a row.
    failures: u32,
    /// When the next attempt is due, if one is.
    due: Option<u64>,
}

impl Retry {
    /// The wait after the first, second, third and every later failure in
    /// a row.
    pub const DELAYS_MS: [u64; 4] = [1_000, 5_000, 30_000, 60_000];

    /// An attempt failed at `now_ms`: the next is due one delay later. The
    /// delay.
    pub fn failed(&mut self, now_ms: u64) -> u64 {
        let index = usize::try_from(self.failures)
            .unwrap_or(usize::MAX)
            .min(Self::DELAYS_MS.len() - 1);
        let delay = Self::DELAYS_MS[index];
        self.failures = self.failures.saturating_add(1);
        self.due = Some(now_ms.saturating_add(delay));
        delay
    }

    /// Attempts that failed in a row.
    pub fn failures(&self) -> u32 {
        self.failures
    }

    /// When the next attempt is due, if one is.
    pub fn due(&self) -> Option<u64> {
        self.due
    }

    /// Whether an attempt is due at `now_ms`; it stops being due.
    pub fn take_due(&mut self, now_ms: u64) -> bool {
        match self.due {
            Some(at) if at <= now_ms => {
                self.due = None;
                true
            }
            _ => false,
        }
    }

    /// Forget the failures and the attempt due.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}
