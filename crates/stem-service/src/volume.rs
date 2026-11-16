//! The volume item: a wheel notch steps the volume, and its menu mutes or
//! unmutes the output device.

use std::sync::atomic::Ordering;

use stem_core::{ScrollDirection, VolumeStatus};
use stem_platform::PlatformError;

use crate::{DockService, ServiceResult};

impl DockService {
    /// One wheel notch over the volume item (docs/ipc/widget-items.md):
    /// `Platform::step_volume`, which reads the volume now (not the
    /// engine's last reading, which a burst of notches outruns), applies
    /// `stem_core::engine::volume_step` and sets the level, then the mute
    /// (a notch up unmutes), as one operation on the platform's side, so
    /// notches that arrive together add up. Returns the volume it set, or
    /// `None` when there was nothing to do: no output device, the end of
    /// the range, or a platform that did not answer in time. That last one
    /// is a log line (one `warn` per streak) instead of an error, so a fast
    /// spin over a stuck volume thread does not send the page a failure per
    /// notch. Such a notch may or may not have been applied: the platform
    /// skips a step it takes only after the caller's deadline, but one it
    /// started in time finishes even when the answer comes too late; the
    /// next `VolumeChanged` says which. Other platform errors are returned.
    pub fn step_volume(&self, direction: ScrollDirection) -> ServiceResult<Option<VolumeStatus>> {
        match self.inner.platform.step_volume(direction) {
            Ok(stepped) => {
                self.inner.volume_stalled.store(false, Ordering::Relaxed);
                log::debug!("volume step {direction:?}: {stepped:?}");
                Ok(stepped)
            }
            Err(PlatformError::Timeout) => {
                if self.inner.volume_stalled.swap(true, Ordering::Relaxed) {
                    log::debug!(
                        "volume step {direction:?}: no answer in time again; it may or may not \
                         have been applied"
                    );
                } else {
                    log::warn!(
                        "volume step {direction:?}: the platform did not answer in time; the \
                         notch may or may not have been applied (later timeouts are logged at \
                         debug until a step goes through)"
                    );
                }
                Ok(None)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Mute or unmute the default output device (the volume item's
    /// *Mute* and *Unmute* menu entries). Fails without an output device.
    pub fn set_volume_muted(&self, muted: bool) -> ServiceResult<()> {
        Ok(self.inner.platform.set_volume_muted(muted)?)
    }

    /// The engine's last volume reading (`DockState::volume`): what
    /// the volume item's menu is built from (*Mute* or *Unmute*).
    pub fn volume(&self) -> Option<VolumeStatus> {
        self.inner.engine.lock().volume()
    }
}
