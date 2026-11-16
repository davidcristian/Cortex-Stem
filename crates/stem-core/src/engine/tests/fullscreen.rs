//! The fullscreen fact, per dock.

use pretty_assertions::assert_eq;

use super::*;

/// An engine that knows the primary monitor and has the primary dock
/// window registered on it, without a panel rect yet.
fn engine_with_primary_dock() -> DockEngine {
    let mut engine = DockEngine::new(minimal_settings());
    engine.apply(monitors(vec![monitor(PRIMARY, true, 0)]));
    engine.apply(panel_of("dock", Some(PRIMARY), None));
    engine
}

#[test]
fn fullscreen_is_stored_and_bumps_only_on_a_change() {
    let mut engine = engine_with_primary_dock();
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    let r = engine.snapshot().revision;
    assert!(!is_fullscreen(&engine, PRIMARY));
    assert!(!engine.fullscreen());
    assert_eq!(engine.fullscreen_monitor(), None);

    // The platform's first verdict is usually `false`: nothing to emit.
    assert!(!engine.apply(fullscreen_off()).changed);
    assert_eq!(engine.snapshot().revision, r);

    // A verdict that names the dock's monitor.
    let out = engine.apply(fullscreen_on(Some(PRIMARY)));
    assert!(out.changed);
    assert!(out.unresolved_apps.is_empty());
    let state = engine.snapshot();
    assert_eq!(
        state.docks,
        vec![DockFacts {
            monitor: MonitorId(PRIMARY.into()),
            fullscreen: true,
            overlapped: false,
        }]
    );
    assert!(engine.fullscreen());
    assert_eq!(
        engine.fullscreen_monitor(),
        Some(&MonitorId(PRIMARY.into()))
    );
    assert_eq!(state.revision, r + 1);
    // Items and foreground are untouched by it.
    assert_eq!(keys(&state), vec!["app:exe:c:/a.exe"]);

    // Repeats change nothing; the end of fullscreen is one more bump.
    assert!(!engine.apply(fullscreen_on(Some(PRIMARY))).changed);
    assert_eq!(engine.snapshot().revision, r + 1);
    assert!(engine.apply(fullscreen_off()).changed);
    assert!(!is_fullscreen(&engine, PRIMARY));
    assert!(!engine.fullscreen());
    assert_eq!(engine.fullscreen_monitor(), None);
    assert_eq!(engine.snapshot().revision, r + 2);

    // A verdict that names no monitor lands on the primary's dock.
    assert!(engine.apply(fullscreen_on(None)).changed);
    assert!(is_fullscreen(&engine, PRIMARY));
    assert_eq!(engine.fullscreen_monitor(), None);
}

#[test]
fn fullscreen_survives_a_resync_and_is_independent_of_the_foreground() {
    let mut engine = engine_with_primary_dock();
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    engine.apply(fullscreen_on(Some(PRIMARY)));
    engine.apply(foreground(Some(1)));
    engine.apply(PlatformEvent::WindowsSnapshot(vec![win(1, r"C:\a.exe", "A")]).into());
    engine.apply(foreground(None));
    engine.apply(removed(1));
    // Only the platform says when it is over.
    assert!(is_fullscreen(&engine, PRIMARY));
}

#[test]
fn fullscreen_lands_on_the_dock_of_the_verdicts_monitor_only() {
    let mut engine = DockEngine::new(minimal_settings());
    engine.apply(monitors(vec![
        monitor(PRIMARY, true, 0),
        monitor(SECOND, false, 2560),
    ]));
    engine.apply(panel_of("dock", Some(PRIMARY), None));
    engine.apply(panel_of("dock-DISPLAY3", Some(SECOND), None));
    let r = engine.snapshot().revision;

    // A video goes fullscreen on the second monitor: only its dock hides.
    assert!(engine.apply(fullscreen_on(Some(SECOND))).changed);
    assert!(!is_fullscreen(&engine, PRIMARY));
    assert!(is_fullscreen(&engine, SECOND));
    assert_eq!(engine.snapshot().revision, r + 1);

    // The fullscreen window moves to the primary: the fact moves with it.
    assert!(engine.apply(fullscreen_on(Some(PRIMARY))).changed);
    assert!(is_fullscreen(&engine, PRIMARY));
    assert!(!is_fullscreen(&engine, SECOND));

    // A verdict with no monitor: the primary's dock, not the other.
    assert!(!engine.apply(fullscreen_on(None)).changed);
    assert!(is_fullscreen(&engine, PRIMARY));
    assert!(!is_fullscreen(&engine, SECOND));

    // A monitor no dock sits on: nobody hides.
    assert!(engine.apply(fullscreen_on(Some(r"\\.\DISPLAY9"))).changed);
    assert!(!is_fullscreen(&engine, PRIMARY));
    assert!(!is_fullscreen(&engine, SECOND));
    assert!(engine.fullscreen());

    assert!(!engine.apply(fullscreen_off()).changed);
    assert!(!engine.fullscreen());
}

#[test]
fn without_a_flagged_primary_the_first_monitor_takes_an_unplaced_verdict() {
    let mut engine = DockEngine::new(minimal_settings());
    engine.apply(monitors(vec![
        monitor(SECOND, false, 2560),
        monitor(PRIMARY, false, 0),
    ]));
    engine.apply(panel_of("dock", Some(SECOND), None));
    engine.apply(panel_of("dock-mock-primary", Some(PRIMARY), None));
    engine.apply(fullscreen_on(None));
    assert!(is_fullscreen(&engine, SECOND));
    assert!(!is_fullscreen(&engine, PRIMARY));
    // No monitors known at all: an unplaced verdict hides nobody.
    engine.apply(monitors(vec![]));
    assert!(!is_fullscreen(&engine, SECOND));
    assert!(!is_fullscreen(&engine, PRIMARY));
}

#[test]
fn a_rect_change_alone_is_stored_without_bumping_the_revision() {
    let mut engine = engine_with_primary_dock();
    let mut raw = win(1, r"C:\a.exe", "A");
    raw.rect = Some(Rect::new(0, 0, 800, 600));
    engine.apply(added(raw.clone()));
    let r = engine.snapshot().revision;
    assert_eq!(
        engine.window(WindowId(1)).and_then(|w| w.rect),
        Some(Rect::new(0, 0, 800, 600))
    );

    // The window moved (the next query of it carried the new frame).
    raw.rect = Some(Rect::new(100, 50, 800, 600));
    assert!(!engine.apply(updated(raw.clone())).changed);
    assert_eq!(engine.snapshot().revision, r);
    assert_eq!(
        engine.window(WindowId(1)).and_then(|w| w.rect),
        Some(Rect::new(100, 50, 800, 600))
    );

    // A platform that cannot read the frame reports `None`; still no bump.
    raw.rect = None;
    assert!(!engine.apply(updated(raw.clone())).changed);
    assert_eq!(engine.window(WindowId(1)).and_then(|w| w.rect), None);
    assert_eq!(engine.snapshot().revision, r);

    // With a panel known, a move that stays clear of it is still
    // stored without a bump.
    assert!(!engine.apply(panel(Some(PANEL))).changed);
    raw.rect = Some(Rect::new(0, 0, 800, 600));
    assert!(!engine.apply(updated(raw)).changed);
    assert_eq!(engine.snapshot().revision, r);
    assert!(!engine.overlapped("dock"));
}
