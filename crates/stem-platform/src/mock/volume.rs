//! The scripted output device: its volume changed outside Stem, set, or
//! stepped by one wheel notch.

use stem_core::{PlatformEvent, ScrollDirection, VolumeStatus};

use super::{MockCall, MockPlatform};
use crate::{PlatformError, PlatformResult};

impl MockPlatform {
    /// The volume, the mute or the default output device changed outside
    /// Stem (the volume keys, the Quick Settings slider, headphones
    /// plugged in; `None`: no output device any more): report `status` the
    /// way the volume thread does, with one `VolumeChanged`, and only when
    /// it differs from the reading before (the real thread's change filter).
    /// Returns whether it was reported; nothing is recorded.
    pub fn change_volume(&self, status: Option<VolumeStatus>) -> bool {
        if self.state.lock().volume == status {
            return false;
        }
        self.push(PlatformEvent::VolumeChanged(status));
        true
    }

    /// Apply `change` to the scripted device's volume, as the volume thread
    /// sets it: an error without a device, else the new reading, reported
    /// with one `VolumeChanged` when it differs.
    pub(super) fn change_output(
        &self,
        change: impl FnOnce(&mut VolumeStatus),
    ) -> PlatformResult<()> {
        let changed = {
            let s = self.state.lock();
            let Some(current) = s.volume else {
                return Err(PlatformError::Other("no audio output device".into()));
            };
            let mut next = current;
            change(&mut next);
            (next != current).then_some(next)
        };
        if let Some(next) = changed {
            self.push(PlatformEvent::VolumeChanged(Some(next)));
        }
        Ok(())
    }

    /// [`Platform::step_volume`](crate::Platform::step_volume) on the
    /// scripted device.
    pub(super) fn step_output(
        &self,
        direction: ScrollDirection,
    ) -> PlatformResult<Option<VolumeStatus>> {
        self.record(MockCall::StepVolume(direction));
        let (next, sink) = {
            let mut s = self.state.lock();
            let next = s.volume.and_then(|current| {
                let next = stem_core::engine::volume_step(current, direction);
                (next != current).then_some(next)
            });
            if next.is_some() {
                s.volume = next;
            }
            (next, s.sink.clone())
        };
        if let (Some(next), Some(sink)) = (next, sink) {
            sink(PlatformEvent::VolumeChanged(Some(next)));
        }
        Ok(next)
    }
}
