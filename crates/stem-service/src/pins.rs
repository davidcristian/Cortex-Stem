//! Pinning: the commands that add, remove and reorder the pinned apps.

use stem_core::{AppId, EngineInput, LaunchTarget};

use crate::{DockService, ServiceError, ServiceResult, WorkerMsg};

impl DockService {
    /// Pin a running app (its identity must already be resolved).
    pub fn pin_app(&self, app_id: &AppId) -> ServiceResult<()> {
        let launch = self
            .inner
            .engine
            .lock()
            .identity(app_id)
            .and_then(|i| i.launch.clone())
            .ok_or_else(|| ServiceError::NotLaunchable(app_id.clone()))?;
        self.inner.mutate_settings(|settings| {
            settings.pin(stem_core::settings::PinnedApp {
                id: app_id.clone(),
                label: None,
                launch,
            })
        })?;
        Ok(())
    }

    /// Pin something the user dropped or chose: a path, a shortcut, a Store app.
    /// Returns the canonical id it was pinned under.
    pub fn pin_target(&self, target: LaunchTarget) -> ServiceResult<AppId> {
        let identity = self.inner.platform.identify_target(&target)?;
        let app_id = identity.id.clone();
        let launch = identity.launch.clone().unwrap_or(target);
        let inner = &self.inner;
        inner.mutate_settings(|settings| {
            if !settings.pin(stem_core::settings::PinnedApp {
                id: app_id.clone(),
                label: None,
                launch,
            }) {
                return false;
            }
            // Make the identity known before the new item renders: this is
            // queued ahead of the `SettingsChanged` that `mutate_settings`
            // sends on the same channel.
            let _ = inner
                .worker_tx
                .send(WorkerMsg::Input(EngineInput::IdentityResolved {
                    app_id: app_id.clone(),
                    identity,
                }));
            true
        })?;
        Ok(app_id)
    }

    pub fn unpin_app(&self, app_id: &AppId) -> ServiceResult<()> {
        let mut was_pinned = false;
        self.inner.mutate_settings(|settings| {
            was_pinned = settings.unpin(app_id);
            was_pinned
        })?;
        if was_pinned {
            Ok(())
        } else {
            Err(ServiceError::NotPinned(app_id.clone()))
        }
    }

    pub fn reorder_pinned(&self, order: &[AppId]) -> ServiceResult<()> {
        self.inner.mutate_settings(|settings| {
            let before = settings.pinned.clone();
            settings.reorder_pinned(order);
            settings.pinned != before
        })?;
        Ok(())
    }
}
