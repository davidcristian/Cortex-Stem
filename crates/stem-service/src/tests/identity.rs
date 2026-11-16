//! The identity thread: retries, the fallback, and apps that change or
//! vanish before their identity is resolved.

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn identity_resolution_retries_once_after_a_failure() {
    let relay = Arc::new(Relay::demo().flaky(WindowId(2), 1));
    let started = Instant::now();
    let (service, rx) = start_relay(relay.clone());

    // Everyone else resolves at once; Firefox keeps its window title.
    let state = wait_state(&rx, |s| {
        app_items(s).iter().filter(|i| i.icon_key.is_some()).count() == 4
    });
    let firefox = item_titled(&state, "Stem — Mozilla Firefox").expect("unresolved firefox");
    assert!(firefox.icon_key.is_none());

    let state = wait_state(&rx, resolved);
    let waited = started.elapsed();
    assert!(
        waited >= IDENTITY_RETRY_DELAY - Duration::from_millis(100),
        "retry came too early: {waited:?}"
    );
    let firefox = item_titled(&state, "firefox").unwrap();
    assert!(firefox.icon_key.is_some());
    let calls = relay.identify_calls_for(WindowId(2));
    assert_eq!(calls.len(), 2, "exactly one retry");
    assert!(calls[1].duration_since(calls[0]) >= IDENTITY_RETRY_DELAY - Duration::from_millis(100));
    service.shutdown();
}

#[test]
fn identity_resolution_falls_back_after_the_retry_fails() {
    let relay = Arc::new(Relay::demo().flaky(WindowId(2), 2));
    let (service, rx) = start_relay(relay.clone());

    // The fallback name comes from the app id, without an icon.
    let state = wait_state(&rx, |s| {
        item_titled(s, "firefox").is_some_and(|i| i.icon_key.is_none())
    });
    assert_eq!(item_titled(&state, "firefox").unwrap().windows.len(), 2);
    assert_eq!(relay.identify_calls_for(WindowId(2)).len(), 2);

    // The engine stopped asking: no third attempt.
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(relay.identify_calls_for(WindowId(2)).len(), 2);
    service.shutdown();
}

#[test]
fn an_identity_for_another_id_is_never_stored_under_the_engines_id() {
    // Firefox's first window now reads as another app (its facts
    // changed after the engine grouped it): the platform's answer names
    // that app, with its name, icon and Store launch.
    let other = AppIdentity {
        id: AppId::for_app_user_model_id("Contoso.Other_p!App"),
        display_name: "Other".into(),
        icon_key: Some(IconKey("aumid:Contoso.Other_p!App".into())),
        launch: Some(LaunchTarget::StoreApp {
            app_user_model_id: "Contoso.Other_p!App".into(),
        }),
    };
    let relay = Arc::new(Relay::demo().answering_window(WindowId(2), other));
    let (service, rx) = start_relay(relay.clone());

    // Asked twice (the retry), then the fallback: the app id's name, no
    // icon, no launch target of the other app.
    let state = wait_state(&rx, |s| {
        item_titled(s, "firefox").is_some_and(|i| i.icon_key.is_none())
    });
    let firefox = item_titled(&state, "firefox").unwrap();
    assert_eq!(firefox.windows.len(), 2);
    assert_eq!(relay.identify_calls_for(WindowId(2)).len(), 2);
    assert!(item_titled(&state, "Other").is_none());
    let app_id = firefox.app_id.clone().unwrap();
    let identity = service.inner.engine.lock().identity(&app_id).cloned();
    let identity = identity.expect("the fallback is stored");
    assert_eq!(identity.display_name, "firefox");
    assert_eq!(identity.icon_key, None);
    assert_eq!(identity.launch, None);
    // So the item cannot be pinned as the other app.
    assert!(matches!(
        service.pin_app(&app_id),
        Err(ServiceError::NotLaunchable(_))
    ));
    service.shutdown();
}

#[test]
fn an_app_that_vanished_before_resolution_is_asked_again_when_it_returns() {
    let relay = Arc::new(Relay::demo().slow(Duration::from_millis(100)));
    let (service, rx) = start_relay(relay.clone());

    // While the resolver is busy with the demo apps, an app appears and
    // disappears again; its request is queued behind the others. The
    // events go through the mock so that it can identify the window
    // later (`Relay::emit` would leave the mock unaware of it).
    let mut fleeting = relay.window(WindowId(4));
    fleeting.id = WindowId(7);
    fleeting.process_id = 7007;
    fleeting.title = "Fleeting".into();
    fleeting.exe_path = Some(PathBuf::from(r"C:\Apps\fleeting.exe"));
    relay
        .mock
        .push(PlatformEvent::WindowAdded(fleeting.clone()));
    relay.mock.push(PlatformEvent::WindowRemoved(WindowId(7)));
    wait_state(&rx, resolved);
    // By now the resolver has reached the stale request: nothing to ask,
    // nothing stored.
    std::thread::sleep(Duration::from_millis(300));
    assert!(relay.identify_calls_for(WindowId(7)).is_empty());
    assert!(!has_window_titled(&service.state(), "Fleeting"));

    // The app returns: its identity is requested and resolved for real
    // instead of being stuck with a fallback made while it was gone.
    relay.mock.push(PlatformEvent::WindowAdded(fleeting));
    let state = wait_state(&rx, |s| {
        item_titled(s, "fleeting").is_some_and(|i| i.icon_key.is_some())
    });
    assert!(has_window_titled(&state, "Fleeting"));
    assert_eq!(relay.identify_calls_for(WindowId(7)).len(), 1);
    service.shutdown();
}

#[test]
fn slow_identity_resolution_does_not_delay_event_processing() {
    let relay = Arc::new(Relay::demo().slow(Duration::from_millis(300)));
    let (service, rx) = start_relay(relay.clone());
    // The windows show up before any identity is known.
    wait_state(&rx, |s| app_items(s).len() == 5);

    let mut w = relay.window(WindowId(4));
    w.title = "prompt".into();
    let sent = Instant::now();
    relay.emit(PlatformEvent::WindowUpdated(w));
    wait_state(&rx, |s| has_window_titled(s, "prompt"));
    let latency = sent.elapsed();
    assert!(
        latency < Duration::from_millis(200),
        "event took {latency:?} while identities were being resolved"
    );

    // Resolution still completes (5 apps x 300 ms, on its own thread).
    wait_state(&rx, resolved);
    service.shutdown();
}
