//! The facts of each dock: fullscreen, overlapped, and two dock windows.

use pretty_assertions::assert_eq;

use super::*;

// ----- the platform's fullscreen verdict, per dock -------------------------

/// The facts of the dock on `monitor`, if the snapshot has an entry.
fn facts_on(state: &DockState, monitor: &str) -> Option<stem_core::DockFacts> {
    state.docks.iter().find(|d| d.monitor.0 == monitor).cloned()
}

fn fullscreen_on(state: &DockState, monitor: &str) -> bool {
    facts_on(state, monitor).is_some_and(|d| d.fullscreen)
}

fn overlapped_on(state: &DockState, monitor: &str) -> bool {
    facts_on(state, monitor).is_some_and(|d| d.overlapped)
}

#[test]
fn fullscreen_verdicts_reach_the_snapshot_and_repeats_emit_nothing() {
    let relay = Arc::new(Relay::demo());
    let (service, rx) = start_relay(relay.clone());
    wait_state(&rx, resolved);
    // The app layer registers the primary dock window on the demo
    // monitor (no panel rect yet): one entry, both facts clear.
    service.set_dock_panel("dock", Some(demo_monitor_id()), None);
    let state = wait_state(&rx, |s| !s.docks.is_empty());
    assert_eq!(
        state.docks,
        vec![stem_core::DockFacts {
            monitor: demo_monitor_id(),
            fullscreen: false,
            overlapped: false,
        }]
    );
    while rx.try_recv().is_ok() {}

    // The first verdict after the start snapshot is usually `false`:
    // nothing changes, nothing is emitted.
    relay.emit(PlatformEvent::FullscreenChanged {
        active: false,
        monitor: None,
    });
    assert!(rx.recv_timeout(Duration::from_millis(150)).is_err());

    // A verdict naming the dock's monitor.
    relay.emit(PlatformEvent::FullscreenChanged {
        active: true,
        monitor: Some(demo_monitor_id()),
    });
    let state = wait_state(&rx, |s| fullscreen_on(s, DEMO_MONITOR));
    // The items are untouched by it.
    assert_eq!(app_items(&state).len(), 5);
    assert!(fullscreen_on(&service.state(), DEMO_MONITOR));

    relay.emit(PlatformEvent::FullscreenChanged {
        active: true,
        monitor: Some(demo_monitor_id()),
    });
    assert!(rx.recv_timeout(Duration::from_millis(150)).is_err());

    relay.emit(PlatformEvent::FullscreenChanged {
        active: false,
        monitor: None,
    });
    wait_state(&rx, |s| !fullscreen_on(s, DEMO_MONITOR));

    // A verdict with no monitor lands on the primary monitor's dock; one
    // for another monitor hides nobody.
    relay.emit(PlatformEvent::FullscreenChanged {
        active: true,
        monitor: None,
    });
    wait_state(&rx, |s| fullscreen_on(s, DEMO_MONITOR));
    relay.emit(PlatformEvent::FullscreenChanged {
        active: true,
        monitor: Some(MonitorId(r"\\.\DISPLAY3".into())),
    });
    wait_state(&rx, |s| !fullscreen_on(s, DEMO_MONITOR));
    assert!(facts_on(&service.state(), DEMO_MONITOR).is_some());
    service.shutdown();
}

// ----- a window over the dock --------------------------------------------

#[test]
fn the_dock_panel_and_window_moves_drive_overlapped_and_only_flips_emit() {
    let relay = Arc::new(Relay::demo());
    let (service, rx) = start_relay(relay.clone());
    wait_state(&rx, resolved);
    while rx.try_recv().is_ok() {}
    assert!(service.state().docks.is_empty());

    // The UI's panel rests at the bottom of the demo monitor's work
    // area, below every demo window: the dock's entry appears (one
    // state) with nothing overlapping it.
    let panel = Rect::new(1000, 1320, 560, 72);
    service.set_dock_panel("dock", Some(demo_monitor_id()), Some(panel));
    let state = wait_state(&rx, |s| facts_on(s, DEMO_MONITOR).is_some());
    assert!(!overlapped_on(&state, DEMO_MONITOR));
    assert!(rx.recv_timeout(Duration::from_millis(150)).is_err());

    // Code's window is dragged down over the dock: the platform reports
    // a rect-only update, the flag flips, one state follows.
    relay
        .mock
        .move_window(WindowId(4), Some(Rect::new(600, 900, 700, 480)));
    let state = wait_state(&rx, |s| overlapped_on(s, DEMO_MONITOR));
    assert_eq!(app_items(&state).len(), 5);
    assert!(overlapped_on(&service.state(), DEMO_MONITOR));
    // Another nudge that keeps it over the dock: nothing to emit.
    relay
        .mock
        .move_window(WindowId(4), Some(Rect::new(620, 910, 700, 480)));
    assert!(rx.recv_timeout(Duration::from_millis(150)).is_err());

    // Dragged away again: cleared.
    relay
        .mock
        .move_window(WindowId(4), Some(Rect::new(260, 40, 1600, 1000)));
    wait_state(&rx, |s| !overlapped_on(s, DEMO_MONITOR));

    // A minimised window over the dock does not count; restoring it does.
    relay
        .mock
        .move_window(WindowId(6), Some(Rect::new(600, 900, 700, 480)));
    assert!(rx.recv_timeout(Duration::from_millis(150)).is_err());
    assert!(!overlapped_on(&service.state(), DEMO_MONITOR));
    service
        .window_action(WindowId(6), WindowAction::Restore)
        .unwrap();
    wait_state(&rx, |s| overlapped_on(s, DEMO_MONITOR));

    // The dock window was hidden (the tray): no panel, no overlap, the
    // entry stays; shown again: back.
    service.set_dock_panel("dock", Some(demo_monitor_id()), None);
    let state = wait_state(&rx, |s| !overlapped_on(s, DEMO_MONITOR));
    assert!(facts_on(&state, DEMO_MONITOR).is_some());
    service.set_dock_panel("dock", Some(demo_monitor_id()), Some(panel));
    wait_state(&rx, |s| overlapped_on(s, DEMO_MONITOR));
    // A window the platform cannot read the frame of says nothing.
    relay.mock.move_window(WindowId(6), None);
    wait_state(&rx, |s| !overlapped_on(s, DEMO_MONITOR));

    // The window is gone (its monitor unplugged): the entry goes with it.
    service.set_dock_panel("dock", None, None);
    wait_state(&rx, |s| s.docks.is_empty());
    service.shutdown();
}

#[test]
fn our_own_process_windows_never_overlap_the_dock() {
    let relay = Arc::new(Relay::demo());
    let (service, rx) = start_relay(relay.clone());
    wait_state(&rx, resolved);
    service.set_dock_panel(
        "dock",
        Some(demo_monitor_id()),
        Some(Rect::new(1000, 1320, 560, 72)),
    );
    wait_state(&rx, |s| facts_on(s, DEMO_MONITOR).is_some());
    while rx.try_recv().is_ok() {}

    // A window of this very process (the settings window) over the dock.
    let mut ours = relay.window(WindowId(4));
    ours.id = WindowId(70);
    ours.process_id = std::process::id();
    ours.title = "Stem Settings".into();
    ours.rect = Some(Rect::new(0, 0, 2560, 1392));
    relay.emit(PlatformEvent::WindowAdded(ours.clone()));
    let state = wait_state(&rx, |s| has_window_titled(s, "Stem Settings"));
    assert!(!overlapped_on(&state, DEMO_MONITOR));
    // The same frame on another process's window counts.
    ours.id = WindowId(71);
    ours.process_id = 7171;
    ours.title = "Theirs".into();
    relay.emit(PlatformEvent::WindowAdded(ours));
    wait_state(&rx, |s| overlapped_on(s, DEMO_MONITOR));
    service.shutdown();
}

// ----- two dock windows -------------------------------------------------------

#[test]
fn two_docks_keep_their_own_facts() {
    let relay = Arc::new(Relay::demo());
    let (service, rx) = start_relay(relay.clone());
    wait_state(&rx, resolved);
    let second = MonitorId(r"\\.\DISPLAY3".into());
    service.set_dock_panel(
        "dock",
        Some(demo_monitor_id()),
        Some(Rect::new(1000, 1320, 560, 72)),
    );
    service.set_dock_panel(
        "dock-DISPLAY3",
        Some(second.clone()),
        Some(Rect::new(3560, 1000, 560, 72)),
    );
    let state = wait_state(&rx, |s| s.docks.len() == 2);
    assert!(state.docks.iter().all(|d| !d.fullscreen && !d.overlapped));
    while rx.try_recv().is_ok() {}

    // A window dragged under the second dock: only it is overlapped.
    relay
        .mock
        .move_window(WindowId(4), Some(Rect::new(3200, 800, 700, 480)));
    let state = wait_state(&rx, |s| overlapped_on(s, r"\\.\DISPLAY3"));
    assert!(!overlapped_on(&state, DEMO_MONITOR));

    // A video goes fullscreen on the second monitor: only its dock.
    relay.emit(PlatformEvent::FullscreenChanged {
        active: true,
        monitor: Some(second),
    });
    let state = wait_state(&rx, |s| fullscreen_on(s, r"\\.\DISPLAY3"));
    assert!(!fullscreen_on(&state, DEMO_MONITOR));
    assert!(overlapped_on(&state, r"\\.\DISPLAY3"));
    service.shutdown();
}

#[test]
fn clicks_and_menus_of_a_dock_act_on_the_windows_on_its_monitor() {
    let (service, platform, rx) = start();
    let state = wait_state(&rx, resolved);
    let firefox = item_titled(&state, "firefox").unwrap().clone();
    let firefox_id = firefox.app_id.clone().unwrap();
    let second_dock = "dock-DISPLAY3";
    let second = MonitorId(r"\\.\DISPLAY3".into());

    // Firefox's second window (3) is dragged to the second monitor; its
    // first (2), in the foreground, stays on the primary. One dock per
    // monitor, each listing only the windows on its own.
    let mut moved = platform
        .windows()
        .unwrap()
        .into_iter()
        .find(|w| w.id == WindowId(3))
        .unwrap();
    moved.monitor = Some(second.clone());
    platform.push(PlatformEvent::WindowUpdated(moved));
    let mut settings = service.settings();
    settings.monitors = stem_core::settings::MonitorMode::All;
    settings.behavior.show_only_current_monitor = true;
    service.update_settings(settings.clone()).unwrap();
    service.set_dock_panel(DOCK, Some(demo_monitor_id()), None);
    service.set_dock_panel(second_dock, Some(second), None);
    // The worker applies its queue in order: once both docks are in a
    // snapshot, so are the move and the settings.
    wait_state(&rx, |s| s.docks.len() == 2);
    assert_eq!(service.windows_on_dock(&firefox_id, DOCK), [WindowId(2)]);
    assert_eq!(
        service.windows_on_dock(&firefox_id, second_dock),
        [WindowId(3)]
    );

    // The primary's dock shows only the active window 2: minimise it
    // (every window would cycle to 3). The second dock shows only 3,
    // which is not active: activate it.
    platform.clear_calls();
    service.primary_click(&firefox.key, DOCK).unwrap();
    service.primary_click(&firefox.key, second_dock).unwrap();
    assert_eq!(
        platform.calls(),
        vec![
            MockCall::WindowAction(WindowId(2), WindowAction::Minimize),
            MockCall::WindowAction(WindowId(3), WindowAction::Activate),
        ]
    );

    // The second dock's menu lists window 3 alone; the primary's, 2.
    let listed = |dock: &str| -> Vec<WindowId> {
        let item = service.dock_item(&firefox.key, dock).unwrap();
        item.windows.iter().map(|w| w.id).collect()
    };
    assert_eq!(listed(second_dock), [WindowId(3)]);
    assert_eq!(listed(DOCK), [WindowId(2)]);
    assert_eq!(service.dock_item("bogus", second_dock), None);

    // The setting off: both docks list, click and close every window.
    settings.behavior.show_only_current_monitor = false;
    service.update_settings(settings).unwrap();
    wait_until("the setting to reach the engine", || {
        service.windows_on_dock(&firefox_id, second_dock).len() == 2
    });
    assert_eq!(
        service.windows_on_dock(&firefox_id, DOCK),
        [WindowId(2), WindowId(3)]
    );
    assert_eq!(listed(second_dock), [WindowId(2), WindowId(3)]);
    service.shutdown();
}
