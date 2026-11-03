//! The Tauri shell. Thin by design: every decision lives in `stem-service`
//! and below. This crate only
//! - creates and positions the dock window(s),
//! - exposes `stem-service` through Tauri commands (`commands`),
//! - forwards `ServiceEvent`s as Tauri events (`docs/ipc/events.md`),
//! - wires plugins (logging now; tray, single-instance, autostart in M1/M2).

mod commands;
mod windows;

use std::sync::Arc;

use stem_platform::create_platform;
use stem_service::{
    DockService, FileSettingsStore, MemorySettingsStore, ServiceEvent, SettingsStore,
};
use tauri::{AppHandle, Emitter, Manager};

/// Shared handle to the service, stored in Tauri's managed state.
#[derive(Debug, Clone)]
pub struct AppService(pub Arc<DockService>);

/// Event names, as documented in `docs/ipc/events.md`.
pub mod events {
    pub const STATE: &str = "dock://state";
    pub const SETTINGS: &str = "dock://settings";
    pub const THEME: &str = "dock://theme";
    pub const MONITORS: &str = "dock://monitors";
    pub const BOUNCE: &str = "dock://item-bounce";
}

pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .invoke_handler(commands::handler())
        .setup(|app| {
            let platform = create_platform();
            log::info!("platform: {}", platform.name());

            let store = settings_store(app.handle());
            let service = Arc::new(DockService::start(platform, store)?);
            forward_events(app.handle().clone(), &service);
            app.manage(AppService(Arc::clone(&service)));

            windows::create_dock_windows(app.handle(), &service)?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build the Tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event
                && let Some(service) = app.try_state::<AppService>()
            {
                service.0.shutdown();
            }
        });
}

/// Settings live in `<app config dir>/settings.json`; the mock platform uses
/// memory so demo runs never touch a real profile.
fn settings_store(app: &AppHandle) -> Box<dyn SettingsStore> {
    if std::env::var("STEM_PLATFORM").is_ok_and(|v| v.eq_ignore_ascii_case("mock")) {
        return Box::new(MemorySettingsStore::default());
    }
    match app.path().app_config_dir() {
        Ok(dir) => Box::new(FileSettingsStore::new(dir.join("settings.json"))),
        Err(e) => {
            log::error!("no app config dir ({e}); settings will not persist");
            Box::new(MemorySettingsStore::default())
        }
    }
}

/// Turn service events into Tauri events for every window.
fn forward_events(app: AppHandle, service: &DockService) {
    service.subscribe(Box::new(move |event| {
        let result = match event {
            ServiceEvent::State(state) => app.emit(events::STATE, state),
            ServiceEvent::Settings(settings) => app.emit(events::SETTINGS, settings),
            ServiceEvent::Theme(theme) => app.emit(events::THEME, theme),
            ServiceEvent::Monitors(monitors) => app.emit(events::MONITORS, monitors),
            ServiceEvent::Bounce { item_key } => {
                app.emit(events::BOUNCE, BouncePayload { item_key })
            }
        };
        if let Err(e) = result {
            log::warn!("failed to emit event: {e}");
        }
    }));
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct BouncePayload<'a> {
    item_key: &'a str,
}
