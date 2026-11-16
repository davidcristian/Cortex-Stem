//! Start-up, clicks, the Recycle Bin drop, pins and the settings file.

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn startup_resolves_identities_and_groups_windows() {
    let (service, _platform, rx) = start();
    let state = wait_state(&rx, resolved);
    let firefox = item_titled(&state, "firefox").unwrap();
    assert_eq!(firefox.windows.len(), 2);
    assert!(firefox.is_active);
    assert_eq!(state.items.first().unwrap().kind, DockItemKind::Launcher);
    assert_eq!(state.items.last().unwrap().kind, DockItemKind::RecycleBin);
    assert_eq!(
        state.items.last().unwrap().icon_key,
        Some(IconKey::shell("recycle-bin-full"))
    );
    service.shutdown();
}

fn bin_icon(state: &DockState) -> Option<IconKey> {
    state
        .items
        .iter()
        .find(|i| i.kind == DockItemKind::RecycleBin)
        .and_then(|i| i.icon_key.clone())
}

fn start_empty() -> (DockService, Arc<MockPlatform>, Receiver<ServiceEvent>) {
    let platform = Arc::new(MockPlatform::new());
    let store = Box::new(MemorySettingsStore::default());
    let service = DockService::start(platform.clone(), store).unwrap();
    let rx = record(&service);
    (service, platform, rx)
}

#[test]
fn a_drop_on_the_bin_recycles_the_paths_and_the_icon_turns_full_without_a_poll() {
    let (service, platform, rx) = start_empty();
    assert_eq!(
        bin_icon(&service.state()),
        Some(IconKey::shell("recycle-bin-empty"))
    );
    let paths = vec![
        r"C:\Users\me\Desktop\notes.txt".to_owned(),
        r"C:\Users\me\Desktop\Old photos".to_owned(),
    ];

    service.delete_to_recycle_bin(&paths).unwrap();
    assert_eq!(platform.calls(), vec![MockCall::DeleteToRecycleBin(paths)]);
    // The mock pushes no event and has no poll: only the service's
    // refresh can tell the engine.
    let state = wait_state(&rx, |s| {
        bin_icon(s) == Some(IconKey::shell("recycle-bin-full"))
    });
    assert_eq!(service.platform().recycle_bin().unwrap().item_count, 2);
    assert_eq!(bin_icon(&state), Some(IconKey::shell("recycle-bin-full")));
    service.shutdown();
}

#[test]
fn a_refused_delete_is_an_error_and_the_bin_is_refreshed_anyway() {
    let (service, platform, rx) = start_empty();
    // Something is in the bin that no event reported (like the part of a
    // batch that went before the shell refused an item).
    platform
        .delete_to_recycle_bin(&[r"C:\Users\me\earlier.txt".to_owned()])
        .unwrap();
    platform.set_recycle_failure(Some(r"C:\Users\me\report.docx is open in Word"));

    let err = service
        .delete_to_recycle_bin(&[r"C:\Users\me\report.docx".to_owned()])
        .unwrap_err();
    assert!(matches!(
        err,
        ServiceError::Platform(PlatformError::Other(_))
    ));
    assert_eq!(err.to_string(), r"C:\Users\me\report.docx is open in Word");
    wait_state(&rx, |s| {
        bin_icon(s) == Some(IconKey::shell("recycle-bin-full"))
    });
    service.shutdown();
}

#[test]
fn a_drop_without_paths_asks_the_platform_nothing() {
    let (service, platform, _rx) = start_empty();
    service.delete_to_recycle_bin(&[]).unwrap();
    assert_eq!(platform.calls(), vec![]);
    service.shutdown();
}

#[test]
fn a_click_behind_our_own_window_activates_instead_of_minimising() {
    let asked = std::cell::Cell::new(0);
    let ask = |answer: bool| {
        let asked = &asked;
        move || {
            asked.set(asked.get() + 1);
            answer
        }
    };
    let id = WindowId(7);
    assert_eq!(
        over_own_window(ItemAction::MinimizeWindow(id), ask(true)),
        ItemAction::ActivateWindow(id)
    );
    assert_eq!(
        over_own_window(ItemAction::MinimizeWindow(id), ask(false)),
        ItemAction::MinimizeWindow(id)
    );
    assert_eq!(asked.get(), 2);
    // Only a minimise asks; nothing else changes.
    let app = AppId::for_app_user_model_id("Contoso.App!App");
    for action in [
        ItemAction::ActivateWindow(id),
        ItemAction::Launch(app),
        ItemAction::Shell(ShellAction::OpenStartMenu),
        ItemAction::None,
    ] {
        assert_eq!(over_own_window(action.clone(), ask(true)), action);
    }
    assert_eq!(asked.get(), 2);
}

#[test]
fn primary_click_minimises_active_and_activates_inactive() {
    let (service, platform, rx) = start();
    let state = wait_state(&rx, resolved);
    let firefox_key = item_titled(&state, "firefox").unwrap().key.clone();
    let code_key = item_titled(&state, "Code").unwrap().key.clone();

    // Firefox has two windows and the first is active -> cycle to the second.
    service.primary_click(&firefox_key, DOCK).unwrap();
    // Code has one inactive window -> activate.
    service.primary_click(&code_key, DOCK).unwrap();
    assert_eq!(
        platform.calls(),
        vec![
            MockCall::WindowAction(WindowId(3), WindowAction::Activate),
            MockCall::WindowAction(WindowId(4), WindowAction::Activate),
        ]
    );
    let state = wait_state(&rx, |s| s.foreground == Some(WindowId(4)));
    assert!(
        app_items(&state)
            .iter()
            .find(|i| i.key == code_key)
            .unwrap()
            .is_active
    );

    // Code is active with a single window, but the settings window is
    // in front of it (the engine still names Code active): the click
    // brings Code forward instead of minimising it behind the settings.
    platform.clear_calls();
    service
        .primary_click_over(&code_key, DOCK, || true)
        .unwrap();
    assert_eq!(
        platform.calls(),
        vec![MockCall::WindowAction(WindowId(4), WindowAction::Activate)]
    );

    // Code is active with a single window and nothing of ours is in
    // front (or a dock is, the focus shortcut's session) -> minimise.
    platform.clear_calls();
    service.primary_click(&code_key, DOCK).unwrap();
    assert_eq!(
        platform.calls(),
        vec![MockCall::WindowAction(WindowId(4), WindowAction::Minimize)]
    );

    // Launcher.
    service.primary_click("fixed:launcher", DOCK).unwrap();
    assert_eq!(
        platform.calls().last(),
        Some(&MockCall::Shell(ShellAction::OpenStartMenu))
    );
    assert!(matches!(
        service.primary_click("bogus", DOCK),
        Err(ServiceError::UnknownItem(_))
    ));
}

#[test]
fn pin_unpin_persist_and_launch_bounces() {
    let (service, platform, rx) = start();
    let state = wait_state(&rx, resolved);
    let spotify = item_titled(&state, "Spotify").unwrap().clone();
    let app_id = spotify.app_id.clone().unwrap();

    service.pin_app(&app_id).unwrap();
    let state = wait_state(&rx, |s| {
        s.items.iter().any(|i| i.key == spotify.key && i.pinned)
    });
    // Pinned items come before running ones.
    assert_eq!(app_items(&state)[0].key, spotify.key);
    assert!(service.settings().is_pinned(&app_id));

    // Close its only window: it stays, as a pinned app without windows.
    service
        .window_action(WindowId(6), WindowAction::Close)
        .unwrap();
    let state = wait_state(&rx, |s| {
        s.items
            .iter()
            .any(|i| i.key == spotify.key && i.windows.is_empty())
    });
    assert!(app_items(&state).iter().any(|i| i.key == spotify.key));

    // Clicking it now launches and bounces.
    platform.clear_calls();
    service.primary_click(&spotify.key, DOCK).unwrap();
    assert!(matches!(
        platform.calls().as_slice(),
        [MockCall::Launch(LaunchTarget::Exe { .. })]
    ));
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut bounced = false;
    while Instant::now() < deadline {
        if let Ok(ServiceEvent::Bounce { item_key }) = rx.recv_timeout(Duration::from_millis(100)) {
            assert_eq!(item_key, spotify.key);
            bounced = true;
            break;
        }
    }
    assert!(bounced, "expected a bounce event");

    service.unpin_app(&app_id).unwrap();
    wait_state(&rx, |s| !s.items.iter().any(|i| i.key == spotify.key));
    assert!(matches!(
        service.unpin_app(&app_id),
        Err(ServiceError::NotPinned(_))
    ));
}

#[test]
fn settings_are_loaded_validated_and_saved() {
    let platform = Arc::new(MockPlatform::new());
    let store =
        MemorySettingsStore::with_document(r#"{"iconSize": 1000, "items": {"launcher": false}}"#);
    let service = DockService::start(platform, Box::new(store)).unwrap();
    assert_eq!(service.settings().icon_size, 128);
    assert!(!service.settings().items.launcher);

    let mut s = service.settings();
    s.icon_size = 64;
    let saved = service.update_settings(s).unwrap();
    assert_eq!(saved.icon_size, 64);
}
