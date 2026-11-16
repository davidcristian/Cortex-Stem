use stem_service::{DockService, ServiceEvent};
use tauri::{AppHandle, Emitter};

use crate::{appbar, autostart, backdrop, events, shortcut, taskbar, tray, windows};

/// Turn service events into Tauri events for every window. Runs on the
/// service worker thread; `emit` is thread-safe and the dock re-layout
/// hops to the main thread itself.
pub(crate) fn forward_events(app: AppHandle, service: &DockService) {
    service.subscribe(Box::new(move |event| {
        let result = match event {
            ServiceEvent::State(state) => app.emit(events::STATE, state),
            ServiceEvent::Settings(settings) => {
                tray::on_settings_changed(&app, settings);
                autostart::on_settings_changed(&app, settings);
                taskbar::on_settings_changed(&app, settings);
                shortcut::on_settings_changed(&app, settings);
                // The pages before their windows: the emit is queued on the
                // main thread ahead of the window work `windows` posts, so
                // on a new edge a page usually lays itself out for
                // it before its window moves, and its next content-size
                // report is read for the edge it was laid out for.
                let emitted = app.emit(events::SETTINGS, settings);
                windows::on_settings_changed(&app, settings);
                // After `windows`: a position change moves the windows
                // first, and the bands follow them.
                appbar::on_settings_changed(&app, settings);
                // The backdrop prototype follows the setting and the theme.
                backdrop::on_settings_changed(&app, settings);
                emitted
            }
            ServiceEvent::Theme(theme) => {
                backdrop::on_theme_changed(&app, theme);
                app.emit(events::THEME, theme)
            }
            ServiceEvent::Monitors(monitors) => {
                windows::on_monitors_changed(&app, monitors.clone());
                app.emit(events::MONITORS, monitors)
            }
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
