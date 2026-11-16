//! What a dock item does: the primary click, window and shell actions,
//! launches, and files dropped on the Recycle Bin.

use std::sync::Arc;

use stem_core::{
    AppId, DockItem, EngineInput, ItemAction, LaunchTarget, PlatformEvent, ShellAction,
    WindowAction, WindowId,
};

use crate::{DockService, Inner, ServiceError, ServiceEvent, ServiceResult, WorkerMsg};

impl DockService {
    /// The dock's "click" on `item_key` in the dock window `dock` (its
    /// label): launch, activate, minimise, or run a shell action. Only the
    /// windows that dock lists take part (`DockEngine::primary_action`):
    /// with a dock per monitor and `showOnlyCurrentMonitor` on, the
    /// ones on its own monitor.
    pub fn primary_click(&self, item_key: &str, dock: &str) -> ServiceResult<()> {
        self.primary_click_over(item_key, dock, || false)
    }

    /// [`Self::primary_click`] for a click while one of Stem's own windows
    /// other than a dock may be in front (the settings window):
    /// `own_window_in_front` says whether one is, and is asked only when the
    /// click would minimise ([`over_own_window`]). The app layer can tell;
    /// the engine cannot, since it never takes Stem's own windows for the
    /// foreground.
    pub fn primary_click_over(
        &self,
        item_key: &str,
        dock: &str,
        own_window_in_front: impl FnOnce() -> bool,
    ) -> ServiceResult<()> {
        let action = self.inner.engine.lock().primary_action(item_key, dock);
        let action = over_own_window(action, own_window_in_front);
        log::debug!("primary click on {item_key} in {dock:?}: {action:?}");
        match action {
            ItemAction::Launch(app_id) => self.launch_app(&app_id),
            ItemAction::ActivateWindow(id) => Ok(self
                .inner
                .platform
                .window_action(id, WindowAction::Activate)?),
            ItemAction::MinimizeWindow(id) => Ok(self
                .inner
                .platform
                .window_action(id, WindowAction::Minimize)?),
            ItemAction::Shell(action) => Ok(self.inner.platform.shell_action(action)?),
            ItemAction::None => Err(ServiceError::UnknownItem(item_key.to_owned())),
        }
    }

    /// The item `item_key` as the dock window `dock` lists it, which its
    /// context menu is built from (`DockEngine::dock_item`): an app
    /// item with only the windows that dock lists. `None` for an unknown
    /// key.
    pub fn dock_item(&self, item_key: &str, dock: &str) -> Option<DockItem> {
        self.inner.engine.lock().dock_item(item_key, dock)
    }

    /// The windows of `app_id` the dock window `dock` lists, now
    /// (`DockEngine::windows_on_dock`): what its menu's *Close all
    /// windows* closes.
    pub fn windows_on_dock(&self, app_id: &AppId, dock: &str) -> Vec<WindowId> {
        self.inner.engine.lock().windows_on_dock(app_id, dock)
    }

    pub fn window_action(&self, id: WindowId, action: WindowAction) -> ServiceResult<()> {
        Ok(self.inner.platform.window_action(id, action)?)
    }

    pub fn shell_action(&self, action: ShellAction) -> ServiceResult<()> {
        Ok(self.inner.platform.shell_action(action)?)
    }

    /// Move dropped files and folders to the Recycle Bin: a file
    /// drop on its item. An empty list does nothing.
    ///
    /// Blocks while the platform does, which can include the user answering
    /// the shell's dialogs (`Platform::delete_to_recycle_bin`): call it off
    /// the UI thread. Afterwards, whatever the outcome (part of a refused
    /// batch may already be in the bin), the bin is queried once on a
    /// short-lived thread and the answer handed to the engine, so the item's
    /// icon turns "full" without waiting for the platform's next poll and
    /// without delaying this call's result by a slow query.
    pub fn delete_to_recycle_bin(&self, paths: &[String]) -> ServiceResult<()> {
        if paths.is_empty() {
            log::debug!("delete_to_recycle_bin with no paths: nothing to do");
            return Ok(());
        }
        let result = self.inner.platform.delete_to_recycle_bin(paths);
        self.inner.refresh_recycle_bin();
        Ok(result?)
    }

    /// Start (another instance of) an app, pinned or merely running.
    pub fn launch_app(&self, app_id: &AppId) -> ServiceResult<()> {
        let target = self
            .inner
            .launch_target_for(app_id)
            .ok_or_else(|| ServiceError::NotLaunchable(app_id.clone()))?;
        self.inner.platform.launch(&target)?;
        self.inner.notify(&ServiceEvent::Bounce {
            item_key: format!("app:{}", app_id.as_str()),
        });
        Ok(())
    }
}

impl Inner {
    /// Ask the platform for the Recycle Bin's state on a short-lived thread
    /// (`SHQueryRecycleBinW` walks every drive and can take seconds) and
    /// queue the answer for the worker like a platform event. A duplicate of
    /// what the platform's poll reports later changes nothing: the engine
    /// keeps the latest state and bumps the revision only when the icon flips.
    fn refresh_recycle_bin(&self) {
        let platform = Arc::clone(&self.platform);
        let worker_tx = self.worker_tx.clone();
        let spawned = std::thread::Builder::new()
            .name("stem-recycle-bin-refresh".into())
            .spawn(move || match platform.recycle_bin() {
                Ok(state) => {
                    // The worker may be gone (shutdown): nothing to update then.
                    let _ = worker_tx.send(WorkerMsg::Input(EngineInput::Platform(
                        PlatformEvent::RecycleBinChanged(state),
                    )));
                }
                Err(e) => log::debug!("recycle bin refresh failed: {e}"),
            });
        if let Err(e) = spawned {
            log::debug!(
                "no recycle bin refresh (thread not started: {e}); the poll will report it"
            );
        }
    }

    /// The launch target of a pinned app (from the authoritative settings),
    /// else what its identity says.
    fn pinned_launch_target(&self, app_id: &AppId) -> Option<LaunchTarget> {
        self.settings
            .lock()
            .pinned
            .iter()
            .find(|p| &p.id == app_id)
            .map(|p| p.launch.clone())
    }

    fn launch_target_for(&self, app_id: &AppId) -> Option<LaunchTarget> {
        self.pinned_launch_target(app_id).or_else(|| {
            self.engine
                .lock()
                .identity(app_id)
                .and_then(|i| i.launch.clone())
        })
    }
}

/// What a dock click does while one of Stem's own windows other than a dock
/// may be in front. The engine never takes Stem's own windows for
/// the foreground, so while the settings window is in front its active
/// window is the app behind it: a click that would minimise that app as
/// the active one (`behavior.clickActiveMinimizes`) brings it forward
/// instead, as the stock taskbar does for a window that is not in front.
/// `own_window_in_front` is asked only for such a click. A dock in front
/// (the focus shortcut's session) is not one of those windows: Enter on the
/// active app's item still minimises it. Every other action is unchanged.
pub fn over_own_window(
    action: ItemAction,
    own_window_in_front: impl FnOnce() -> bool,
) -> ItemAction {
    match action {
        ItemAction::MinimizeWindow(id) if own_window_in_front() => {
            log::debug!(
                "a Stem window other than a dock is in front: {id:?}, the active window behind it, comes forward instead of minimising"
            );
            ItemAction::ActivateWindow(id)
        }
        other => other,
    }
}
