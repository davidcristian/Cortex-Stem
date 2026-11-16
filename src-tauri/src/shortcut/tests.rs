use tauri_plugin_global_shortcut::{Code, Modifiers};

use super::rules::{Steps, parse};
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
fn every_shortcut_given_up_is_unregistered_before_any_is_registered() {
    let toggle = parsed("Ctrl+Alt+D");
    let focus = parsed("Ctrl+Alt+Shift+D");
    let f8 = parsed("Ctrl+Shift+F8");
    let both = [Some(toggle), Some(focus)];
    let steps_to =
        |registered: &[Option<Shortcut>; 2], t: &str, f: &str| steps(registered, &plan(t, f));

    // The first request registers both; the same request again does
    // nothing.
    assert_eq!(
        steps_to(&[None, None], "Ctrl+Alt+D", "Ctrl+Alt+Shift+D"),
        Steps {
            unregister: vec![],
            register: vec![(Kind::Toggle, toggle), (Kind::Focus, focus)],
        }
    );
    assert_eq!(
        steps_to(&both, "ctrl+alt+d", "Ctrl+Alt+Shift+D"),
        Steps::default()
    );
    // One changes: the other is left alone.
    assert_eq!(
        steps_to(&both, "Ctrl+Alt+D", "Ctrl+Shift+F8"),
        Steps {
            unregister: vec![(Kind::Focus, focus)],
            register: vec![(Kind::Focus, f8)],
        }
    );
    // The two trade places in one change: both chords are given up
    // first, so neither registration collides with the other's old one.
    assert_eq!(
        steps_to(&both, "Ctrl+Alt+Shift+D", "Ctrl+Alt+D"),
        Steps {
            unregister: vec![(Kind::Toggle, toggle), (Kind::Focus, focus)],
            register: vec![(Kind::Toggle, focus), (Kind::Focus, toggle)],
        }
    );
    // The toggle is turned off and the focus shortcut takes its chord.
    assert_eq!(
        steps_to(&both, "", "Ctrl+Alt+D"),
        Steps {
            unregister: vec![(Kind::Toggle, toggle), (Kind::Focus, focus)],
            register: vec![(Kind::Focus, toggle)],
        }
    );
    // The focus shortcut moves onto the toggle's chord: refused, so it
    // is only unregistered.
    assert_eq!(
        steps_to(&both, "Ctrl+Alt+D", "alt+ctrl+d"),
        Steps {
            unregister: vec![(Kind::Focus, focus)],
            register: vec![],
        }
    );
    // A shortcut Windows refused earlier (not registered) is tried
    // again; both off unregisters both.
    assert_eq!(
        steps_to(&[Some(toggle), None], "Ctrl+Alt+D", "Ctrl+Alt+Shift+D"),
        Steps {
            unregister: vec![],
            register: vec![(Kind::Focus, focus)],
        }
    );
    assert_eq!(
        steps_to(&both, "", ""),
        Steps {
            unregister: vec![(Kind::Toggle, toggle), (Kind::Focus, focus)],
            register: vec![],
        }
    );
}

#[test]
fn the_outcome_is_the_plan_unless_windows_refused_the_registration() {
    let focus = parsed("Ctrl+Alt+Shift+D");
    assert_eq!(outcome(&Ok(None), None), Outcome::None);
    assert_eq!(outcome(&Ok(Some(focus)), None), Outcome::Active(focus));
    assert_eq!(
        outcome(&Err("it needs Ctrl".to_owned()), None),
        Outcome::Refused("it needs Ctrl".to_owned())
    );
    assert_eq!(
        outcome(&Ok(Some(focus)), Some("held by another app".to_owned())),
        Outcome::Refused("held by another app".to_owned())
    );
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
