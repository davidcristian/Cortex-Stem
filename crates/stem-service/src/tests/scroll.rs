//! Scrolling over an item.

use pretty_assertions::assert_eq;

use super::*;

/// `ms` milliseconds after `start`.
fn after(start: Instant, ms: u64) -> Instant {
    start + Duration::from_millis(ms)
}

/// Set `behavior.scrollAction` to `cycleWindows` and wait until the
/// worker reports the settings, when the engine has them.
fn cycle_windows(service: &DockService, rx: &Receiver<ServiceEvent>) {
    let mut settings = service.settings();
    settings.behavior.scroll_action = stem_core::settings::ScrollAction::CycleWindows;
    service.update_settings(settings).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match rx.recv_deadline(deadline) {
            Ok(ServiceEvent::Settings(s))
                if s.behavior.scroll_action == stem_core::settings::ScrollAction::CycleWindows =>
            {
                break;
            }
            Ok(_) => {}
            Err(e) => panic!("no settings event: {e}"),
        }
    }
}

#[test]
fn the_scroll_gate_lets_one_activation_per_dock_window_through_every_150_ms() {
    assert_eq!(SCROLL_INTERVAL, Duration::from_millis(150));
    let t0 = Instant::now();
    let mut gate = ScrollGate::new();
    assert!(gate.admit(DOCK, t0));
    assert!(!gate.admit(DOCK, after(t0, 1)));
    assert!(!gate.admit(DOCK, after(t0, 149)));
    // Another dock window has a gate of its own.
    assert!(gate.admit("dock-DISPLAY3", after(t0, 100)));
    assert!(!gate.admit("dock-DISPLAY3", after(t0, 140)));
    // The dropped scrolls did not move the gate: 150 ms after the one
    // admitted, not after the last one dropped.
    assert!(gate.admit(DOCK, after(t0, 150)));
    assert!(!gate.admit("dock-DISPLAY3", after(t0, 249)));
    assert!(gate.admit("dock-DISPLAY3", after(t0, 250)));
    // A time before the last admission (two threads racing) is too soon.
    assert!(!gate.admit(DOCK, t0));
    assert!(gate.admit(DOCK, after(t0, 300)));
}

#[test]
fn a_fast_spin_moves_one_window_per_interval_not_one_per_notch() {
    // A notch every 16 ms (a free-spinning wheel) for about a second.
    let t0 = Instant::now();
    let mut gate = ScrollGate::new();
    let admitted: Vec<u64> = (0..=62u64)
        .map(|n| n * 16)
        .filter(|&ms| gate.admit(DOCK, after(t0, ms)))
        .collect();
    assert_eq!(admitted, [0, 160, 320, 480, 640, 800, 960]);
}

#[test]
fn the_scroll_gate_forgets_dock_windows_whose_interval_has_run_out() {
    let t0 = Instant::now();
    let mut gate = ScrollGate::new();
    for n in 0..10 {
        assert!(gate.admit(&format!("dock-{n}"), t0));
    }
    assert_eq!(gate.last.len(), 10);
    assert!(gate.admit(DOCK, after(t0, 150)));
    assert_eq!(gate.last.keys().collect::<Vec<_>>(), [DOCK]);
}

#[test]
fn a_scroll_activates_the_next_or_previous_window_at_most_once_per_interval() {
    use ScrollDirection::{Next, Previous};
    let (service, platform, rx) = start();
    let state = wait_state(&rx, resolved);
    let firefox = item_titled(&state, "firefox").unwrap().key.clone();
    let code = item_titled(&state, "Code").unwrap().key.clone();
    let separator = state
        .items
        .iter()
        .find(|i| i.kind == DockItemKind::Separator)
        .unwrap()
        .key
        .clone();
    let second_dock = "dock-DISPLAY3";
    let t0 = Instant::now();

    // `none`, the default: nothing happens, but an unknown key rejects.
    assert_eq!(
        service.settings().behavior.scroll_action,
        stem_core::settings::ScrollAction::None
    );
    assert_eq!(
        service.scroll_item_at(&firefox, DOCK, Next, t0).unwrap(),
        None
    );
    assert!(matches!(
        service.scroll_item_at("bogus", DOCK, Next, t0),
        Err(ServiceError::UnknownItem(_))
    ));
    assert_eq!(platform.calls(), vec![]);

    cycle_windows(&service, &rx);

    // Firefox: window 2 in front, 3 minimised. Next restores 3.
    let at = |ms| after(t0, ms);
    assert_eq!(
        service.scroll_item_at(&firefox, DOCK, Next, at(0)).unwrap(),
        Some(WindowId(3))
    );
    wait_state(&rx, |s| s.foreground == Some(WindowId(3)));
    // Too soon on the same dock window, whatever the item: dropped.
    assert_eq!(
        service.scroll_item_at(&code, DOCK, Next, at(100)).unwrap(),
        None
    );
    // Another dock window has its own gate: next from 3 wraps to 2.
    assert_eq!(
        service
            .scroll_item_at(&firefox, second_dock, Next, at(100))
            .unwrap(),
        Some(WindowId(2))
    );
    wait_state(&rx, |s| s.foreground == Some(WindowId(2)));
    // 150 ms after its last one: previous from 2 wraps to 3.
    assert_eq!(
        service
            .scroll_item_at(&firefox, DOCK, Previous, at(150))
            .unwrap(),
        Some(WindowId(3))
    );
    wait_state(&rx, |s| s.foreground == Some(WindowId(3)));
    // Fixed items and separators do nothing, and do not arm the gate.
    assert_eq!(
        service
            .scroll_item_at("fixed:launcher", DOCK, Next, at(300))
            .unwrap(),
        None
    );
    assert_eq!(
        service
            .scroll_item_at(&separator, DOCK, Next, at(300))
            .unwrap(),
        None
    );
    // Code's one window is not in front: either way activates it.
    assert_eq!(
        service
            .scroll_item_at(&code, DOCK, Previous, at(300))
            .unwrap(),
        Some(WindowId(4))
    );
    assert!(matches!(
        service.scroll_item_at("bogus", DOCK, Next, at(600)),
        Err(ServiceError::UnknownItem(_))
    ));
    assert_eq!(
        platform.calls(),
        vec![
            MockCall::WindowAction(WindowId(3), WindowAction::Activate),
            MockCall::WindowAction(WindowId(2), WindowAction::Activate),
            MockCall::WindowAction(WindowId(3), WindowAction::Activate),
            MockCall::WindowAction(WindowId(4), WindowAction::Activate),
        ]
    );
    service.shutdown();
}

#[test]
fn a_scroll_goes_on_past_a_window_windows_refuses_to_bring_forward() {
    use ScrollDirection::{Next, Previous};
    let relay = Arc::new(Relay::demo().refusing(WindowId(3)));
    let (service, rx) = start_relay(relay.clone());
    wait_state(&rx, resolved);
    // A third Firefox window, seen last: Firefox cycles 2, 3 and 8.
    let mut third = relay.window(WindowId(2));
    third.id = WindowId(8);
    third.title = "Third".into();
    relay.mock.push(PlatformEvent::WindowAdded(third));
    let state = wait_state(&rx, |s| {
        s.foreground == Some(WindowId(2))
            && item_titled(s, "firefox").is_some_and(|i| i.windows.len() == 3)
    });
    let firefox = item_titled(&state, "firefox").unwrap();
    let order: Vec<WindowId> = firefox.windows.iter().map(|w| w.id).collect();
    assert_eq!(order, [WindowId(2), WindowId(3), WindowId(8)]);
    let firefox = firefox.key.clone();
    cycle_windows(&service, &rx);
    relay.mock.clear_calls();
    let t0 = Instant::now();
    let at = |ms| after(t0, ms);

    // 2 in front: next is 3, which Windows refuses (an elevated window,
    // say); the page hears the error and the foreground stays on 2.
    assert!(matches!(
        service.scroll_item_at(&firefox, DOCK, Next, at(0)),
        Err(ServiceError::Platform(_))
    ));
    assert_eq!(service.state().foreground, Some(WindowId(2)));
    // The next notch goes on to 8 rather than trying 3 again.
    assert_eq!(
        service
            .scroll_item_at(&firefox, DOCK, Next, at(150))
            .unwrap(),
        Some(WindowId(8))
    );
    wait_state(&rx, |s| s.foreground == Some(WindowId(8)));
    // Back from 8: 3 is refused again, and the notch after it goes on
    // to 2.
    assert!(matches!(
        service.scroll_item_at(&firefox, DOCK, Previous, at(300)),
        Err(ServiceError::Platform(_))
    ));
    assert_eq!(
        service
            .scroll_item_at(&firefox, DOCK, Previous, at(450))
            .unwrap(),
        Some(WindowId(2))
    );
    wait_state(&rx, |s| s.foreground == Some(WindowId(2)));
    let activations: Vec<MockCall> = relay
        .mock
        .calls()
        .into_iter()
        .filter(|c| matches!(c, MockCall::WindowAction(..)))
        .collect();
    assert_eq!(
        activations,
        vec![
            MockCall::WindowAction(WindowId(8), WindowAction::Activate),
            MockCall::WindowAction(WindowId(2), WindowAction::Activate),
        ]
    );

    // 2 in front, 3 refused once more. A notch that comes only after
    // the scroll memory has run out steps from the window in front again
    // and tries 3 anew, instead of passing it over for good.
    assert!(matches!(
        service.scroll_item_at(&firefox, DOCK, Next, at(600)),
        Err(ServiceError::Platform(_))
    ));
    let later = 600 + u64::try_from(stem_core::engine::SCROLL_MEMORY.as_millis()).unwrap();
    assert!(matches!(
        service.scroll_item_at(&firefox, DOCK, Next, at(later)),
        Err(ServiceError::Platform(_))
    ));
    assert_eq!(service.state().foreground, Some(WindowId(2)));
    service.shutdown();
}

#[test]
fn a_scroll_on_a_dock_per_monitor_cycles_only_the_windows_on_its_monitor() {
    let (service, platform, rx) = start();
    let state = wait_state(&rx, resolved);
    let firefox = item_titled(&state, "firefox").unwrap().clone();
    let second_dock = "dock-DISPLAY3";
    let second = MonitorId(r"\\.\DISPLAY3".into());

    // Firefox's window 3 goes to the second monitor; 2, in front, stays.
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
    settings.behavior.scroll_action = stem_core::settings::ScrollAction::CycleWindows;
    service.update_settings(settings).unwrap();
    service.set_dock_panel(DOCK, Some(demo_monitor_id()), None);
    service.set_dock_panel(second_dock, Some(second), None);
    wait_state(&rx, |s| s.docks.len() == 2);

    // The primary's dock lists 2 alone, in front: activated again. The
    // second dock lists 3 alone: activated, not 2.
    let t0 = Instant::now();
    platform.clear_calls();
    assert_eq!(
        service
            .scroll_item_at(&firefox.key, DOCK, ScrollDirection::Next, t0)
            .unwrap(),
        Some(WindowId(2))
    );
    assert_eq!(
        service
            .scroll_item_at(&firefox.key, second_dock, ScrollDirection::Previous, t0)
            .unwrap(),
        Some(WindowId(3))
    );
    assert_eq!(
        platform.calls(),
        vec![
            MockCall::WindowAction(WindowId(2), WindowAction::Activate),
            MockCall::WindowAction(WindowId(3), WindowAction::Activate),
        ]
    );
    service.shutdown();
}
