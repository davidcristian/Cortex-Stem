//! Virtual desktop switches.
//!
//! Contract:
//! - Virtual desktops: every query but the move path's fills
//!   `RawWindow::on_current_desktop` from the shell's
//!   `IVirtualDesktopManager` (`windows::virtual_desktop`), whose
//!   per-thread instance this thread enables in `setup`, inside its COM
//!   apartment, and releases before it; the move path keeps the answer last
//!   reported (a move does not change the desktop). The call has no
//!   timeout: a slow one pauses the asking for 30 s (`None` meanwhile), a
//!   hung one blocks this thread until Explorer answers (see
//!   `windows::virtual_desktop`). A desktop switch has no event of
//!   its own: its signature is the shell cloaking and uncloaking top-level
//!   windows in a burst, and each of those events already re-queries its
//!   window. When two of them arrive within 100 ms
//!   (`rules::events::DesktopSwitchDetector`) the thread arms one
//!   [`DESKTOP_SWITCH_SETTLE`] timer (400 ms) and, when it fires, emits
//!   one `WindowsSnapshot` (a re-enumeration, with a fresh manager), so a
//!   window whose event was missed or whose answer was taken mid-switch is
//!   put right; any snapshot resets the detector. Debug line: `desktop
//!   switch settled; re-enumerating`.

use std::time::Duration;

use super::state::ThreadState;
use super::timer::ThreadTimer;
use crate::rules::events::DesktopSwitchDetector;

/// How long after a burst of cloak events (a virtual-desktop switch)
/// the windows are enumerated again: once the shell has finished cloaking
/// and uncloaking, one `WindowsSnapshot` puts every window right.
pub const DESKTOP_SWITCH_SETTLE: Duration =
    Duration::from_millis(DesktopSwitchDetector::SNAPSHOT_DELAY_MS);

impl ThreadState {
    /// An `EVENT_OBJECT_CLOAKED` or `EVENT_OBJECT_UNCLOAKED` about a
    /// top-level window: the second within 100 ms is a virtual
    /// desktop switch in progress, so one snapshot is taken
    /// [`DESKTOP_SWITCH_SETTLE`] from now; when no timer can be had it is
    /// taken at once rather than never.
    pub(super) fn on_cloak_event(&mut self) {
        let now = self.now_ms();
        if !self.switch.observe(now) {
            return;
        }
        log::trace!("cloak burst: a desktop switch; snapshot in {DESKTOP_SWITCH_SETTLE:?}");
        match ThreadTimer::start(DESKTOP_SWITCH_SETTLE) {
            Some(timer) => self.switch_timer = Some(timer),
            None => self.on_desktop_switch_settled(),
        }
    }

    /// The switch timer fired: enumerate everything again, so the windows
    /// of both desktops are reported as the shell sees them now.
    pub(super) fn on_desktop_switch_settled(&mut self) {
        // Thread timers repeat; this one is one-shot.
        self.switch_timer = None;
        log::debug!("desktop switch settled; re-enumerating");
        self.emit_snapshot();
    }
}
