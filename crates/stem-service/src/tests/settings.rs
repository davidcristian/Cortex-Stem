//! Settings changes: serialised, composed in order, and never undone by an
//! older document.

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn back_to_back_pins_and_a_reorder_never_lose_a_change() {
    let platform = Arc::new(MockPlatform::with_demo_scenario());
    let store = SharedStore::default();
    let document = Arc::clone(&store.0);
    let service = DockService::start(platform, Box::new(store)).unwrap();
    let rx = record(&service);
    wait_state(&rx, resolved);

    // Two pins in a row, before the engine has applied the first one.
    let a = service.pin_target(exe_target(r"C:\Apps\a.exe")).unwrap();
    let b = service.pin_target(exe_target(r"C:\Apps\b.exe")).unwrap();
    assert_eq!(a, AppId::for_exe(Path::new(r"C:\Apps\a.exe")));
    assert_eq!(pinned_ids(&service.settings()), vec![a.clone(), b.clone()]);

    // A reorder right away keeps both pins and applies the new order.
    service.reorder_pinned(&[b.clone(), a.clone()]).unwrap();
    assert_eq!(pinned_ids(&service.settings()), vec![b.clone(), a.clone()]);
    // Pinning again is a no-op that changes nothing.
    assert_eq!(service.pin_target(exe_target(r"C:\Apps\a.exe")).unwrap(), a);
    assert_eq!(pinned_ids(&service.settings()), vec![b.clone(), a.clone()]);

    // The store holds exactly the authoritative document.
    let saved = Settings::from_json(&document.document().expect("a saved document")).unwrap();
    assert_eq!(saved, service.settings());

    // The engine catches up: both items, in that order, with the names
    // `pin_target` resolved (sent ahead of the settings).
    let state = wait_state(&rx, |s| {
        let pinned: Vec<&str> = app_items(s)
            .iter()
            .filter(|i| i.pinned)
            .map(|i| i.key.as_str())
            .collect();
        pinned == ["app:exe:c:/apps/b.exe", "app:exe:c:/apps/a.exe"]
    });
    assert_eq!(app_items(&state)[0].title, "b");
    assert_eq!(app_items(&state)[1].title, "a");

    // Unpinning one, then the other, straight after each other.
    service.unpin_app(&a).unwrap();
    service.unpin_app(&b).unwrap();
    assert!(service.settings().pinned.is_empty());
    assert!(matches!(
        service.unpin_app(&b),
        Err(ServiceError::NotPinned(_))
    ));
    wait_state(&rx, |s| app_items(s).iter().all(|i| !i.pinned));
    let saved = Settings::from_json(&document.document().unwrap()).unwrap();
    assert!(saved.pinned.is_empty());
    service.shutdown();
}

#[test]
fn update_settings_and_pins_compose_in_order() {
    let (service, _platform, rx) = start();
    let state = wait_state(&rx, resolved);
    let spotify = item_titled(&state, "Spotify")
        .unwrap()
        .app_id
        .clone()
        .unwrap();

    let mut s = service.settings();
    s.icon_size = 64;
    service.update_settings(s).unwrap();
    service.pin_app(&spotify).unwrap();
    let mut s = service.settings();
    s.icon_size = 72;
    let saved = service.update_settings(s).unwrap();
    // The pin made between the two updates survives both.
    assert_eq!(saved.icon_size, 72);
    assert!(saved.is_pinned(&spotify));
    assert_eq!(saved, service.settings());
    service.shutdown();
}

#[test]
fn a_settings_document_older_than_a_pin_or_a_hide_does_not_undo_them() {
    let platform = Arc::new(MockPlatform::with_demo_scenario());
    let store = SharedStore::default();
    let document = Arc::clone(&store.0);
    let service = DockService::start(platform, Box::new(store)).unwrap();
    let rx = record(&service);
    let state = wait_state(&rx, resolved);
    let app = |title: &str| item_titled(&state, title).unwrap().app_id.clone().unwrap();
    let (spotify, code) = (app("Spotify"), app("Code"));
    let pinned_before = pinned_ids(&service.settings());

    // The settings page builds its document; before it arrives, the
    // dock's menu hides Spotify and pins Code.
    let mut stale = service.settings();
    stale.icon_size = 64;
    service.hide_app(&spotify).unwrap();
    service.pin_app(&code).unwrap();
    // The page's lists are older still: it names other apps altogether.
    let mut from_page = stale.clone();
    from_page.pinned.clear();
    from_page.hidden_apps.push(stem_core::settings::HiddenApp {
        id: AppId::for_exe(Path::new(r"C:\Apps\other.exe")),
        name: "other".into(),
        path: Some(r"C:\Apps\other.exe".into()),
        icon_key: None,
    });
    let saved = service.update_settings(stale).unwrap();
    assert_eq!(saved.icon_size, 64, "the rest of the document applies");
    assert_eq!(hidden_ids(&saved), vec![spotify.as_str()]);
    assert!(saved.is_pinned(&code));
    let saved = service.update_settings(from_page).unwrap();
    assert_eq!(hidden_ids(&saved), vec![spotify.as_str()]);
    let mut expected = pinned_before.clone();
    expected.push(code.clone());
    assert_eq!(pinned_ids(&saved), expected);
    // In memory and on disk alike.
    assert_eq!(saved, service.settings());
    let on_disk = Settings::from_json(&document.document().unwrap()).unwrap();
    assert_eq!(on_disk, saved);

    // A reset clears both lists, as its confirmation says.
    let reset = service.reset_settings().unwrap();
    assert_eq!(reset, Settings::default());
    assert!(reset.pinned.is_empty() && reset.hidden_apps.is_empty());
    assert_eq!(service.settings(), reset);
    let on_disk = Settings::from_json(&document.document().unwrap()).unwrap();
    assert_eq!(on_disk, reset);
    wait_state(&rx, |s| {
        item_titled(s, "Spotify").is_some() && app_items(s).iter().all(|i| !i.pinned)
    });
    service.shutdown();
}

#[test]
fn invalid_settings_document_is_an_error_not_a_panic() {
    let platform = Arc::new(MockPlatform::new());
    let store = MemorySettingsStore::with_document("{ not json");
    assert!(matches!(
        DockService::start(platform, Box::new(store)),
        Err(ServiceError::Settings(_))
    ));
}

#[test]
fn a_stale_whole_document_never_undoes_a_pin_and_reset_clears_them() {
    let (service, _platform, rx) = start();
    let state = wait_state(&rx, resolved);
    let spotify = item_titled(&state, "Spotify")
        .unwrap()
        .app_id
        .clone()
        .unwrap();

    // The settings page took its copy, then the dock pinned an app.
    let mut stale = service.settings();
    service.pin_app(&spotify).unwrap();
    stale.icon_size = 64;
    let saved = service.update_settings(stale.clone()).unwrap();
    assert_eq!(saved.icon_size, 64);
    assert!(saved.is_pinned(&spotify), "the pin survives the stale copy");
    // A document that drops every pin changes none either.
    stale.pinned.clear();
    assert!(service.update_settings(stale).unwrap().is_pinned(&spotify));

    // One field in place: the pin made meanwhile stays too.
    let was = service.settings().behavior.click_through;
    let saved = service
        .modify_settings(|s| s.behavior.click_through = !was)
        .unwrap();
    assert_eq!(saved.behavior.click_through, !was);
    assert!(saved.is_pinned(&spotify));

    // Reset restores the defaults, pins included.
    let reset = service.reset_settings().unwrap();
    assert_eq!(reset, Settings::default());
    assert_eq!(service.settings(), Settings::default());
    service.shutdown();
}
