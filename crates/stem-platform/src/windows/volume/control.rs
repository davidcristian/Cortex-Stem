//! The callers' requests on the thread: a live reading, the level, the mute
//! and one wheel notch.

use stem_core::engine::volume_step;
use stem_core::{ScrollDirection, VolumeStatus};

use super::endpoint::{BindError, Endpoint, read_endpoint};
use super::thread::Follower;
use crate::rules::volume as rules;
use crate::{PlatformError, PlatformResult};

impl Follower<'_> {
    /// A live reading. A failed one (logged once per streak) drops the
    /// binding and binds once more; when the fresh binding cannot be read
    /// either, it is dropped too and tried again later like a failed bind,
    /// so a broken device costs one try per retry delay, not one per
    /// notification.
    pub(super) fn read(&mut self) -> Option<VolumeStatus> {
        let endpoint = self.endpoint.as_ref()?;
        let error = match read_endpoint(endpoint) {
            Ok(status) => return Some(self.read_ok(status)),
            Err(e) => e,
        };
        if self.read_failing {
            log::debug!("volume: reading the default output device failed again: {error}");
        } else {
            log::warn!("volume: reading the default output device failed: {error}; binding again");
        }
        self.read_failing = true;
        self.bind_default("read failed");
        let endpoint = self.endpoint.as_ref()?;
        match read_endpoint(endpoint) {
            Ok(status) => Some(self.read_ok(status)),
            Err(e) => {
                self.endpoint = None;
                self.failed(&BindError::other(e), "read failed");
                None
            }
        }
    }

    /// A good reading ends every failure streak.
    fn read_ok(&mut self, status: VolumeStatus) -> VolumeStatus {
        self.read_failing = false;
        self.failures = 0;
        status
    }

    fn bound(&self) -> PlatformResult<&Endpoint> {
        self.endpoint
            .as_ref()
            .ok_or_else(|| PlatformError::Other("no audio output device".into()))
    }

    pub(super) fn set_level(&self, percent: u8) -> PlatformResult<()> {
        let endpoint = self.bound()?;
        // SAFETY: a plain COM call on a live control; a null event context
        // is documented as allowed.
        unsafe {
            endpoint
                .volume
                .SetMasterVolumeLevelScalar(rules::scalar_of(percent), std::ptr::null())
        }?;
        Ok(())
    }

    /// One wheel notch: the reading now, then what `volume_step` makes of
    /// it, the level first and then the mute, each only when it changes.
    /// `None` without a device or when nothing changes.
    pub(super) fn step(
        &mut self,
        direction: ScrollDirection,
    ) -> PlatformResult<Option<VolumeStatus>> {
        let Some(current) = self.read() else {
            return Ok(None);
        };
        let next = volume_step(current, direction);
        if next == current {
            return Ok(None);
        }
        if next.percent != current.percent {
            self.set_level(next.percent)?;
        }
        if next.muted != current.muted {
            self.set_muted(next.muted)?;
        }
        log::debug!("volume: step {direction:?}: {current:?} -> {next:?}");
        Ok(Some(next))
    }

    pub(super) fn set_muted(&self, muted: bool) -> PlatformResult<()> {
        let endpoint = self.bound()?;
        // SAFETY: a plain COM call on a live control; a null event context
        // is documented as allowed.
        unsafe { endpoint.volume.SetMute(muted, std::ptr::null()) }?;
        Ok(())
    }
}
