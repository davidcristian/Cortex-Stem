//! Global shortcuts, both from `behavior` and both live:
//! - the toggle: `toggleShortcut` (default `Ctrl+Alt+D`) does from
//!   any app what a left click on the tray icon does
//!   ([`crate::windows::toggle_dock`]);
//! - the focus shortcut (`docs/ipc/keyboard-entry.md`):
//!   `focusShortcut` (default `Ctrl+Alt+Shift+D`) gives a dock window the
//!   keyboard focus, or gives it back when a dock already has it
//!   ([`crate::windows::on_focus_shortcut`]).
//!
//! `tauri-plugin-global-shortcut` registers each with `RegisterHotKey` (and
//! `MOD_NOREPEAT`, so holding the keys presses once) on a hidden window that
//! belongs to the main thread; that window's procedure reports the press on
//! the main thread, inside the `WM_HOTKEY` that gives Stem the right to take
//! the foreground. The plugin also reports the release (from a short-lived
//! polling thread), which is ignored here.
//!
//! Both follow the same rules ([`parse`]). A value that does not parse,
//! that would take over plain typing or that Windows refuses (another app
//! holds it) logs a warning and leaves that shortcut unregistered; the
//! settings value stays as the user wrote it, so they can correct it. An
//! empty value registers nothing. A focus shortcut that names the toggle's
//! chord (case and modifier order aside: the parsed shortcuts are equal)
//! registers nothing either, with a warning; the toggle keeps the chord
//! ([`plan`]).
//!
//! A change of either value re-plans both: first every registered shortcut
//! that is no longer wanted is unregistered, then every wanted one that is
//! not registered yet is registered, so two values that trade places in one
//! change never collide with each other. One log line per shortcut whose
//! outcome changed (registered, none, or refused and why).
//!
//! Threads: the plugin runs every (un)registration on the main thread and
//! makes the caller wait for it. Settings events arrive on the service
//! worker, where that wait could deadlock against a main thread that is
//! itself waiting on the service, so a change hops to the main thread
//! without waiting; there the plugin's own hop runs inline.
//!
//! [`parse`]: rules::parse

mod rules;
#[cfg(test)]
mod tests;

use parking_lot::Mutex;
use stem_core::Settings;
use stem_service::DockService;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcut, Shortcut, ShortcutEvent, ShortcutState};

use rules::{Outcome, outcome, plan, steps};

/// The two shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Toggle,
    Focus,
}

impl Kind {
    const ALL: [Kind; 2] = [Kind::Toggle, Kind::Focus];

    fn name(self) -> &'static str {
        match self {
            Kind::Toggle => "toggle shortcut",
            Kind::Focus => "focus shortcut",
        }
    }

    fn index(self) -> usize {
        match self {
            Kind::Toggle => 0,
            Kind::Focus => 1,
        }
    }

    /// The plugin's handler for this shortcut.
    fn handler(self) -> fn(&AppHandle, &Shortcut, ShortcutEvent) {
        match self {
            Kind::Toggle => on_toggle,
            Kind::Focus => on_focus,
        }
    }
}

/// The trimmed accelerators the settings ask for, indexed by [`Kind::index`].
type Wanted = [String; 2];

/// Managed state; it exists only while the plugin is registered.
#[derive(Debug, Default)]
struct Shortcuts {
    /// What the latest settings ask for; `None` before the first request.
    wanted: Mutex<Option<Wanted>>,
    /// What is registered now and what was logged. Only the main thread
    /// touches it.
    applied: Mutex<Applied>,
}

#[derive(Debug, Default)]
struct Applied {
    registered: [Option<Shortcut>; 2],
    outcome: [Option<Outcome>; 2],
}

/// Register the plugin and the shortcuts from the settings. Runs in `setup`
/// (main thread), after `forward_events` and once the dock window exists;
/// the settings are read once the state is managed, so a change in between
/// is not lost. If the plugin cannot start, neither setting has an effect
/// this session and the reason is logged once.
pub fn init(app: &AppHandle, service: &DockService) {
    if let Err(e) = app.plugin(tauri_plugin_global_shortcut::Builder::new().build()) {
        log::warn!("global shortcuts unavailable: {e}");
        return;
    }
    app.manage(Shortcuts::default());
    on_settings_changed(app, &service.settings());
}

/// The settings changed (or the app started): re-plan both shortcuts when
/// `behavior.toggleShortcut` or `behavior.focusShortcut` differs from the
/// previous request. Any thread; never blocks.
pub fn on_settings_changed(app: &AppHandle, settings: &Settings) {
    let Some(state) = app.try_state::<Shortcuts>() else {
        return;
    };
    let wanted: Wanted = [
        settings.behavior.toggle_shortcut.trim().to_owned(),
        settings.behavior.focus_shortcut.trim().to_owned(),
    ];
    {
        let mut requested = state.wanted.lock();
        if requested.as_ref() == Some(&wanted) {
            return;
        }
        *requested = Some(wanted.clone());
    }
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || apply(&handle, &wanted)) {
        log::warn!("could not schedule the global-shortcut change: {e}");
    }
}

/// Make the registered shortcuts match `wanted` ([`plan`], [`steps`]).
/// Main thread.
fn apply(app: &AppHandle, wanted: &Wanted) {
    let (Some(plugin), Some(state)) = (
        app.try_state::<GlobalShortcut<Wry>>(),
        app.try_state::<Shortcuts>(),
    ) else {
        return;
    };
    let plans = plan(&wanted[0], &wanted[1]);
    let mut applied = state.applied.lock();
    let steps = steps(&applied.registered, &plans);

    for &(kind, old) in &steps.unregister {
        applied.registered[kind.index()] = None;
        match plugin.unregister(old) {
            Ok(()) => log::debug!("{} {old} unregistered", kind.name()),
            Err(e) => log::warn!("{} {old}: unregistering failed: {e}", kind.name()),
        }
    }

    let mut refused: [Option<String>; 2] = [None, None];
    for &(kind, shortcut) in &steps.register {
        match plugin.on_shortcut(shortcut, kind.handler()) {
            Ok(()) => applied.registered[kind.index()] = Some(shortcut),
            Err(e) => {
                refused[kind.index()] = Some(format!(
                    "it could not be registered ({e}); another app may hold it"
                ));
            }
        }
    }

    for kind in Kind::ALL {
        let i = kind.index();
        let outcome = outcome(&plans[i], refused[i].take());
        if applied.outcome[i].as_ref() != Some(&outcome) {
            log_outcome(kind, &wanted[i], &outcome);
            applied.outcome[i] = Some(outcome);
        }
    }
}

fn log_outcome(kind: Kind, wanted: &str, outcome: &Outcome) {
    let name = kind.name();
    match outcome {
        Outcome::None => log::info!("{name}: none"),
        Outcome::Active(_) => log::info!("{name}: {wanted}"),
        Outcome::Refused(reason) => {
            log::warn!("{name} {wanted:?} ignored: {reason}; no {name} is active");
        }
    }
}

/// The plugin's handler for the toggle: a press toggles the dock, the
/// release is ignored.
fn on_toggle(app: &AppHandle, _shortcut: &Shortcut, event: ShortcutEvent) {
    if event.state == ShortcutState::Pressed {
        log::debug!("toggle shortcut pressed");
        crate::windows::toggle_dock(app);
    }
}

/// The plugin's handler for the focus shortcut: a press gives a dock the
/// keyboard focus (or gives it back), the release is ignored.
fn on_focus(app: &AppHandle, _shortcut: &Shortcut, event: ShortcutEvent) {
    if event.state == ShortcutState::Pressed {
        log::debug!("focus shortcut pressed");
        crate::windows::on_focus_shortcut(app);
    }
}
