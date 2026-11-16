//! The default output device's volume: the readings, the
//! coalescing of notification bursts, the change filter and the retry
//! after a failed bind.
//!
//! `windows::volume` owns a thread that follows the default console render
//! endpoint (`IMMDeviceEnumerator::GetDefaultAudioEndpoint(eRender,
//! eConsole)`) through its `IAudioEndpointVolume`. It hands the two values
//! it reads to [`status_of`], turns a percent into the scalar it sets with
//! [`scalar_of`], asks [`Throttle`] when a notification may be reported,
//! reports a reading only when [`ChangeFilter`] says it changed, asks
//! [`is_console_render`] whether a default-device change concerns it, and
//! [`retry_after_ms`] when to try a failed bind again. The rules:
//!
//! - **Percent.** The endpoint's master level is a scalar from 0.0 to 1.0,
//!   the scale Windows' own slider shows ×100. The percent is that ×100,
//!   rounded to the nearest whole number (half away from zero); a value
//!   outside the range is clamped first and one that is not a number is 0.
//!   A percent above 100 sets 1.0.
//! - **Coalescing.** A slider drag fires many notifications. At most one
//!   reading is reported per [`Throttle::INTERVAL_MS`] (50 ms): a
//!   notification after a quiet interval is reported at once (the leading
//!   edge); one inside the interval is reported when it ends (the trailing
//!   edge), once for the whole burst and with the value read then, so the
//!   last value always arrives, at most 50 ms late.
//! - **Change filter.** A reading is reported when it is the first or
//!   differs from the last one reported, `None` (no device) included.
//! - **Default-device changes.** Only the console render role is followed:
//!   a change of another flow (capture) or role (multimedia,
//!   communications) is not this thread's business. A device's state
//!   change, arrival or removal always makes the thread look at the default
//!   again (`windows::volume` re-binds only when the default's id differs).
//! - **Retry.** Every failed bind is tried again on a timer, so recovery
//!   never depends on a notification alone. One that failed because there
//!   is no output device (`E_NOTFOUND`) is tried again every 60 s (one
//!   wake-up a minute while there is no device; a device notification ends
//!   the wait sooner). Any other failure (the enumerator or its
//!   notification client could not be set up, the audio service is not
//!   running) is retried after 2 s, then 4 s, 8 s and so on, at most every
//!   60 s, until a bind succeeds; the thread logs the first failure of a
//!   streak at `warn` and the rest at `debug`. Every timed retry builds the
//!   enumerator and its notification client again before it binds: a
//!   restart of the audio service under Stem may leave them dead, and a
//!   dead registration never calls back. A failure other than no device
//!   drops them at once ([`drops_watch_at_once`]); no device keeps them
//!   until the retry, so a device that comes meanwhile is noticed at once.
//!   While a device is bound and reads, there is no timer at all.
//!
//! Time is an opaque millisecond counter supplied by the caller, as for
//! `rules::backoff`. Plain numbers, so it is unit-tested on every host. The
//! `EDataFlow` / `ERole` values and the HRESULT mirror the SDK;
//! `windows/volume.rs` asserts at compile time that they equal its values.

use stem_core::VolumeStatus;

/// `EDataFlow::eRender`: audio rendering (output).
pub const E_RENDER: i32 = 0;
/// `ERole::eConsole`: games, system sounds, voice commands; what the
/// volume keys and the Quick Settings slider drive.
pub const E_CONSOLE: i32 = 0;
/// `E_NOTFOUND` as `IMMDeviceEnumerator::GetDefaultAudioEndpoint` returns
/// it for "no device is available": `HRESULT_FROM_WIN32(ERROR_NOT_FOUND)`.
pub const E_NOTFOUND: u32 = 0x8007_0490;

/// The percent for the endpoint's master level `scalar` (0.0 to 1.0).
pub fn percent_of(scalar: f32) -> u8 {
    if scalar.is_nan() {
        return 0;
    }
    // In range after the clamp, so the cast neither truncates nor wraps.
    (scalar.clamp(0.0, 1.0) * 100.0).round() as u8
}

/// The master level to set for `percent` (more than 100 is 100).
pub fn scalar_of(percent: u8) -> f32 {
    f32::from(percent.min(100)) / 100.0
}

/// The reading for a master level `scalar` and a mute state.
pub fn status_of(scalar: f32, muted: bool) -> VolumeStatus {
    VolumeStatus {
        percent: percent_of(scalar),
        muted,
    }
}

/// Whether an `IMMNotificationClient::OnDefaultDeviceChanged` for `flow`
/// and `role` (their raw values) names the endpoint the thread follows.
pub fn is_console_render(flow: i32, role: i32) -> bool {
    flow == E_RENDER && role == E_CONSOLE
}

/// Whether a failed call's HRESULT (as `u32`) says there is no output
/// device, as opposed to a failure worth trying again.
pub fn is_no_device(code: u32) -> bool {
    code == E_NOTFOUND
}

/// The coalescing of notification bursts: see the module documentation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Throttle {
    /// When the last reading was reported.
    last: Option<u64>,
    /// When the pending report is due, if one is pending.
    due: Option<u64>,
}

impl Throttle {
    /// The shortest time between two reports.
    pub const INTERVAL_MS: u64 = 50;

    pub const fn new() -> Self {
        Self {
            last: None,
            due: None,
        }
    }

    /// Something changed at `now_ms`. `true`: read and report now. `false`:
    /// a report is due later ([`Self::deadline`]), one already pending
    /// included, which will read the value then.
    pub fn notify(&mut self, now_ms: u64) -> bool {
        if self.due.is_some() {
            return false;
        }
        match self.last {
            Some(last) if now_ms < last.saturating_add(Self::INTERVAL_MS) => {
                self.due = Some(last.saturating_add(Self::INTERVAL_MS));
                false
            }
            _ => true,
        }
    }

    /// When the pending report is due, if one is pending.
    pub fn deadline(&self) -> Option<u64> {
        self.due
    }

    /// Whether the pending report is due at `now_ms`; it is no longer
    /// pending once this says so.
    pub fn take_due(&mut self, now_ms: u64) -> bool {
        match self.due {
            Some(due) if now_ms >= due => {
                self.due = None;
                true
            }
            _ => false,
        }
    }

    /// A reading was reported at `now_ms` (one the change filter let
    /// through; a reading that changed nothing reports nothing and does not
    /// count).
    pub fn reported(&mut self, now_ms: u64) {
        self.last = Some(now_ms);
    }
}

/// The thread's change filter: a reading is reported when it is the first
/// or differs from the last one reported.
#[derive(Debug, Default)]
pub struct ChangeFilter {
    last: Option<Option<VolumeStatus>>,
}

impl ChangeFilter {
    /// Whether `reading` is to be reported; it becomes the last one if so.
    pub fn observe(&mut self, reading: Option<VolumeStatus>) -> bool {
        if self.last == Some(reading) {
            return false;
        }
        self.last = Some(reading);
        true
    }
}

/// Why a bind of the default output device failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindFailure {
    /// There is no output device (`E_NOTFOUND`), and the device
    /// notifications are registered, so they may say when one comes.
    NoDevice,
    /// Anything else, or no device while the notifications are not
    /// registered.
    Other,
}

/// The first delay before a failed bind is tried again.
pub const RETRY_FIRST_MS: u64 = 2_000;
/// The longest delay between two tries, and the delay after no device.
pub const RETRY_MAX_MS: u64 = 60_000;

/// How long after the `failures`-th failure in a row (1 for the first) the
/// thread tries to bind again, building the enumerator and its client
/// afresh: [`RETRY_MAX_MS`] for [`BindFailure::NoDevice`] (a slow look for
/// a device no notification announced, the registration perhaps dead),
/// else [`RETRY_FIRST_MS`] doubled for each failure before this one, at
/// most [`RETRY_MAX_MS`]. Never "wait for a callback": every failure has a
/// delay.
pub fn retry_after_ms(failure: BindFailure, failures: u32) -> u64 {
    match failure {
        BindFailure::NoDevice => RETRY_MAX_MS,
        BindFailure::Other => {
            let doublings = failures.saturating_sub(1).min(16);
            RETRY_FIRST_MS
                .saturating_mul(1 << doublings)
                .min(RETRY_MAX_MS)
        }
    }
}

/// Whether a failed bind drops the enumerator and its notification client
/// at once rather than at the retry: for every failure but
/// [`BindFailure::NoDevice`], whose client is kept until the retry so a
/// device that comes meanwhile is noticed at once. Either way the retry
/// builds them again.
pub fn drops_watch_at_once(failure: BindFailure) -> bool {
    failure == BindFailure::Other
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn the_sdk_values_are_the_documented_ones() {
        // mmdeviceapi.h: eRender = 0, eConsole = 0; winerror.h: ERROR_NOT_FOUND
        // is 1168 (0x490) and HRESULT_FROM_WIN32 puts it in FACILITY_WIN32.
        assert_eq!(E_RENDER, 0);
        assert_eq!(E_CONSOLE, 0);
        assert_eq!(E_NOTFOUND, 0x8007_0000 | 1168);
    }

    #[test]
    fn the_percent_is_the_scalar_times_100_rounded_and_clamped() {
        for (scalar, percent) in [
            (0.0, 0),
            (1.0, 100),
            (0.45, 45),
            // What a level set by steps of 0.02 reads back as.
            (0.439_999_98, 44),
            (0.460_000_04, 46),
            (0.004, 0),
            (0.006, 1),
            (0.994, 99),
            (0.996, 100),
            // Exactly half way (0.125 is exact in binary): away from zero.
            (0.125, 13),
            // Out of range, or not a number: clamped, or nothing.
            (-0.2, 0),
            (1.7, 100),
            (f32::INFINITY, 100),
            (f32::NEG_INFINITY, 0),
            (f32::NAN, 0),
        ] {
            assert_eq!(percent_of(scalar), percent, "{scalar}");
        }
        assert_eq!(
            status_of(0.6, true),
            VolumeStatus {
                percent: 60,
                muted: true
            }
        );
    }

    #[test]
    fn every_percent_survives_the_round_trip_through_the_scalar() {
        for percent in 0..=100u8 {
            assert_eq!(percent_of(scalar_of(percent)), percent);
        }
        assert_eq!(scalar_of(0), 0.0);
        assert_eq!(scalar_of(100), 1.0);
        assert_eq!(scalar_of(250), 1.0);
    }

    #[test]
    fn only_the_console_render_role_is_followed() {
        // eRender/eCapture = 0/1; eConsole/eMultimedia/eCommunications = 0/1/2.
        assert!(is_console_render(0, 0));
        for (flow, role) in [(0, 1), (0, 2), (1, 0), (1, 1), (2, 0)] {
            assert!(!is_console_render(flow, role), "{flow} {role}");
        }
        assert!(is_no_device(0x8007_0490));
        // AUDCLNT_E_SERVICE_NOT_RUNNING, E_ACCESSDENIED: worth a retry.
        assert!(!is_no_device(0x8889_0010));
        assert!(!is_no_device(0x8007_0005));
    }

    #[test]
    fn a_change_after_a_quiet_interval_is_reported_at_once() {
        let mut t = Throttle::new();
        assert!(t.notify(1_000));
        t.reported(1_000);
        assert_eq!(t.deadline(), None);
        // 50 ms later or more: at once again.
        assert!(t.notify(1_050));
        t.reported(1_050);
        assert!(t.notify(5_000));
    }

    #[test]
    fn a_burst_is_reported_once_at_the_end_of_the_interval_with_the_last_value() {
        let mut t = Throttle::new();
        assert!(t.notify(0));
        t.reported(0);
        // A slider drag: a notification every 5 ms. Only the first after
        // the report arms the deadline; the rest wait for it.
        assert!(!t.notify(5));
        assert_eq!(t.deadline(), Some(50));
        for ms in (10..50).step_by(5) {
            assert!(!t.notify(ms));
            assert_eq!(t.deadline(), Some(50));
        }
        assert!(!t.take_due(49));
        assert!(t.take_due(50));
        assert_eq!(t.deadline(), None);
        t.reported(50);
        // The drag goes on: the next report is due 50 ms after that one.
        assert!(!t.notify(52));
        assert_eq!(t.deadline(), Some(100));
        // A late wake-up still reports, once.
        assert!(t.take_due(130));
        assert!(!t.take_due(131));
        t.reported(130);
        assert!(!t.notify(140));
        assert_eq!(t.deadline(), Some(180));
    }

    #[test]
    fn reports_are_at_least_an_interval_apart_and_the_last_change_is_never_lost() {
        // A notification every 7 ms for 300 ms, a report whenever allowed:
        // every report at least 50 ms after the one before, and one at or
        // after the last notification.
        let mut t = Throttle::new();
        let mut reports = Vec::new();
        for now in 0..400u64 {
            if now <= 300 && now % 7 == 0 && t.notify(now) {
                t.reported(now);
                reports.push(now);
            }
            if t.take_due(now) {
                t.reported(now);
                reports.push(now);
            }
        }
        assert!(
            reports
                .windows(2)
                .all(|w| w[1] - w[0] >= Throttle::INTERVAL_MS)
        );
        assert!(reports.last().is_some_and(|&last| last >= 294));
        assert_eq!(t.deadline(), None);
    }

    #[test]
    fn a_reading_that_changed_nothing_does_not_start_an_interval() {
        let mut t = Throttle::new();
        assert!(t.notify(0));
        // Read, but the filter dropped it: no `reported`.
        assert!(t.notify(10));
        t.reported(10);
        assert!(!t.notify(20));
    }

    #[test]
    fn the_filter_reports_the_first_reading_and_then_only_changes() {
        let level = |percent, muted| Some(VolumeStatus { percent, muted });
        let mut f = ChangeFilter::default();
        // The first reading goes out, "no device" included.
        assert!(f.observe(None));
        assert!(!f.observe(None));
        assert!(f.observe(level(60, false)));
        assert!(!f.observe(level(60, false)));
        assert!(f.observe(level(62, false)));
        assert!(f.observe(level(62, true)));
        assert!(f.observe(None));
        assert!(f.observe(level(62, true)));
        let mut first = ChangeFilter::default();
        assert!(first.observe(level(10, false)));
    }

    #[test]
    fn a_missing_device_is_looked_for_every_minute_and_other_failures_back_off() {
        // No device: one try a minute, however long the streak, so a
        // registration the audio service's restart left dead (which never
        // calls back) cannot leave the thread waiting for good.
        for failures in [0, 1, 2, 9, u32::MAX] {
            assert_eq!(retry_after_ms(BindFailure::NoDevice, failures), 60_000);
        }
        let delays: Vec<u64> = (1..=8)
            .map(|n| retry_after_ms(BindFailure::Other, n))
            .collect();
        assert_eq!(
            delays,
            [2_000, 4_000, 8_000, 16_000, 32_000, 60_000, 60_000, 60_000]
        );
        // A count of 0 is read as the first; a huge one stays at the cap.
        assert_eq!(retry_after_ms(BindFailure::Other, 0), 2_000);
        assert_eq!(retry_after_ms(BindFailure::Other, u32::MAX), 60_000);
        // Never sooner than the first delay nor later than the cap.
        for failure in [BindFailure::NoDevice, BindFailure::Other] {
            for failures in 0..=20 {
                let delay = retry_after_ms(failure, failures);
                assert!((RETRY_FIRST_MS..=RETRY_MAX_MS).contains(&delay));
            }
        }
    }

    #[test]
    fn only_a_failure_other_than_no_device_drops_the_enumerator_at_once() {
        // The audio service restarted: the old enumerator and client may be
        // dead, so they go now and the retry starts from scratch.
        assert!(drops_watch_at_once(BindFailure::Other));
        // No device: the client stays until the retry (which builds it
        // again), so a device that comes meanwhile is noticed at once.
        assert!(!drops_watch_at_once(BindFailure::NoDevice));
    }
}
