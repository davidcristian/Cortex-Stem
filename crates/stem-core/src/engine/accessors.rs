//! Read access to the facts the engine stores, and the apps it asks the
//! platform to identify.

use super::DockEngine;
use crate::model::{
    AppId, AppIdentity, ClockFormat, MonitorInfo, PowerStatus, RecycleBinState, ThemeInfo,
    VolumeStatus, WindowId,
};
use crate::settings::Settings;

impl DockEngine {
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn theme(&self) -> &ThemeInfo {
        &self.theme
    }

    pub fn monitors(&self) -> &[MonitorInfo] {
        &self.monitors
    }

    pub fn recycle_bin(&self) -> RecycleBinState {
        self.recycle_bin
    }

    /// The platform's last power reading: every field `false` and
    /// no percent until the first `PlatformEvent::PowerChanged`.
    pub fn power(&self) -> PowerStatus {
        self.power
    }

    /// The platform's last reading of the default output device's volume:
    /// `None` until the first `PlatformEvent::VolumeChanged`, and
    /// while the platform knows no default output device.
    pub fn volume(&self) -> Option<VolumeStatus> {
        self.volume
    }

    /// The platform's last reading of the user's regional format:
    /// no locale and no hour cycle until the first
    /// `PlatformEvent::ClockFormatChanged`.
    pub fn clock_format(&self) -> &ClockFormat {
        &self.clock_format
    }

    /// The OS foreground window, if the engine tracks it. It may be a window
    /// that no item shows (cloaked or invisible).
    pub fn foreground(&self) -> Option<WindowId> {
        self.foreground
    }

    pub fn identity(&self, id: &AppId) -> Option<&AppIdentity> {
        self.identities.get(id)
    }

    /// Apps (pinned or running) with no identity yet, regardless of whether
    /// they were requested before. Use for retries after a failed resolution.
    pub fn unresolved_apps(&self) -> Vec<AppId> {
        let mut ids: Vec<AppId> = self.settings.pinned.iter().map(|p| p.id.clone()).collect();
        ids.extend(self.running_order.iter().cloned());
        ids.sort();
        ids.dedup();
        ids.retain(|id| !self.identities.contains_key(id));
        ids
    }

    pub(super) fn take_new_unresolved(&mut self) -> Vec<AppId> {
        let fresh: Vec<AppId> = self
            .unresolved_apps()
            .into_iter()
            .filter(|id| !self.identity_requested.contains(id))
            .collect();
        self.identity_requested.extend(fresh.iter().cloned());
        fresh
    }
}
