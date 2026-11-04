//! Autostart: the OS "start at sign-in" entry follows
//! `system.autostart`.
//!
//! `tauri-plugin-autostart` owns the entry. On Windows its `auto-launch`
//! backend writes the value `Stem` (the product name), holding the path of
//! the running executable, under
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (under `HKLM` instead
//! when the process may write there, i.e. when it runs elevated), and marks
//! it enabled under `…\Explorer\StartupApproved\Run` when that key exists;
//! turning it off deletes the value. `is_enabled` is `false` while Task
//! Manager's *Startup apps* page has the entry disabled.
//!
//! The app syncs at start and whenever a settings change flips the value: it
//! reads `is_enabled()` and writes only on a difference. That is registry
//! I/O, so it runs neither on the main thread nor on the service worker:
//! each sync is a task on Tauri's blocking pool, the tasks run one at a time,
//! and each one applies the latest wanted value, so the last change wins in
//! whatever order the tasks start. A failure is logged at `warn` and not
//! retried until the value changes again or the app restarts.
//!
//! The mock platform (demo runs) never touches the real entry, just as it
//! never touches the real settings file.

use parking_lot::Mutex;
use stem_core::Settings;
use stem_platform::is_mock;
use stem_service::DockService;
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::AutoLaunchManager;

use crate::on_off;

/// Managed state; it exists only while the plugin is registered.
#[derive(Debug, Default)]
struct AutostartSync {
    /// What the latest settings ask for; `None` before the first request.
    wanted: Mutex<Option<bool>>,
    /// Held while a sync runs, so that syncs never overlap.
    running: Mutex<()>,
}

/// Register the plugin and bring the OS entry in line with the settings.
/// Runs in `setup`, after `forward_events`: the settings are read once the
/// state is managed, so a change in between is not lost. If the plugin
/// cannot start, the setting has no effect this session and the reason is
/// logged once.
pub fn init(app: &AppHandle, service: &DockService) {
    if is_mock(service.platform().as_ref()) {
        log::info!("autostart: the mock platform leaves the OS entry alone");
        return;
    }
    if let Err(e) = app.plugin(tauri_plugin_autostart::Builder::new().build()) {
        log::warn!("autostart unavailable: {e}");
        return;
    }
    app.manage(AutostartSync::default());
    on_settings_changed(app, &service.settings());
}

/// The settings changed (or the app started): schedule a sync when
/// `system.autostart` differs from the previous request. Any thread; never
/// blocks.
pub fn on_settings_changed(app: &AppHandle, settings: &Settings) {
    let Some(sync) = app.try_state::<AutostartSync>() else {
        return;
    };
    let wanted = settings.system.autostart;
    {
        let mut requested = sync.wanted.lock();
        if *requested == Some(wanted) {
            return;
        }
        *requested = Some(wanted);
    }
    let app = app.clone();
    drop(tauri::async_runtime::spawn_blocking(move || {
        let Some(sync) = app.try_state::<AutostartSync>() else {
            return;
        };
        let _one_at_a_time = sync.running.lock();
        // The latest request, not necessarily the one that spawned this task.
        let Some(wanted) = *sync.wanted.lock() else {
            return;
        };
        if let Some(manager) = app.try_state::<AutoLaunchManager>() {
            apply(&manager, wanted);
        }
    }));
}

/// Make the OS entry say `wanted`. Blocking pool.
fn apply(manager: &AutoLaunchManager, wanted: bool) {
    match manager.is_enabled() {
        Ok(enabled) if enabled == wanted => {
            log::debug!("autostart is already {}", on_off(wanted));
            return;
        }
        Ok(_) => {}
        // Both writes are idempotent, so a state that cannot be read is no
        // reason to leave it as it may be.
        Err(e) => log::warn!(
            "autostart: could not read the OS entry ({e}); turning it {} anyway",
            on_off(wanted)
        ),
    }
    let result = if wanted {
        manager.enable()
    } else {
        manager.disable()
    };
    match result {
        Ok(()) => log::info!("autostart {}", on_off(wanted)),
        Err(e) => log::warn!("autostart: could not turn it {}: {e}", on_off(wanted)),
    }
}
