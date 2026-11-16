//! The worker thread: coalescing, shutdown, a panicking listener, theme and
//! monitor changes.

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn event_storm_is_coalesced_into_few_state_notifications() {
    let relay = Arc::new(Relay::demo());
    let (service, rx) = start_relay(relay.clone());
    // Stamp every State on the worker thread itself, so spacing is
    // measured where the rate limit applies, not where the test dequeues.
    let stamps: Arc<Mutex<Vec<Instant>>> = Arc::default();
    let stamper = Arc::clone(&stamps);
    service.subscribe(Box::new(move |ev| {
        if matches!(ev, ServiceEvent::State(_)) {
            stamper.lock().push(Instant::now());
        }
    }));
    wait_state(&rx, resolved);
    while rx.try_recv().is_ok() {}

    let code = relay.window(WindowId(4));
    let started = Instant::now();
    for i in 0..1000 {
        let mut w = code.clone();
        w.title = format!("title {i}");
        relay.emit(PlatformEvent::WindowUpdated(w));
    }
    let mut states = 0u128;
    let deadline = started + Duration::from_secs(10);
    loop {
        match rx.recv_deadline(deadline) {
            Ok(ServiceEvent::State(s)) => {
                states += 1;
                if has_window_titled(&s, "title 999") {
                    break;
                }
            }
            Ok(_) => {}
            Err(_) => panic!("final title not seen after {states} state notifications"),
        }
    }
    let elapsed = started.elapsed();
    assert!(states >= 1);
    assert!(
        states <= 100,
        "1000 events produced {states} state notifications"
    );
    // Hard bound from the rate limit: one per interval, plus slack for
    // the first emit and the one already due when the storm began.
    let max = elapsed.as_millis() / STATE_EMIT_INTERVAL.as_millis() + 2;
    assert!(
        states <= max,
        "{states} state notifications in {elapsed:?} (max {max})"
    );
    // Copy out under a short-lived guard: the worker may still be inside
    // `notify` for the last event (the recorder woke us before the
    // stamper ran), and `shutdown` below joins it.
    let stamps: Vec<Instant> = stamps.lock().clone();
    for pair in stamps.windows(2) {
        let gap = pair[1].duration_since(pair[0]);
        assert!(
            gap >= STATE_EMIT_INTERVAL - Duration::from_millis(1),
            "two state notifications only {gap:?} apart"
        );
    }
    service.shutdown();
}

#[test]
fn shutdown_flushes_queued_inputs_and_joins_both_threads() {
    let relay = Arc::new(Relay::demo());
    let (service, rx) = start_relay(relay.clone());
    wait_state(&rx, resolved);

    let code = relay.window(WindowId(4));
    for i in 0..200 {
        let mut w = code.clone();
        w.title = format!("title {i}");
        relay.emit(PlatformEvent::WindowUpdated(w));
    }
    let started = Instant::now();
    service.shutdown();
    let took = started.elapsed();
    assert!(service.worker.lock().is_none());
    assert!(service.resolver.lock().is_none());
    assert!(took < Duration::from_secs(2), "shutdown took {took:?}");

    // Everything queued before the shutdown made it into the last state.
    let last = rx
        .try_iter()
        .filter_map(|ev| match ev {
            ServiceEvent::State(s) => Some(s),
            _ => None,
        })
        .last()
        .expect("a final state");
    assert!(has_window_titled(&last, "title 199"));

    // Idempotent, and the platform was stopped: nothing gets through.
    service.shutdown();
    relay.emit(PlatformEvent::WindowUpdated(code));
    assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
}

#[test]
fn a_panicking_listener_does_not_kill_the_worker() {
    let relay = Arc::new(Relay::demo());
    let service =
        DockService::start(relay.clone(), Box::new(MemorySettingsStore::default())).unwrap();
    let panics = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&panics);
    service.subscribe(Box::new(move |ev| {
        if matches!(ev, ServiceEvent::State(_)) {
            counter.fetch_add(1, Ordering::SeqCst);
            panic!("listener failure under test");
        }
    }));
    // Subscribed after the faulty one: must still be called.
    let rx = record(&service);
    wait_state(&rx, resolved);

    let mut w = relay.window(WindowId(4));
    w.title = "still alive".into();
    relay.emit(PlatformEvent::WindowUpdated(w));
    wait_state(&rx, |s| has_window_titled(s, "still alive"));
    assert!(panics.load(Ordering::SeqCst) >= 2);

    // Commands and the final flush work as usual.
    service.primary_click("fixed:launcher", DOCK).unwrap();
    service.shutdown();
    assert!(service.worker.lock().is_none());
}

#[test]
fn theme_and_monitor_changes_notify_without_a_state_emit() {
    let relay = Arc::new(Relay::demo());
    let (service, rx) = start_relay(relay.clone());
    wait_state(&rx, resolved);
    while rx.try_recv().is_ok() {}

    let theme = ThemeInfo {
        accent: "#FF8800".into(),
        dark: false,
        high_contrast: false,
        transparency: true,
    };
    let monitors = vec![MonitorInfo {
        id: stem_core::MonitorId("second".into()),
        name: "Second".into(),
        bounds: Rect::new(2560, 0, 1920, 1080),
        work_area: Rect::new(2560, 0, 1920, 1032),
        scale_factor: 1.0,
        is_primary: false,
    }];
    relay.emit(PlatformEvent::ThemeChanged(theme.clone()));
    relay.emit(PlatformEvent::MonitorsChanged(monitors.clone()));

    let mut seen = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(2);
    while seen.len() < 2 {
        match rx.recv_deadline(deadline) {
            Ok(ev) => seen.push(ev),
            Err(_) => panic!("expected theme and monitors events, got {seen:?}"),
        }
    }
    assert_eq!(
        seen,
        vec![
            ServiceEvent::Theme(theme.clone()),
            ServiceEvent::Monitors(monitors.clone())
        ]
    );
    assert_eq!(service.theme(), theme);
    assert_eq!(service.monitors(), monitors);
    // No State followed: the snapshot did not change.
    assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    service.shutdown();
}
