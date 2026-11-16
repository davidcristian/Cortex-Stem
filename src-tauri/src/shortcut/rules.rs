//! The pure rules: what each shortcut should be, and the steps that make
//! the registered shortcuts match.

use std::str::FromStr;

use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

use super::Kind;

/// What became of one shortcut, as it was logged last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Outcome {
    /// An empty value: nothing registered, on purpose.
    None,
    /// Registered.
    Active(Shortcut),
    /// Not registered, and why.
    Refused(String),
}

/// What it takes to make the registered shortcuts match their plans: the
/// shortcuts to unregister, then the ones to register, each with its kind.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Steps {
    pub(super) unregister: Vec<(Kind, Shortcut)>,
    pub(super) register: Vec<(Kind, Shortcut)>,
}

/// The [`Steps`] from `registered` (what is registered now) to `plans`
/// ([`plan`]), both indexed by [`Kind::index`]. A shortcut that is
/// registered and still wanted is left alone; one that is no longer wanted
/// (another chord, none, or refused) is unregistered; a wanted one that is
/// not registered is registered. [`apply`] runs every unregistering before
/// any registering, so a chord one shortcut gives up is free for the
/// other, also when the two trade places in one change. Pure.
///
/// [`apply`]: super::apply
pub(super) fn steps(
    registered: &[Option<Shortcut>; 2],
    plans: &[Result<Option<Shortcut>, String>; 2],
) -> Steps {
    let mut steps = Steps::default();
    for kind in Kind::ALL {
        let i = kind.index();
        let desired = plans[i].as_ref().ok().copied().flatten();
        if registered[i] == desired {
            continue;
        }
        if let Some(old) = registered[i] {
            steps.unregister.push((kind, old));
        }
        if let Some(new) = desired {
            steps.register.push((kind, new));
        }
    }
    steps
}

/// What became of a shortcut with this `plan` once [`apply`] ran its
/// [`Steps`]: `refused` is why Windows refused to register it, if it did.
/// A planned shortcut that needed no registering was registered already.
/// Pure.
///
/// [`apply`]: super::apply
pub(super) fn outcome(plan: &Result<Option<Shortcut>, String>, refused: Option<String>) -> Outcome {
    match (plan, refused) {
        (_, Some(reason)) => Outcome::Refused(reason),
        (Err(reason), None) => Outcome::Refused(reason.clone()),
        (Ok(None), None) => Outcome::None,
        (Ok(Some(shortcut)), None) => Outcome::Active(*shortcut),
    }
}

/// What each shortcut should be for the trimmed settings values `toggle`
/// and `focus`, indexed by [`Kind::index`]: each one's [`parse`], except
/// that a focus shortcut naming the toggle's chord (the same modifiers and
/// key, however written) is refused, so the toggle keeps it. Pure.
pub(super) fn plan(toggle: &str, focus: &str) -> [Result<Option<Shortcut>, String>; 2] {
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
pub(super) fn parse(accelerator: &str) -> Result<Option<Shortcut>, String> {
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
