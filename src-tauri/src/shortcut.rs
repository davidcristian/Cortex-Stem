//! Global shortcut that toggles the dock: `behavior.toggleShortcut`
//! (default `Ctrl+Alt+D`) does from any app what a left click on the tray
//! icon does ([`crate::windows::toggle_dock`]).
//!
//! `tauri-plugin-global-shortcut` registers it with `RegisterHotKey` on a
//! hidden window that belongs to the main thread; that window's procedure
//! reports the press on the main thread. The plugin also reports the release
//! (from a short-lived polling thread), which is ignored here.
//!
//! A change unregisters the old shortcut first, then registers the new one.
//! A value that does not parse, that would take over plain typing (see
//! [`parse`]) or that Windows refuses (another app holds it) logs a warning
//! and leaves no shortcut registered; the settings value stays as the user
//! wrote it, so they can correct it. An empty value registers nothing.
//!
//! Threads: the plugin runs every (un)registration on the main thread and
//! makes the caller wait for it. Settings events arrive on the service
//! worker, where that wait could deadlock against a main thread that is
//! itself waiting on the service, so a change hops to the main thread
//! without waiting; there the plugin's own hop runs inline.

use std::str::FromStr;

use parking_lot::Mutex;
use stem_core::Settings;
use stem_service::DockService;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_global_shortcut::{
    Code, GlobalShortcut, Modifiers, Shortcut, ShortcutEvent, ShortcutState,
};

/// Managed state; it exists only while the plugin is registered.
#[derive(Debug, Default)]
struct ToggleShortcut {
    /// The trimmed accelerator the latest settings ask for; `None` before
    /// the first request.
    wanted: Mutex<Option<String>>,
    /// What is registered now. Only the main thread touches it.
    registered: Mutex<Option<Shortcut>>,
}

/// Register the plugin and the shortcut from the settings. Runs in `setup`
/// (main thread), after `forward_events` and once the dock window exists;
/// the settings are read once the state is managed, so a change in between
/// is not lost. If the plugin cannot start, the setting has no effect this
/// session and the reason is logged once.
pub fn init(app: &AppHandle, service: &DockService) {
    if let Err(e) = app.plugin(tauri_plugin_global_shortcut::Builder::new().build()) {
        log::warn!("global shortcuts unavailable: {e}");
        return;
    }
    app.manage(ToggleShortcut::default());
    on_settings_changed(app, &service.settings());
}

/// The settings changed (or the app started): re-register when
/// `behavior.toggleShortcut` differs from the previous request. Any thread;
/// never blocks.
pub fn on_settings_changed(app: &AppHandle, settings: &Settings) {
    let Some(state) = app.try_state::<ToggleShortcut>() else {
        return;
    };
    let wanted = settings.behavior.toggle_shortcut.trim().to_owned();
    {
        let mut requested = state.wanted.lock();
        if requested.as_deref() == Some(wanted.as_str()) {
            return;
        }
        *requested = Some(wanted.clone());
    }
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || apply(&handle, &wanted)) {
        log::warn!("could not schedule the toggle-shortcut change: {e}");
    }
}

/// Swap the registered shortcut for `wanted`. Main thread.
fn apply(app: &AppHandle, wanted: &str) {
    let (Some(plugin), Some(state)) = (
        app.try_state::<GlobalShortcut<Wry>>(),
        app.try_state::<ToggleShortcut>(),
    ) else {
        return;
    };
    let mut registered = state.registered.lock();
    if let Some(old) = registered.take() {
        match plugin.unregister(old) {
            Ok(()) => log::debug!("toggle shortcut {old} unregistered"),
            Err(e) => log::warn!("toggle shortcut {old}: unregistering failed: {e}"),
        }
    }
    let shortcut = match parse(wanted) {
        Ok(Some(shortcut)) => shortcut,
        Ok(None) => {
            log::info!("toggle shortcut: none");
            return;
        }
        Err(reason) => {
            log::warn!("toggle shortcut {wanted:?} ignored: {reason}; no shortcut is active");
            return;
        }
    };
    match plugin.on_shortcut(shortcut, on_shortcut) {
        Ok(()) => {
            log::info!("toggle shortcut: {wanted}");
            *registered = Some(shortcut);
        }
        Err(e) => log::warn!(
            "toggle shortcut {wanted:?} could not be registered ({e}); another app may hold it; no shortcut is active"
        ),
    }
}

/// The plugin's handler: a press toggles the dock, the release is ignored.
fn on_shortcut(app: &AppHandle, _shortcut: &Shortcut, event: ShortcutEvent) {
    if event.state == ShortcutState::Pressed {
        log::debug!("toggle shortcut pressed");
        crate::windows::toggle_dock(app);
    }
}

/// The shortcut in `accelerator` (already trimmed): `Ok(None)` when it is
/// empty, else the parsed shortcut or why it cannot be used. Without `Ctrl`,
/// `Alt` or `Super` only `F1`..`F24` are accepted: `RegisterHotKey` takes the
/// combination away from every app, so a plain or shifted letter would stop
/// typing that letter anywhere.
fn parse(accelerator: &str) -> Result<Option<Shortcut>, String> {
    if accelerator.is_empty() {
        return Ok(None);
    }
    let shortcut = Shortcut::from_str(accelerator).map_err(|e| e.to_string())?;
    let held = Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER;
    if !shortcut.mods.intersects(held) && !is_function_key(shortcut.key) {
        return Err("it needs Ctrl, Alt or Super (only F1..F24 may go without)".to_owned());
    }
    Ok(Some(shortcut))
}

fn is_function_key(key: Code) -> bool {
    matches!(
        key,
        Code::F1
            | Code::F2
            | Code::F3
            | Code::F4
            | Code::F5
            | Code::F6
            | Code::F7
            | Code::F8
            | Code::F9
            | Code::F10
            | Code::F11
            | Code::F12
            | Code::F13
            | Code::F14
            | Code::F15
            | Code::F16
            | Code::F17
            | Code::F18
            | Code::F19
            | Code::F20
            | Code::F21
            | Code::F22
            | Code::F23
            | Code::F24
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(accelerator: &str) -> Shortcut {
        match parse(accelerator) {
            Ok(Some(shortcut)) => shortcut,
            other => panic!("{accelerator:?} should parse, got {other:?}"),
        }
    }

    #[test]
    fn the_default_parses_as_ctrl_alt_d() {
        let shortcut = parsed(stem_core::settings::DEFAULT_TOGGLE_SHORTCUT);
        assert_eq!(shortcut.mods, Modifiers::CONTROL | Modifiers::ALT);
        assert_eq!(shortcut.key, Code::KeyD);
        // Case and spaces around `+` do not matter.
        assert_eq!(parsed("ctrl + alt + d"), shortcut);
        assert_eq!(parsed("CONTROL+Option+KeyD"), shortcut);
    }

    #[test]
    fn empty_means_no_shortcut() {
        assert!(matches!(parse(""), Ok(None)));
    }

    #[test]
    fn function_keys_alone_and_held_modifiers_are_accepted() {
        assert_eq!(parsed("F9").key, Code::F9);
        assert_eq!(parsed("Shift+F24").mods, Modifiers::SHIFT);
        assert_eq!(parsed("Super+Space").mods, Modifiers::SUPER);
        assert_eq!(parsed("Alt+Shift+1").key, Code::Digit1);
    }

    #[test]
    fn keys_that_would_take_over_typing_are_refused() {
        for accelerator in ["D", "Shift+D", "Space", "Shift+/", "Enter"] {
            assert!(
                parse(accelerator).is_err(),
                "{accelerator:?} must need Ctrl, Alt or Super"
            );
        }
    }

    #[test]
    fn malformed_accelerators_are_refused() {
        for accelerator in [
            "Ctrl+Alt",
            "Ctrl+Alt+D+E",
            "Ctrl+D+Alt",
            "Ctrl++D",
            "Ctrl+Nope",
            "Hyper+D",
        ] {
            assert!(
                parse(accelerator).is_err(),
                "{accelerator:?} must not parse"
            );
        }
    }
}
