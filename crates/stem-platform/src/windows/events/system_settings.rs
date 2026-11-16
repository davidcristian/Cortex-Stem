//! Theme, regional format and monitor changes.
//!
//! Contract:
//! - Clock format: `WM_SETTINGCHANGE` whose `lParam` names the
//!   `intl` area (`rules::locale::is_intl_area`; the window procedure
//!   copies at most [`SETTING_AREA_MAX`] units of the string out while it
//!   is valid) re-reads `locale::current` at once and once more
//!   [`CLOCK_FORMAT_RECHECK`] later (one one-shot timer, re-armed by each
//!   such broadcast), and emits `ClockFormatChanged` only when the reading
//!   differs from the last one emitted (`rules::locale::ChangeFilter`); the
//!   first reading is emitted right after the start snapshot, after the
//!   power reading. The second read is there because Microsoft does not document
//!   that the process's copy of the user's locale data is current when the
//!   broadcast arrives: a stale first read would equal the last reading and
//!   be dropped. Debug line per emission: `clock format: <reading>
//!   (<cause>)`.
//!
//! [`SETTING_AREA_MAX`]: super::window::SETTING_AREA_MAX

use std::time::Duration;

use stem_core::PlatformEvent;

use super::state::ThreadState;
use super::timer::ThreadTimer;
use crate::windows::{locale, monitors, theme};

/// How long after a `WM_SETTINGCHANGE` for `intl` the regional format is
/// read a second time: late enough for locale data that was not yet
/// current at the first read, soon enough that the clock follows within a
/// second.
pub const CLOCK_FORMAT_RECHECK: Duration = Duration::from_millis(500);

impl ThreadState {
    pub(super) fn emit_monitors(&self) {
        match monitors::enumerate_monitors() {
            Ok(monitors) => self.emit(PlatformEvent::MonitorsChanged(monitors)),
            Err(e) => log::warn!("monitor enumeration failed: {e}"),
        }
    }

    /// A `WM_SETTINGCHANGE` named the `intl` area: the regional
    /// format or one of its formats changed. Read it now, and once more
    /// [`CLOCK_FORMAT_RECHECK`] from now (a pending second read is moved to
    /// then; replacing the timer kills the old one).
    pub(super) fn on_intl_change(&mut self) {
        self.refresh_clock_format("WM_SETTINGCHANGE intl");
        self.clock_format_timer = ThreadTimer::start(CLOCK_FORMAT_RECHECK);
    }

    /// Read the user's regional format and emit
    /// `ClockFormatChanged` when it is the first reading or differs from the
    /// last one emitted; `cause` names what asked, for the log.
    pub(super) fn refresh_clock_format(&mut self, cause: &str) {
        let format = locale::current();
        if self.clock_format.observe(&format) {
            log::debug!("clock format: {format:?} ({cause})");
            self.emit(PlatformEvent::ClockFormatChanged(format));
        } else {
            log::trace!("clock format unchanged ({cause})");
        }
    }

    /// Emit `ThemeChanged` when the theme differs from the last one emitted
    /// (or always, when `force`). Errors are skipped: the theme module is
    /// another task's and may still be a stub.
    pub(super) fn refresh_theme(&mut self, force: bool) {
        match theme::current_theme() {
            Ok(theme) => {
                if force || self.last_theme.as_ref() != Some(&theme) {
                    self.last_theme = Some(theme.clone());
                    self.emit(PlatformEvent::ThemeChanged(theme));
                }
            }
            Err(e) => log::debug!("theme unavailable: {e}"),
        }
    }
}
