//! A pause after a slow call that has no timeout of its own.
//!
//! `windows::virtual_desktop` asks the shell
//! `IVirtualDesktopManager::IsWindowOnCurrentVirtualDesktop` on the events
//! thread, a COM call into Explorer that cannot be given a timeout. One
//! slow answer is tolerable; a slow answer on every query, while Explorer
//! is busy, would delay every window event behind it. [`SlowCallBackoff`]
//! bounds that cost: a call that took longer than
//! [`SLOW_MS`](SlowCallBackoff::SLOW_MS) starts a pause of
//! [`PAUSE_MS`](SlowCallBackoff::PAUSE_MS) during which the caller does not
//! make the call at all (it answers "unknown"); the first call after the
//! pause is made again and judged the same way. A slow Explorer therefore
//! costs at most one slow call per pause. A call that never returns is not
//! helped: the pause can only start once the call is over.
//!
//! Time is an opaque millisecond counter supplied by the caller, as for
//! `rules::events::TitleDebouncer`. Plain data, unit-tested on every host.

/// The pause policy. See the module documentation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlowCallBackoff {
    /// While paused: the time (ms) from which the call may be made again.
    paused_until: Option<u64>,
    /// Pauses started so far (the first is the one worth a warning).
    pauses: u32,
}

/// Whether to make the call now ([`SlowCallBackoff::gate`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Make the call.
    Call,
    /// Make the call: the first one after a pause.
    Resume,
    /// Do not: a pause is running. Answer "unknown".
    Skip,
}

/// What a finished call means ([`SlowCallBackoff::record`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Quick enough: keep calling.
    Quick,
    /// Too slow: no call before `until_ms`. `first` for the first pause of
    /// this policy's life (log it at warn; later ones at debug).
    Paused { until_ms: u64, first: bool },
}

impl SlowCallBackoff {
    /// A call that takes longer than this is slow.
    pub const SLOW_MS: u64 = 200;
    /// How long a slow call stops the calls.
    pub const PAUSE_MS: u64 = 30_000;

    pub const fn new() -> Self {
        Self {
            paused_until: None,
            pauses: 0,
        }
    }

    /// Whether the call may be made at `now_ms`. A pause that is over ends
    /// here, and the call that ends it is a [`Gate::Resume`].
    pub fn gate(&mut self, now_ms: u64) -> Gate {
        match self.paused_until {
            None => Gate::Call,
            Some(until) if now_ms < until => Gate::Skip,
            Some(_) => {
                self.paused_until = None;
                Gate::Resume
            }
        }
    }

    /// A call that ended at `now_ms` took `elapsed_ms`.
    pub fn record(&mut self, now_ms: u64, elapsed_ms: u64) -> Verdict {
        if elapsed_ms <= Self::SLOW_MS {
            return Verdict::Quick;
        }
        let until_ms = now_ms.saturating_add(Self::PAUSE_MS);
        self.paused_until = Some(until_ms);
        self.pauses = self.pauses.saturating_add(1);
        Verdict::Paused {
            until_ms,
            first: self.pauses == 1,
        }
    }

    /// A pause is running at `now_ms`.
    pub fn is_paused(&self, now_ms: u64) -> bool {
        self.paused_until.is_some_and(|until| now_ms < until)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn quick_calls_never_pause() {
        let mut backoff = SlowCallBackoff::new();
        for now in [0, 1_000, 2_000] {
            assert_eq!(backoff.gate(now), Gate::Call);
            assert_eq!(backoff.record(now, 5), Verdict::Quick);
        }
        // Exactly the threshold is not slow.
        assert_eq!(
            backoff.record(3_000, SlowCallBackoff::SLOW_MS),
            Verdict::Quick
        );
        assert!(!backoff.is_paused(3_000));
        assert_eq!(backoff, SlowCallBackoff::default());
    }

    #[test]
    fn a_slow_call_pauses_the_calls_for_thirty_seconds() {
        let mut backoff = SlowCallBackoff::new();
        assert_eq!(backoff.gate(10_000), Gate::Call);
        assert_eq!(
            backoff.record(10_250, 250),
            Verdict::Paused {
                until_ms: 40_250,
                first: true
            }
        );
        assert!(backoff.is_paused(10_250));
        assert_eq!(backoff.gate(10_251), Gate::Skip);
        assert_eq!(backoff.gate(40_249), Gate::Skip);
        // The pause is over: the next call is made, once marked as a resume.
        assert!(!backoff.is_paused(40_250));
        assert_eq!(backoff.gate(40_250), Gate::Resume);
        assert_eq!(backoff.record(40_260, 10), Verdict::Quick);
        assert_eq!(backoff.gate(40_300), Gate::Call);
    }

    #[test]
    fn a_still_slow_shell_costs_one_slow_call_per_pause() {
        let mut backoff = SlowCallBackoff::new();
        let mut slow_calls = 0;
        let mut first_pauses = 0;
        // A query every 100 ms for two minutes, every call taking 300 ms.
        let mut now = 0u64;
        while now < 120_000 {
            if backoff.gate(now) != Gate::Skip {
                slow_calls += 1;
                now += 300;
                if let Verdict::Paused { first: true, .. } = backoff.record(now, 300) {
                    first_pauses += 1;
                }
            }
            now += 100;
        }
        // At 0 s, then as each pause ends: 30.3, 60.6 and 90.9 s.
        assert_eq!(slow_calls, 4);
        // Only the first pause is worth a warning.
        assert_eq!(first_pauses, 1);
    }

    #[test]
    fn later_pauses_are_not_first() {
        let mut backoff = SlowCallBackoff::new();
        assert!(matches!(
            backoff.record(0, 1_000),
            Verdict::Paused { first: true, .. }
        ));
        assert_eq!(backoff.gate(SlowCallBackoff::PAUSE_MS), Gate::Resume);
        assert_eq!(
            backoff.record(SlowCallBackoff::PAUSE_MS + 500, 500),
            Verdict::Paused {
                until_ms: 2 * SlowCallBackoff::PAUSE_MS + 500,
                first: false
            }
        );
    }

    #[test]
    fn the_clock_saturates_instead_of_wrapping() {
        let mut backoff = SlowCallBackoff::new();
        assert_eq!(
            backoff.record(u64::MAX - 10, 1_000),
            Verdict::Paused {
                until_ms: u64::MAX,
                first: true
            }
        );
        assert_eq!(backoff.gate(u64::MAX - 1), Gate::Skip);
    }
}
