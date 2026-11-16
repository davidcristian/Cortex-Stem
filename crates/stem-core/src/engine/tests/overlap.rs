//! The overlap fact: a window over a dock's panel.

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn overlapped_follows_the_panel_and_the_windows_and_bumps_only_when_it_flips() {
    let mut engine = DockEngine::new(minimal_settings());
    engine.apply(added(framed(1, r"C:\a.exe", OVER)));
    let r = engine.snapshot().revision;
    // No panel yet: nothing can overlap it, and there is no dock entry.
    assert!(!engine.overlapped("dock"));
    assert!(engine.snapshot().docks.is_empty());
    assert_eq!(engine.dock_panel("dock"), None);
    assert_eq!(engine.dock_monitor("dock"), None);

    // The UI's first report: the window is over it. The dock gets its
    // entry (one bump) with the flag set.
    let out = engine.apply(panel(Some(PANEL)));
    assert!(out.changed);
    assert!(out.unresolved_apps.is_empty());
    assert!(engine.overlapped("dock"));
    assert_eq!(engine.dock_panel("dock"), Some(PANEL));
    assert_eq!(
        engine.dock_monitor("dock"),
        Some(&MonitorId(PRIMARY.into()))
    );
    let state = engine.snapshot();
    assert_eq!(
        state.docks,
        vec![DockFacts {
            monitor: MonitorId(PRIMARY.into()),
            fullscreen: false,
            overlapped: true,
        }]
    );
    assert_eq!(state.revision, r + 1);
    assert_eq!(keys(&state), vec!["app:exe:c:/a.exe"]);

    // The same panel again, and a move that keeps covering it: no bump.
    assert!(!engine.apply(panel(Some(PANEL))).changed);
    let mut moved = framed(1, r"C:\a.exe", Rect::new(650, 950, 700, 480));
    assert!(!engine.apply(updated(moved.clone())).changed);
    assert_eq!(engine.snapshot().revision, r + 1);

    // Moved clear of it: the flag flips, one bump.
    moved.rect = Some(CLEAR);
    assert!(engine.apply(updated(moved.clone())).changed);
    assert!(!is_overlapped(&engine, PRIMARY));
    assert_eq!(engine.snapshot().revision, r + 2);

    // Back over it, then minimised, restored, cloaked, uncloaked, hidden.
    moved.rect = Some(OVER);
    assert!(engine.apply(updated(moved.clone())).changed);
    assert!(is_overlapped(&engine, PRIMARY));
    moved.is_minimized = true;
    assert!(engine.apply(updated(moved.clone())).changed);
    assert!(!is_overlapped(&engine, PRIMARY));
    moved.is_minimized = false;
    assert!(engine.apply(updated(moved.clone())).changed);
    assert!(is_overlapped(&engine, PRIMARY));
    moved.is_cloaked = true;
    assert!(engine.apply(updated(moved.clone())).changed);
    assert!(!is_overlapped(&engine, PRIMARY));
    moved.is_cloaked = false;
    assert!(engine.apply(updated(moved.clone())).changed);
    assert!(is_overlapped(&engine, PRIMARY));
    moved.is_visible = false;
    assert!(engine.apply(updated(moved.clone())).changed);
    assert!(!is_overlapped(&engine, PRIMARY));
    moved.is_visible = true;
    assert!(engine.apply(updated(moved)).changed);
    assert!(is_overlapped(&engine, PRIMARY));

    // The window closes: clear. A new window over it: set again.
    assert!(engine.apply(removed(1)).changed);
    assert!(!is_overlapped(&engine, PRIMARY));
    assert!(engine.apply(added(framed(2, r"C:\b.exe", OVER))).changed);
    assert!(is_overlapped(&engine, PRIMARY));

    // The dock window was hidden: no panel, nothing overlaps; the dock
    // keeps its entry (it is still on its monitor).
    let r = engine.snapshot().revision;
    assert!(engine.apply(panel(None)).changed);
    assert!(!is_overlapped(&engine, PRIMARY));
    assert!(facts(&engine.snapshot(), PRIMARY).is_some());
    assert_eq!(engine.dock_panel("dock"), None);
    assert_eq!(engine.snapshot().revision, r + 1);
    // Shown again where it was: set again.
    assert!(engine.apply(panel(Some(PANEL))).changed);
    assert!(is_overlapped(&engine, PRIMARY));
}

#[test]
fn a_panel_change_that_does_not_flip_overlapped_does_not_bump() {
    let mut engine = DockEngine::new(minimal_settings());
    engine.apply(added(framed(1, r"C:\a.exe", CLEAR)));
    engine.apply(panel(Some(PANEL)));
    let r = engine.snapshot().revision;
    // The dock grew by an item: a wider panel, still clear of the window.
    let wider = Rect::new(972, 1320, 616, 72);
    assert!(!engine.apply(panel(Some(wider))).changed);
    assert_eq!(engine.dock_panel("dock"), Some(wider));
    assert_eq!(engine.snapshot().revision, r);
    assert!(!engine.apply(panel(None)).changed);
    assert_eq!(engine.snapshot().revision, r);
}

#[test]
fn overlap_ignores_cloaked_windows_whatever_show_only_current_desktop_says() {
    let mut settings = minimal_settings();
    settings.behavior.show_only_current_desktop = false;
    let mut engine = DockEngine::new(settings);
    engine.apply(added(cloaked(framed(1, r"C:\a.exe", OVER))));
    engine.apply(panel(Some(PANEL)));
    // The item is shown (the setting lists other desktops' windows) ...
    assert_eq!(keys(&engine.snapshot()), vec!["app:exe:c:/a.exe"]);
    // ... but a cloaked window is not on screen, so it covers nothing.
    assert!(!is_overlapped(&engine, PRIMARY));
    assert!(engine.apply(updated(framed(1, r"C:\a.exe", OVER))).changed);
    assert!(is_overlapped(&engine, PRIMARY));
}

#[test]
fn our_own_windows_never_overlap_the_dock() {
    let mut engine = DockEngine::new(minimal_settings());
    let mut ours = framed(1, r"C:\stem.exe", OVER);
    ours.process_id = 4242;
    engine.apply(added(ours.clone()));
    engine.apply(panel(Some(PANEL)));
    assert!(is_overlapped(&engine, PRIMARY));

    // The service tells the engine who we are: the flag clears with the
    // next input that touches the rule, without a bump of its own.
    engine.set_own_process_id(Some(4242));
    let out = engine.apply(updated(ours.clone()));
    assert!(out.changed);
    assert!(!is_overlapped(&engine, PRIMARY));
    // A window of another process over it still counts.
    let mut theirs = framed(2, r"C:\b.exe", OVER);
    theirs.process_id = 4243;
    assert!(engine.apply(added(theirs)).changed);
    assert!(is_overlapped(&engine, PRIMARY));
    assert!(engine.apply(removed(2)).changed);
    assert!(!is_overlapped(&engine, PRIMARY));
    // Forgetting the id makes ours count again.
    engine.set_own_process_id(None);
    assert!(engine.apply(updated(ours)).changed);
    assert!(is_overlapped(&engine, PRIMARY));
}

#[test]
fn a_resync_recomputes_overlap() {
    let mut engine = DockEngine::new(minimal_settings());
    engine.apply(panel(Some(PANEL)));
    engine.apply(added(framed(1, r"C:\a.exe", CLEAR)));
    assert!(!is_overlapped(&engine, PRIMARY));

    // The platform re-enumerates: the window has moved over the dock.
    let out =
        engine.apply(PlatformEvent::WindowsSnapshot(vec![framed(1, r"C:\a.exe", OVER)]).into());
    assert!(out.changed);
    assert!(is_overlapped(&engine, PRIMARY));
    // A resync that lists nothing clears it.
    assert!(
        engine
            .apply(PlatformEvent::WindowsSnapshot(vec![]).into())
            .changed
    );
    assert!(!is_overlapped(&engine, PRIMARY));
    // Identical resyncs are no-ops.
    engine.apply(PlatformEvent::WindowsSnapshot(vec![framed(1, r"C:\a.exe", OVER)]).into());
    let r = engine.snapshot().revision;
    assert!(
        !engine
            .apply(PlatformEvent::WindowsSnapshot(vec![framed(1, r"C:\a.exe", OVER)]).into())
            .changed
    );
    assert_eq!(engine.snapshot().revision, r);
}

#[test]
fn overlap_is_independent_of_fullscreen_the_foreground_and_the_settings() {
    let mut settings = minimal_settings();
    let mut engine = DockEngine::new(settings.clone());
    engine.apply(monitors(vec![monitor(PRIMARY, true, 0)]));
    engine.apply(added(framed(1, r"C:\a.exe", OVER)));
    engine.apply(panel(Some(PANEL)));
    assert!(is_overlapped(&engine, PRIMARY));

    engine.apply(foreground(Some(1)));
    assert!(is_overlapped(&engine, PRIMARY));
    let state = engine.snapshot();
    engine.apply(fullscreen_on(Some(PRIMARY)));
    assert!(is_overlapped(&engine, PRIMARY));
    assert!(is_fullscreen(&engine, PRIMARY));
    engine.apply(fullscreen_off());
    assert!(is_overlapped(&engine, PRIMARY));
    assert!(!is_fullscreen(&engine, PRIMARY));
    settings.icon_size = 64;
    settings.behavior.show_only_current_desktop = false;
    assert!(!engine.apply(EngineInput::SettingsChanged(settings)).changed);
    assert!(is_overlapped(&engine, PRIMARY));
    assert_eq!(engine.snapshot().items, state.items);
}
