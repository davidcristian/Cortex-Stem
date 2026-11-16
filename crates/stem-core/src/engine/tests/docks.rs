//! One dock per monitor: each dock window has its own facts.

use pretty_assertions::assert_eq;

use super::*;

/// A 560 × 72 panel at the bottom of the second monitor's work area.
const SECOND_PANEL: Rect = Rect::new(3560, 1320, 560, 72);
/// A frame on the second monitor that covers that panel's left half.
const SECOND_OVER: Rect = Rect::new(3160, 900, 700, 480);

#[test]
fn each_dock_is_overlapped_by_the_windows_over_its_own_panel() {
    let mut engine = DockEngine::new(minimal_settings());
    engine.apply(monitors(vec![
        monitor(PRIMARY, true, 0),
        monitor(SECOND, false, 2560),
    ]));
    engine.apply(panel_of("dock", Some(PRIMARY), Some(PANEL)));
    engine.apply(panel_of("dock-DISPLAY3", Some(SECOND), Some(SECOND_PANEL)));
    // Two entries, sorted by monitor id (`\\.\DISPLAY3` < `mock-primary`).
    let state = engine.snapshot();
    assert_eq!(
        state
            .docks
            .iter()
            .map(|d| d.monitor.0.as_str())
            .collect::<Vec<_>>(),
        vec![SECOND, PRIMARY]
    );
    assert!(state.docks.iter().all(|d| !d.overlapped && !d.fullscreen));
    let r = state.revision;

    // A window over the second dock's panel: only that dock.
    assert!(
        engine
            .apply(added(framed(1, r"C:\a.exe", SECOND_OVER)))
            .changed
    );
    assert!(!is_overlapped(&engine, PRIMARY));
    assert!(is_overlapped(&engine, SECOND));
    assert!(engine.overlapped("dock-DISPLAY3"));
    assert!(!engine.overlapped("dock"));
    // One over the primary's as well: both.
    let r2 = engine.snapshot().revision;
    assert!(engine.apply(added(framed(2, r"C:\b.exe", OVER))).changed);
    assert!(is_overlapped(&engine, PRIMARY));
    assert!(is_overlapped(&engine, SECOND));
    assert!(r2 > r);
    // The first one dragged from the second monitor to the primary,
    // over the primary's panel: the second clears, the primary stays.
    assert!(engine.apply(updated(framed(1, r"C:\a.exe", OVER))).changed);
    assert!(is_overlapped(&engine, PRIMARY));
    assert!(!is_overlapped(&engine, SECOND));

    // Dragged back under the second dock: that flag is set again (the
    // primary's stays, window 2 is still there).
    assert!(
        engine
            .apply(updated(framed(1, r"C:\a.exe", SECOND_OVER)))
            .changed
    );
    assert!(is_overlapped(&engine, PRIMARY));
    assert!(is_overlapped(&engine, SECOND));

    // The second dock's window is hidden by the tray: its flag clears,
    // its entry stays; shown again: back.
    assert!(
        engine
            .apply(panel_of("dock-DISPLAY3", Some(SECOND), None))
            .changed
    );
    assert!(!is_overlapped(&engine, SECOND));
    assert!(facts(&engine.snapshot(), SECOND).is_some());
    assert!(
        engine
            .apply(panel_of("dock-DISPLAY3", Some(SECOND), Some(SECOND_PANEL)))
            .changed
    );
    assert!(is_overlapped(&engine, SECOND));
}

#[test]
fn docks_come_and_go_with_their_windows_and_only_that_bumps() {
    let mut engine = DockEngine::new(minimal_settings());
    engine.apply(monitors(vec![
        monitor(PRIMARY, true, 0),
        monitor(SECOND, false, 2560),
    ]));
    engine.apply(panel_of("dock", Some(PRIMARY), Some(PANEL)));
    let r = engine.snapshot().revision;
    assert_eq!(engine.snapshot().docks.len(), 1);

    // A second monitor's dock is created: one new entry, one bump.
    assert!(
        engine
            .apply(panel_of("dock-DISPLAY3", Some(SECOND), None))
            .changed
    );
    assert_eq!(engine.snapshot().docks.len(), 2);
    assert_eq!(engine.snapshot().revision, r + 1);
    // Its page reports a panel clear of every window: no bump.
    assert!(
        !engine
            .apply(panel_of("dock-DISPLAY3", Some(SECOND), Some(SECOND_PANEL)))
            .changed
    );
    assert_eq!(engine.dock_panel("dock-DISPLAY3"), Some(SECOND_PANEL));
    assert_eq!(engine.snapshot().revision, r + 1);

    // The monitor is unplugged and its window closed: the entry goes,
    // one bump; the label is forgotten.
    assert!(engine.apply(panel_of("dock-DISPLAY3", None, None)).changed);
    assert_eq!(engine.snapshot().docks.len(), 1);
    assert_eq!(engine.snapshot().revision, r + 2);
    assert_eq!(engine.dock_panel("dock-DISPLAY3"), None);
    assert_eq!(engine.dock_monitor("dock-DISPLAY3"), None);
    assert!(!engine.overlapped("dock-DISPLAY3"));
    // Forgetting it again is a no-op.
    assert!(!engine.apply(panel_of("dock-DISPLAY3", None, None)).changed);
    assert_eq!(engine.snapshot().revision, r + 2);
}

#[test]
fn a_dock_whose_monitor_is_unknown_has_no_entry_until_it_is_placed() {
    let mut engine = DockEngine::new(minimal_settings());
    engine.apply(monitors(vec![monitor(PRIMARY, true, 0)]));
    engine.apply(added(framed(1, r"C:\a.exe", OVER)));
    let r = engine.snapshot().revision;

    // The app layer had no monitor list when it placed the window: the
    // panel is remembered, but nothing is published for it.
    assert!(!engine.apply(panel_of("dock", None, Some(PANEL))).changed);
    assert!(engine.snapshot().docks.is_empty());
    assert_eq!(engine.dock_panel("dock"), Some(PANEL));
    assert!(
        engine.overlapped("dock"),
        "the flag is kept up to date meanwhile"
    );
    assert_eq!(engine.snapshot().revision, r);
    // A fullscreen verdict changes nothing visible either.
    assert!(!engine.apply(fullscreen_on(None)).changed);

    // The monitors arrive and the window is placed: the entry appears
    // with both facts right, in one bump.
    assert!(
        engine
            .apply(panel_of("dock", Some(PRIMARY), Some(PANEL)))
            .changed
    );
    assert_eq!(
        engine.snapshot().docks,
        vec![DockFacts {
            monitor: MonitorId(PRIMARY.into()),
            fullscreen: true,
            overlapped: true,
        }]
    );
    assert_eq!(engine.snapshot().revision, r + 1);
}

#[test]
fn two_docks_on_one_monitor_fold_into_one_entry() {
    let mut engine = DockEngine::new(minimal_settings());
    engine.apply(monitors(vec![monitor(PRIMARY, true, 0)]));
    engine.apply(added(framed(1, r"C:\a.exe", OVER)));
    engine.apply(panel_of("dock", Some(PRIMARY), Some(CLEAR)));
    engine.apply(panel_of("dock-extra", Some(PRIMARY), Some(PANEL)));
    let state = engine.snapshot();
    assert_eq!(state.docks.len(), 1);
    assert!(state.docks[0].overlapped, "any panel of the monitor counts");
    assert!(!engine.overlapped("dock"));
    assert!(engine.overlapped("dock-extra"));
}

#[test]
fn a_docks_monitor_can_change_without_its_panel_changing() {
    // The primary monitor changed in Windows Settings: the `dock` window
    // moves to the new primary and the app layer re-registers it there.
    let mut engine = DockEngine::new(minimal_settings());
    engine.apply(monitors(vec![
        monitor(PRIMARY, true, 0),
        monitor(SECOND, false, 2560),
    ]));
    engine.apply(panel_of("dock", Some(PRIMARY), Some(PANEL)));
    engine.apply(fullscreen_on(None));
    assert!(is_fullscreen(&engine, PRIMARY));
    let r = engine.snapshot().revision;

    engine.apply(monitors(vec![
        monitor(PRIMARY, false, 0),
        monitor(SECOND, true, 2560),
    ]));
    // The verdict without a monitor now points at the second monitor,
    // where no dock is yet: the primary's dock clears (one bump).
    assert!(!is_fullscreen(&engine, PRIMARY));
    assert_eq!(engine.snapshot().revision, r + 1);
    assert!(
        engine
            .apply(panel_of("dock", Some(SECOND), Some(SECOND_PANEL)))
            .changed
    );
    assert_eq!(engine.dock_monitor("dock"), Some(&MonitorId(SECOND.into())));
    assert!(is_fullscreen(&engine, SECOND));
    assert!(facts(&engine.snapshot(), PRIMARY).is_none());
}
