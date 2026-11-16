//! What tests script: a failing `delete_to_recycle_bin`, Windows' own taskbar
//! auto-hide, the answer to `ask_yes_no`, and the power and regional format
//! readings the events thread reports.

use stem_core::{ClockFormat, PlatformEvent, PowerStatus};

use super::MockPlatform;

impl MockPlatform {
    /// Make every later `delete_to_recycle_bin` fail with `message` (what
    /// the shell refusing an item or the user cancelling looks like), or
    /// succeed again with `None`.
    pub fn set_recycle_failure(&self, message: Option<&str>) {
        self.state.lock().recycle_failure = message.map(str::to_owned);
    }

    /// Script Windows' own *Automatically hide the taskbar* (what the user
    /// had before Stem) without recording a call.
    pub fn script_taskbar_auto_hide(&self, enabled: bool) {
        self.state.lock().taskbar_auto_hide = enabled;
    }

    /// Script what the user answers to every later `ask_yes_no`;
    /// *No* (`false`) until this is called.
    pub fn script_yes_no_answer(&self, answer: bool) {
        self.state.lock().yes_no_answer = answer;
    }

    /// The power source, the battery or battery saver changed:
    /// report `status` the way the events thread does, with one
    /// `PowerChanged`, and only when it differs from the reading before (the
    /// real thread's change filter). Returns whether it was reported.
    pub fn set_power(&self, status: PowerStatus) -> bool {
        if self.state.lock().power == status {
            return false;
        }
        self.push(PlatformEvent::PowerChanged(status));
        true
    }

    /// The user changed the regional format or the short time format:
    /// report `format` the way the events thread does after a
    /// `WM_SETTINGCHANGE` for `intl`, with one `ClockFormatChanged`, and
    /// only when it differs from the reading before (the real thread's
    /// change filter). Returns whether it was reported.
    pub fn set_clock_format(&self, format: ClockFormat) -> bool {
        if self.state.lock().clock_format == format {
            return false;
        }
        self.push(PlatformEvent::ClockFormatChanged(format));
        true
    }
}
