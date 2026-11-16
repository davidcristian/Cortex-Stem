//! Hidden apps (`docs/ipc/hidden-apps.md`): hiding an app from every dock,
//! showing it again, and what a hidden app keeps.

use stem_core::{AppId, DockEngine, IconKey, LaunchTarget};
use stem_platform::rules::identity::{exe_icon_key, file_icon_key, is_window_icon_key};

use crate::resolver::fallback_display_name;
use crate::{DockService, ServiceError, ServiceResult};

impl DockService {
    /// Stop showing an app on every dock (`docs/ipc/hidden-apps.md`):
    /// `{ id, name, path, iconKey }` joins `Settings::hidden_apps`,
    /// saved like a pin, and the engine drops its item when the new
    /// settings reach it. `name` is what its item shows now
    /// (`DockEngine::app_name`: the resolved display name, else its first
    /// window's title; the id's file name when that is blank); `path` and
    /// `icon_key` are what [`hidden_app_facts`] finds. Hiding a
    /// hidden app again changes nothing. Rejects a pinned app (`Pinned`)
    /// and an app the engine knows nothing of, with neither an identity nor
    /// a window (`UnknownApp`).
    pub fn hide_app(&self, app_id: &AppId) -> ServiceResult<()> {
        let mut outcome = Ok(());
        let inner = &self.inner;
        inner.mutate_settings(|settings| {
            if settings.is_pinned(app_id) {
                outcome = Err(ServiceError::Pinned(app_id.clone()));
                return false;
            }
            if settings.is_hidden(app_id) {
                return false;
            }
            // Settings, then engine: the lock order `Inner::settings` allows.
            let (name, path, icon_key) = {
                let engine = inner.engine.lock();
                let Some(name) = engine.app_name(app_id) else {
                    outcome = Err(ServiceError::UnknownApp(app_id.clone()));
                    return false;
                };
                let (path, icon_key) = hidden_app_facts(&engine, app_id);
                (name, path, icon_key)
            };
            let name = if name.trim().is_empty() {
                fallback_display_name(app_id)
            } else {
                name
            };
            log::info!("hiding {app_id:?} ({name:?}, {path:?}) from the dock");
            settings.hide(stem_core::settings::HiddenApp {
                id: app_id.clone(),
                name,
                path,
                icon_key,
            })
        })?;
        outcome
    }

    /// Show a hidden app again: its entry leaves
    /// `Settings::hidden_apps` and the settings are saved. An id that is not
    /// listed is not an error and saves nothing.
    pub fn unhide_app(&self, app_id: &AppId) -> ServiceResult<()> {
        self.inner.mutate_settings(|settings| {
            let shown = settings.unhide(app_id);
            if !shown {
                log::debug!("unhide_app: {app_id:?} is not hidden; nothing to do");
            }
            shown
        })?;
        Ok(())
    }
}

/// What a hidden app keeps besides its name (`HiddenApp::path` and
/// `HiddenApp::icon_key`): the program's path as Windows reported it, from
/// the window that stands for the app (`DockEngine::app_window`), else the
/// file its identity launches (an `exe` or a shortcut); and the icon its
/// item showed, its identity's else that window's, as the engine picks it
/// for the item, except a key bound to one window (`hwnd:`,
/// `rules::identity::is_window_icon_key`), which is never kept: it is saved
/// for good, and once the window is gone its handle may be another
/// process's. That file's own icon (`exe:` / `file:`) stands in when the
/// path is known; else there is none.
///
/// An `aumid:` app's window is never asked for the path: a Store app shown
/// through an `ApplicationFrameHost` frame (Calculator, Settings) has the
/// frame host's process, so its path would be
/// `C:\Windows\System32\ApplicationFrameHost.exe` and its stand-in icon the
/// frame host's. Such an app keeps only what its identity launches (none
/// for a Store app's `storeApp` target). Pure.
pub(crate) fn hidden_app_facts(
    engine: &DockEngine,
    app_id: &AppId,
) -> (Option<String>, Option<IconKey>) {
    let window = engine.app_window(app_id);
    let identity = engine.identity(app_id);
    let window_path = if app_id.as_str().starts_with("aumid:") {
        None
    } else {
        window.and_then(|w| w.exe_path.as_deref())
    };
    let (path, file_icon) = match window_path {
        Some(exe) => {
            let exe = exe.to_string_lossy().into_owned();
            let icon = exe_icon_key(&exe);
            (Some(exe), Some(icon))
        }
        None => match identity.and_then(|i| i.launch.as_ref()) {
            Some(LaunchTarget::Exe { path, .. }) => {
                let path = path.to_string_lossy().into_owned();
                let icon = exe_icon_key(&path);
                (Some(path), Some(icon))
            }
            Some(LaunchTarget::Shortcut { path }) => {
                let path = path.to_string_lossy().into_owned();
                let icon = file_icon_key(&path);
                (Some(path), Some(icon))
            }
            _ => (None, None),
        },
    };
    let icon_key = [
        identity.and_then(|i| i.icon_key.clone()),
        window.and_then(|w| w.icon_key.clone()),
    ]
    .into_iter()
    .flatten()
    .find(|key| !is_window_icon_key(key))
    .or(file_icon);
    (path, icon_key)
}
