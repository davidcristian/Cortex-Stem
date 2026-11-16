//! What the app layer reads from the service, and the dock panel facts it
//! reports.

use std::sync::Arc;

use stem_core::{
    DockState, EngineInput, IconKey, MonitorId, MonitorInfo, Rect, Settings, ThemeInfo,
};
use stem_platform::Platform;

use crate::{DockService, ServiceResult, WorkerMsg};

impl DockService {
    pub fn platform(&self) -> &Arc<dyn Platform> {
        &self.inner.platform
    }

    pub fn state(&self) -> DockState {
        self.inner.engine.lock().snapshot()
    }

    /// The authoritative settings: what the last successful mutation
    /// produced, which the engine may not have applied yet.
    pub fn settings(&self) -> Settings {
        self.inner.settings.lock().clone()
    }

    pub fn theme(&self) -> ThemeInfo {
        self.inner.engine.lock().theme().clone()
    }

    pub fn monitors(&self) -> Vec<MonitorInfo> {
        self.inner.engine.lock().monitors().to_vec()
    }

    /// Where the panel of the dock window `label` rests on screen, in
    /// physical pixels of the virtual desktop (the app layer
    /// converts the UI's CSS rect), with the monitor the app layer placed
    /// the window on; `rect` is `None` while the window is hidden, and
    /// `monitor` and `rect` both `None` once the window is gone (the engine
    /// forgets the label). Queued for the worker like a platform event, so
    /// it is applied in order with the window updates it is compared
    /// against; the engine recomputes that dock's `overlapped` and a `State`
    /// follows only when its facts change. Never blocks.
    pub fn set_dock_panel(&self, label: &str, monitor: Option<MonitorId>, rect: Option<Rect>) {
        log::debug!("dock panel of {label:?} on {monitor:?}: {rect:?}");
        let _ = self
            .inner
            .worker_tx
            .send(WorkerMsg::Input(EngineInput::DockPanelChanged {
                label: label.to_owned(),
                monitor,
                rect,
            }));
    }

    /// `key`'s icon as PNG bytes whose longer edge is at most `size`, from
    /// the disk icon cache when the service has one and holds a valid entry
    /// (see [`icon_cache`] for which keys are cached and when an
    /// entry is valid), else from the platform. Blocks the calling thread
    /// for as long as the platform's extraction takes on a miss; never on
    /// a cache write.
    ///
    /// [`icon_cache`]: crate::icon_cache
    pub fn icon_png(&self, key: &IconKey, size: u32) -> ServiceResult<Option<Vec<u8>>> {
        let platform = &self.inner.platform;
        let png = match &self.inner.icon_cache {
            Some(cache) => cache.get_or_load(key, size, || platform.icon_png(key, size))?,
            None => platform.icon_png(key, size)?,
        };
        Ok(png)
    }
}
