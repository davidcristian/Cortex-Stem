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

use std::str::FromStr;

use parking_lot::Mutex;
use stem_core::Settings;
use stem_service::DockService;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_global_shortcut::{
    Code, GlobalShortcut, Modifiers, Shortcut, ShortcutEvent, ShortcutState,
};

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

/// What became of one shortcut, as it was logged last.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    /// An empty value: nothing registered, on purpose.
    None,
    /// Registered.
    Active(Shortcut),
    /// Not registered, and why.
    Refused(String),
}

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

/// Make the registered shortcuts match `wanted` ([`plan`]). Main thread.
fn apply(app: &AppHandle, wanted: &Wanted) {
    let (Some(plugin), Some(state)) = (
        app.try_state::<GlobalShortcut<Wry>>(),
        app.try_state::<Shortcuts>(),
    ) else {
        return;
    };
    let plans = plan(&wanted[0], &wanted[1]);
    let mut applied = state.applied.lock();

    // Everything that is no longer wanted goes first, so a chord one
    // shortcut gives up is free for the other.
    for kind in Kind::ALL {
        let i = kind.index();
        let desired = plans[i].as_ref().ok().copied().flatten();
        if applied.registered[i] == desired {
            continue;
        }
        if let Some(old) = applied.registered[i].take() {
            match plugin.unregister(old) {
                Ok(()) => log::debug!("{} {old} unregistered", kind.name()),
                Err(e) => log::warn!("{} {old}: unregistering failed: {e}", kind.name()),
            }
        }
    }

    for kind in Kind::ALL {
        let i = kind.index();
        let outcome = match &plans[i] {
            Ok(None) => Outcome::None,
            Err(reason) => Outcome::Refused(reason.clone()),
            Ok(Some(shortcut)) if applied.registered[i] == Some(*shortcut) => {
                Outcome::Active(*shortcut)
            }
            Ok(Some(shortcut)) => match plugin.on_shortcut(*shortcut, kind.handler()) {
                Ok(()) => {
                    applied.registered[i] = Some(*shortcut);
                    Outcome::Active(*shortcut)
                }
                Err(e) => Outcome::Refused(format!(
                    "it could not be registered ({e}); another app may hold it"
                )),
            },
        };
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

/// What each shortcut should be for the trimmed settings values `toggle`
/// and `focus`, indexed by [`Kind::index`]: each one's [`parse`], except
/// that a focus shortcut naming the toggle's chord (the same modifiers and
/// key, however written) is refused, so the toggle keeps it. Pure.
fn plan(toggle: &str, focus: &str) -> [Result<Option<Shortcut>, String>; 2] {
    let toggle_plan = parse(toggle);
    let focus_plan = match (parse(focus), &toggle_plan) {
        (Ok(Some(f)), Ok(Some(t))) if f.mods == t.mods && f.key == t.key => Err(format!(
            "it is the same shortcut as the toggle shortcut ({toggle})"
        )),
        (other, _) => other,
    };
    [toggle_plan, focus_plan]
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
    fn the_focus_default_parses_as_ctrl_alt_shift_d_and_differs_from_the_toggle() {
        let focus = parsed(stem_core::settings::DEFAULT_FOCUS_SHORTCUT);
        assert_eq!(
            focus.mods,
            Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT
        );
        assert_eq!(focus.key, Code::KeyD);
        let [toggle, focus] = plan(
            stem_core::settings::DEFAULT_TOGGLE_SHORTCUT,
            stem_core::settings::DEFAULT_FOCUS_SHORTCUT,
        );
        assert_eq!(toggle, Ok(Some(parsed("Ctrl+Alt+D"))));
        assert_eq!(focus, Ok(Some(parsed("Ctrl+Alt+Shift+D"))));
    }

    #[test]
    fn a_focus_shortcut_on_the_toggles_chord_is_refused_and_the_toggle_keeps_it() {
        // Case and modifier order do not make another chord.
        for focus in ["Ctrl+Alt+D", "alt+ctrl+d", "CONTROL+Option+KeyD"] {
            let [toggle, refused] = plan("Ctrl+Alt+D", focus);
            assert_eq!(toggle, Ok(Some(parsed("Ctrl+Alt+D"))), "{focus:?}");
            match refused {
                Err(reason) => assert!(reason.contains("toggle shortcut"), "{reason}"),
                Ok(other) => panic!("{focus:?} must be refused, got {other:?}"),
            }
        }
        // Another chord is fine, and so is the same text with the toggle off
        // or unusable.
        let [_, focus] = plan("Ctrl+Alt+D", "Ctrl+Alt+F");
        assert_eq!(focus, Ok(Some(parsed("Ctrl+Alt+F"))));
        let [toggle, focus] = plan("", "Ctrl+Alt+D");
        assert_eq!(toggle, Ok(None));
        assert_eq!(focus, Ok(Some(parsed("Ctrl+Alt+D"))));
        let [toggle, focus] = plan("Shift+D", "Ctrl+Alt+D");
        assert!(toggle.is_err());
        assert_eq!(focus, Ok(Some(parsed("Ctrl+Alt+D"))));
        // Both empty: nothing at all; a bad focus value keeps its own reason.
        assert_eq!(plan("", ""), [Ok(None), Ok(None)]);
        match plan("Ctrl+Alt+D", "Shift+D") {
            [_, Err(reason)] => assert!(!reason.contains("toggle shortcut"), "{reason}"),
            other => panic!("Shift+D must be refused, got {other:?}"),
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
