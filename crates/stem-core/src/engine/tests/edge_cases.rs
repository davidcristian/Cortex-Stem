//! Edge cases: late identities, resyncs, virtual desktops, attention, and
//! inputs that must not change the revision.

use pretty_assertions::assert_eq;

use super::*;

/// The platform could ask the shell: the window is (not) on the
/// current virtual desktop.
fn on_desktop(mut raw: RawWindow, on_current: bool) -> RawWindow {
    raw.on_current_desktop = Some(on_current);
    raw
}

#[test]
fn window_regroups_when_app_user_model_id_appears_later() {
    let mut engine = DockEngine::new(minimal_settings());
    let exe = exe_id(r"C:\app.exe");
    let aumid = AppId::for_app_user_model_id("Vendor.App_abc!App");

    engine.apply(added(win(1, r"C:\app.exe", "First")));
    engine.apply(added(win(2, r"C:\app.exe", "Second")));
    engine.apply(attention(1, true));
    assert_eq!(keys(&engine.snapshot()), vec!["app:exe:c:/app.exe"]);

    // The platform learns the AUMID of window 1 only now.
    let mut moved = win(1, r"C:\app.exe", "First");
    moved.app_user_model_id = Some("Vendor.App_abc!App".into());
    let out = engine.apply(updated(moved));
    assert!(out.changed);
    assert_eq!(out.unresolved_apps, vec![aumid.clone()]);

    let state = engine.snapshot();
    // The exe item keeps its first-seen slot; the AUMID item is a newcomer.
    assert_eq!(
        keys(&state),
        vec!["app:exe:c:/app.exe", "app:aumid:Vendor.App_abc!App"]
    );
    assert_eq!(state.items[0].windows.len(), 1);
    assert_eq!(state.items[1].windows.len(), 1);
    // Attention travels with the window.
    assert!(!state.items[0].needs_attention);
    assert!(state.items[1].needs_attention);
    assert_eq!(engine.windows_of(&aumid), vec![WindowId(1)]);
    assert_eq!(engine.windows_of(&exe), vec![WindowId(2)]);
    // Both apps are still unresolved (sorted by id); only the new one was
    // requested by this input, the exe id was requested when first seen.
    assert_eq!(engine.unresolved_apps(), vec![aumid, exe]);
}

#[test]
fn pinned_launch_target_change_keeps_key_identity_and_revision() {
    let mut settings = minimal_settings();
    settings.pinned = vec![pin(r"C:\a.exe")];
    let a = exe_id(r"C:\a.exe");
    let mut engine = DockEngine::new(settings.clone());
    engine.apply(EngineInput::IdentityResolved {
        app_id: a.clone(),
        identity: identity(&a, "App A", "icon-a"),
    });
    let before = engine.snapshot();

    settings.pinned[0].launch = LaunchTarget::Shortcut {
        path: PathBuf::from(r"C:\Users\me\Desktop\A.lnk"),
    };
    let out = engine.apply(EngineInput::SettingsChanged(settings));
    // Nothing the UI renders changed, so no new revision and no new
    // identity request; the click still launches by id.
    assert!(!out.changed);
    assert!(out.unresolved_apps.is_empty());
    assert_eq!(engine.snapshot(), before);
    assert_eq!(engine.snapshot().items[0].title, "App A");
    assert_eq!(
        engine.primary_action("app:exe:c:/a.exe", DOCK),
        ItemAction::Launch(a.clone())
    );
    assert!(matches!(
        engine.settings().pinned[0].launch,
        LaunchTarget::Shortcut { .. }
    ));
    assert_eq!(
        engine.identity(&a).map(|i| i.display_name.as_str()),
        Some("App A")
    );
}

#[test]
fn resync_after_many_adds_is_a_no_op() {
    let mut engine = DockEngine::new(minimal_settings());
    let windows: Vec<RawWindow> = (1..=40)
        .map(|i| win(i, &format!(r"C:\app{}.exe", i % 7), &format!("W{i}")))
        .collect();
    for w in &windows {
        engine.apply(added(w.clone()));
    }
    engine.apply(foreground(Some(5)));
    engine.apply(attention(9, true));
    let before = engine.snapshot();
    assert_eq!(before.items.len(), 7);

    // The platform re-enumerates in a different order.
    let mut list = windows.clone();
    list.reverse();
    let out = engine.apply(PlatformEvent::WindowsSnapshot(list).into());
    assert!(!out.changed);
    assert!(out.unresolved_apps.is_empty());
    assert_eq!(engine.snapshot(), before);
    assert_eq!(engine.foreground(), Some(WindowId(5)));

    // Window order inside an item also survives (first seen, not list order).
    let item = &engine.snapshot().items[0];
    let ids: Vec<u64> = item.windows.iter().map(|w| w.id.0).collect();
    assert_eq!(ids, vec![1, 8, 15, 22, 29, 36]);
}

#[test]
fn resync_drops_a_vanished_foreground_window() {
    let mut engine = DockEngine::new(minimal_settings());
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    engine.apply(added(win(2, r"C:\b.exe", "B")));
    engine.apply(foreground(Some(1)));
    assert!(engine.snapshot().items[0].is_active);

    let out = engine.apply(PlatformEvent::WindowsSnapshot(vec![win(2, r"C:\b.exe", "B")]).into());
    assert!(out.changed);
    assert_eq!(engine.foreground(), None);
    let state = engine.snapshot();
    assert_eq!(state.foreground, None);
    assert!(state.items.iter().all(|i| !i.is_active));
}

#[test]
fn foreground_on_a_cloaked_window_marks_nothing_active_until_it_is_shown() {
    let mut engine = DockEngine::new(minimal_settings());
    engine.apply(added(cloaked(win(1, r"C:\a.exe", "A"))));
    engine.apply(added(win(2, r"C:\b.exe", "B")));
    engine.apply(foreground(Some(2)));

    // The user switches to the virtual desktop with window 1: the OS
    // reports it as foreground while it is still cloaked for us.
    let out = engine.apply(foreground(Some(1)));
    assert!(out.changed);
    let state = engine.snapshot();
    assert_eq!(state.foreground, Some(WindowId(1)));
    assert_eq!(keys(&state), vec!["app:exe:c:/b.exe"]);
    assert!(state.items.iter().all(|i| !i.is_active));
    assert_eq!(
        engine.primary_action("app:exe:c:/a.exe", DOCK),
        ItemAction::None
    );

    // Uncloaking alone makes the item active; no new ForegroundChanged needed.
    let out = engine.apply(updated(win(1, r"C:\a.exe", "A")));
    assert!(out.changed);
    let state = engine.snapshot();
    assert_eq!(keys(&state), vec!["app:exe:c:/b.exe", "app:exe:c:/a.exe"]);
    assert!(state.items[1].is_active);
    assert!(!state.items[0].is_active);
}

#[test]
fn show_only_current_desktop_false_shows_cloaked_windows_and_toggles_live() {
    let mut settings = minimal_settings();
    settings.behavior.show_only_current_desktop = false;
    let mut engine = DockEngine::new(settings.clone());
    engine.apply(added(cloaked(win(1, r"C:\a.exe", "A"))));
    engine.apply(foreground(Some(1)));
    let state = engine.snapshot();
    assert_eq!(keys(&state), vec!["app:exe:c:/a.exe"]);
    assert!(state.items[0].is_active);
    assert_eq!(engine.windows_of(&exe_id(r"C:\a.exe")), vec![WindowId(1)]);
    assert_eq!(
        engine.primary_action("app:exe:c:/a.exe", DOCK),
        ItemAction::MinimizeWindow(WindowId(1))
    );

    settings.behavior.show_only_current_desktop = true;
    let out = engine.apply(EngineInput::SettingsChanged(settings.clone()));
    assert!(out.changed);
    assert!(engine.snapshot().items.is_empty());
    assert!(engine.windows_of(&exe_id(r"C:\a.exe")).is_empty());

    settings.behavior.show_only_current_desktop = false;
    let out = engine.apply(EngineInput::SettingsChanged(settings));
    assert!(out.changed);
    assert_eq!(keys(&engine.snapshot()), vec!["app:exe:c:/a.exe"]);
}

#[test]
fn on_current_desktop_decides_instead_of_the_cloak_state_when_known() {
    let mut settings = minimal_settings();
    settings.behavior.show_only_current_desktop = true;
    let mut engine = DockEngine::new(settings.clone());
    // Parked on another desktop: away, cloaked or not.
    engine.apply(added(on_desktop(win(1, r"C:\a.exe", "A"), false)));
    engine.apply(added(on_desktop(cloaked(win(2, r"C:\b.exe", "B")), false)));
    // On this desktop but cloaked: a suspended Store app, or a window in
    // the switch animation. Listed.
    engine.apply(added(on_desktop(cloaked(win(3, r"C:\c.exe", "C")), true)));
    // Unknown: the cloak state decides.
    engine.apply(added(cloaked(win(4, r"C:\d.exe", "D"))));
    engine.apply(added(win(5, r"C:\e.exe", "E")));
    assert_eq!(
        keys(&engine.snapshot()),
        vec!["app:exe:c:/c.exe", "app:exe:c:/e.exe"]
    );
    assert!(engine.windows_of(&exe_id(r"C:\a.exe")).is_empty());
    assert_eq!(engine.windows_of(&exe_id(r"C:\c.exe")), vec![WindowId(3)]);
    assert_eq!(
        engine.primary_action("app:exe:c:/a.exe", DOCK),
        ItemAction::None
    );
    assert_eq!(
        engine.primary_action("app:exe:c:/c.exe", DOCK),
        ItemAction::ActivateWindow(WindowId(3))
    );

    // With the setting off every window is listed whatever the desktop:
    // the apps already listed keep their order, the others follow.
    settings.behavior.show_only_current_desktop = false;
    let out = engine.apply(EngineInput::SettingsChanged(settings));
    assert!(out.changed);
    assert_eq!(
        keys(&engine.snapshot()),
        vec![
            "app:exe:c:/c.exe",
            "app:exe:c:/e.exe",
            "app:exe:c:/a.exe",
            "app:exe:c:/b.exe",
            "app:exe:c:/d.exe",
        ]
    );
}

#[test]
fn a_desktop_switch_arrives_as_updates_and_then_as_a_snapshot() {
    let mut engine = DockEngine::new(minimal_settings());
    let here = |id, exe, title| on_desktop(win(id, exe, title), true);
    let away = |id, exe, title| on_desktop(cloaked(win(id, exe, title)), false);
    let snapshot = |list: Vec<RawWindow>| EngineInput::from(PlatformEvent::WindowsSnapshot(list));
    engine.apply(snapshot(vec![
        here(1, r"C:\a.exe", "A"),
        away(2, r"C:\b.exe", "B"),
    ]));
    engine.apply(foreground(Some(1)));
    let state = engine.snapshot();
    assert_eq!(keys(&state), vec!["app:exe:c:/a.exe"]);
    assert!(state.items[0].is_active);

    // Win+Ctrl+Right: the shell cloaks A and uncloaks B, one event each,
    // and B takes the foreground.
    let out = engine.apply(updated(away(1, r"C:\a.exe", "A")));
    assert!(out.changed);
    assert!(engine.snapshot().items.is_empty());
    let out = engine.apply(updated(here(2, r"C:\b.exe", "B")));
    assert!(out.changed);
    engine.apply(foreground(Some(2)));
    let state = engine.snapshot();
    assert_eq!(keys(&state), vec!["app:exe:c:/b.exe"]);
    assert!(state.items[0].is_active);

    // The platform's snapshot 400 ms later lists the same facts: a no-op.
    let revision = state.revision;
    let out = engine.apply(snapshot(vec![
        away(1, r"C:\a.exe", "A"),
        here(2, r"C:\b.exe", "B"),
    ]));
    assert!(!out.changed);
    assert_eq!(engine.snapshot().revision, revision);

    // A snapshot that reveals a window the events missed (C uncloaked on
    // this desktop) adds it, after B, which was seen first.
    let out = engine.apply(snapshot(vec![
        away(1, r"C:\a.exe", "A"),
        here(2, r"C:\b.exe", "B"),
        here(3, r"C:\c.exe", "C"),
    ]));
    assert!(out.changed);
    assert_eq!(
        keys(&engine.snapshot()),
        vec!["app:exe:c:/b.exe", "app:exe:c:/c.exe"]
    );
}

#[test]
fn attention_is_dropped_when_the_window_closes() {
    let mut settings = minimal_settings();
    settings.pinned = vec![pin(r"C:\p.exe")];
    let mut engine = DockEngine::new(settings);
    engine.apply(added(win(1, r"C:\p.exe", "P")));
    engine.apply(added(win(2, r"C:\u.exe", "U")));
    engine.apply(attention(1, true));
    engine.apply(attention(2, true));
    let state = engine.snapshot();
    assert!(
        state
            .items
            .iter()
            .all(|i| i.kind != DockItemKind::App || i.needs_attention)
    );

    let out = engine.apply(removed(1));
    assert!(out.changed);
    let out = engine.apply(removed(2));
    assert!(out.changed);
    let state = engine.snapshot();
    // Pinned item stays, calm; the unpinned one is gone.
    assert_eq!(keys(&state), vec!["app:exe:c:/p.exe"]);
    assert!(!state.items[0].needs_attention);

    // A new window of the same app does not inherit the old flash.
    engine.apply(added(win(3, r"C:\p.exe", "P again")));
    assert!(!engine.snapshot().items[0].needs_attention);

    // The same holds when a resync drops the flashing window.
    engine.apply(attention(3, true));
    assert!(engine.snapshot().items[0].needs_attention);
    engine.apply(PlatformEvent::WindowsSnapshot(vec![]).into());
    assert!(!engine.snapshot().items[0].needs_attention);
}

#[test]
fn each_window_carries_its_own_attention_flag() {
    // Two windows of one app: the item needs attention while any
    // of them does, and the snapshot says which one.
    let mut engine = DockEngine::new(minimal_settings());
    engine.apply(added(win(1, r"C:\a.exe", "A one")));
    engine.apply(added(win(2, r"C:\a.exe", "A two")));
    let flags = |engine: &DockEngine| -> (bool, Vec<bool>) {
        let item = engine.snapshot().items[0].clone();
        let windows = item.windows.iter().map(|w| w.needs_attention).collect();
        (item.needs_attention, windows)
    };
    assert_eq!(flags(&engine), (false, vec![false, false]));

    assert!(engine.apply(attention(2, true)).changed);
    assert_eq!(flags(&engine), (true, vec![false, true]));

    // The other window flashes too; then the first one stops: the item
    // still needs attention, but the snapshot changed (which window).
    assert!(engine.apply(attention(1, true)).changed);
    assert_eq!(flags(&engine), (true, vec![true, true]));
    let revision = engine.snapshot().revision;
    assert!(engine.apply(attention(1, false)).changed);
    assert_eq!(flags(&engine), (true, vec![false, true]));
    assert_eq!(engine.snapshot().revision, revision + 1);

    // Activating the flashing window clears its own flag.
    engine.apply(foreground(Some(2)));
    assert_eq!(flags(&engine), (false, vec![false, false]));

    // The foreground window is never flagged; the one behind it is.
    assert!(!engine.apply(attention(2, true)).changed);
    assert!(engine.apply(attention(1, true)).changed);
    assert_eq!(flags(&engine), (true, vec![true, false]));
}

#[test]
fn attention_and_removal_of_unknown_windows_are_ignored() {
    let mut engine = DockEngine::new(minimal_settings());
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    let before = engine.snapshot();
    assert!(!engine.apply(attention(99, true)).changed);
    assert!(!engine.apply(removed(99)).changed);
    assert!(!engine.apply(attention(1, false)).changed);
    assert_eq!(engine.snapshot(), before);
}

#[test]
fn item_key_is_stable_across_pin_and_unpin() {
    let mut settings = minimal_settings();
    let mut engine = DockEngine::new(settings.clone());
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    engine.apply(added(win(2, r"C:\b.exe", "B")));
    let running_key = engine.snapshot().items[1].key.clone();
    assert_eq!(running_key, "app:exe:c:/b.exe");

    settings.pinned = vec![pin(r"C:\b.exe")];
    let out = engine.apply(EngineInput::SettingsChanged(settings.clone()));
    assert!(out.changed);
    let state = engine.snapshot();
    assert_eq!(
        keys(&state),
        vec!["app:exe:c:/b.exe", "sep:1", "app:exe:c:/a.exe"]
    );
    assert_eq!(state.items[0].key, running_key);
    assert!(state.items[0].pinned);
    assert_eq!(state.items[0].windows.len(), 1);

    settings.pinned.clear();
    engine.apply(EngineInput::SettingsChanged(settings));
    let state = engine.snapshot();
    // Back in the running group, in its original first-seen slot.
    assert_eq!(keys(&state), vec!["app:exe:c:/a.exe", "app:exe:c:/b.exe"]);
    assert_eq!(state.items[1].key, running_key);
    assert!(!state.items[1].pinned);

    // Unpinning an app with no windows removes its item.
    engine.apply(removed(2));
    assert_eq!(keys(&engine.snapshot()), vec!["app:exe:c:/a.exe"]);
}

#[test]
fn monitors_and_theme_changes_do_not_bump_the_snapshot() {
    let mut engine = DockEngine::new(Settings::default());
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    let before = engine.snapshot();

    let monitors = vec![MonitorInfo {
        id: MonitorId("m1".into()),
        name: "Main".into(),
        bounds: Rect::new(0, 0, 1920, 1080),
        work_area: Rect::new(0, 0, 1920, 1032),
        scale_factor: 1.25,
        is_primary: true,
    }];
    let out = engine.apply(PlatformEvent::MonitorsChanged(monitors.clone()).into());
    assert!(!out.changed);
    assert_eq!(engine.monitors(), monitors.as_slice());

    let theme = ThemeInfo {
        accent: "#FF0000".into(),
        dark: false,
        high_contrast: true,
        transparency: false,
    };
    let out = engine.apply(PlatformEvent::ThemeChanged(theme.clone()).into());
    assert!(!out.changed);
    assert_eq!(engine.theme(), &theme);

    assert_eq!(engine.snapshot(), before);
}

#[test]
fn recycle_bin_changes_bump_only_when_the_icon_flips() {
    let mut engine = DockEngine::new(Settings::default());
    let bin = |count: u64| -> EngineInput {
        PlatformEvent::RecycleBinChanged(RecycleBinState {
            item_count: count,
            size_bytes: count * 100,
        })
        .into()
    };
    let icon = |engine: &DockEngine| {
        engine
            .snapshot()
            .items
            .last()
            .and_then(|i| i.icon_key.clone())
    };
    assert_eq!(icon(&engine), Some(IconKey::shell("recycle-bin-empty")));

    assert!(engine.apply(bin(3)).changed);
    assert_eq!(icon(&engine), Some(IconKey::shell("recycle-bin-full")));
    let r = engine.snapshot().revision;
    assert!(!engine.apply(bin(4)).changed);
    assert_eq!(engine.snapshot().revision, r);
    assert_eq!(engine.recycle_bin().item_count, 4);
    assert!(engine.apply(bin(0)).changed);
    assert_eq!(icon(&engine), Some(IconKey::shell("recycle-bin-empty")));

    // Without the item, the bin state is invisible and never bumps.
    let mut engine = DockEngine::new(minimal_settings());
    assert!(!engine.apply(bin(3)).changed);
    assert!(!engine.apply(bin(0)).changed);
    assert_eq!(engine.snapshot().revision, 0);
}

#[test]
fn settings_changes_bump_only_when_items_change() {
    let mut settings = minimal_settings();
    let mut engine = DockEngine::new(settings.clone());
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    let r = engine.snapshot().revision;

    settings.icon_size = 96;
    settings.magnification.max_scale = 2.0;
    settings.appearance.opacity = 0.5;
    let out = engine.apply(EngineInput::SettingsChanged(settings.clone()));
    assert!(!out.changed);
    assert_eq!(engine.snapshot().revision, r);
    assert_eq!(engine.settings().icon_size, 96);

    settings.items.launcher = true;
    let out = engine.apply(EngineInput::SettingsChanged(settings.clone()));
    assert!(out.changed);
    assert_eq!(
        keys(&engine.snapshot()),
        vec!["fixed:launcher", "sep:1", "app:exe:c:/a.exe"]
    );

    settings.items.separators = false;
    let out = engine.apply(EngineInput::SettingsChanged(settings.clone()));
    assert!(out.changed);
    assert_eq!(
        keys(&engine.snapshot()),
        vec!["fixed:launcher", "app:exe:c:/a.exe"]
    );

    // Re-applying identical settings is a no-op.
    assert!(!engine.apply(EngineInput::SettingsChanged(settings)).changed);
}

#[test]
fn identity_for_an_absent_app_is_kept_without_bumping() {
    let mut engine = DockEngine::new(minimal_settings());
    let a = exe_id(r"C:\a.exe");
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    engine.apply(removed(1));
    let r = engine.snapshot().revision;

    // The resolution arrives after the window closed (service latency).
    let out = engine.apply(EngineInput::IdentityResolved {
        app_id: a.clone(),
        identity: identity(&a, "App A", "k"),
    });
    assert!(!out.changed);
    assert_eq!(engine.snapshot().revision, r);

    // When it comes back it is named right away and not requested again.
    let out = engine.apply(added(win(2, r"C:\a.exe", "A")));
    assert!(out.changed);
    assert!(out.unresolved_apps.is_empty());
    assert_eq!(engine.snapshot().items[0].title, "App A");

    // Resolving to the same identity again is a no-op.
    let out = engine.apply(EngineInput::IdentityResolved {
        app_id: a.clone(),
        identity: identity(&a, "App A", "k"),
    });
    assert!(!out.changed);
}

#[test]
fn identity_unavailable_forgets_the_request_without_a_fallback() {
    let mut engine = DockEngine::new(minimal_settings());
    let a = exe_id(r"C:\a.exe");
    let out = engine.apply(added(win(1, r"C:\a.exe", "A")));
    assert_eq!(out.unresolved_apps, vec![a.clone()]);
    engine.apply(removed(1));
    let r = engine.snapshot().revision;

    // The resolver found nothing to ask (no window, no pin): nothing is
    // stored, nothing is shown, nothing is re-requested yet.
    let out = engine.apply(EngineInput::IdentityUnavailable { app_id: a.clone() });
    assert!(!out.changed);
    assert!(out.unresolved_apps.is_empty());
    assert_eq!(engine.snapshot().revision, r);
    assert_eq!(engine.identity(&a), None);
    assert!(engine.unresolved_apps().is_empty());

    // The app comes back: its identity is requested again, and the item
    // shows the window title until it is resolved (no fallback name).
    let out = engine.apply(added(win(2, r"C:\a.exe", "A again")));
    assert!(out.changed);
    assert_eq!(out.unresolved_apps, vec![a.clone()]);
    assert_eq!(engine.snapshot().items[0].title, "A again");
    assert_eq!(engine.unresolved_apps(), vec![a.clone()]);

    // Unavailable while the app is present (it re-appeared before the
    // message arrived): the same input re-requests it immediately.
    let out = engine.apply(EngineInput::IdentityUnavailable { app_id: a.clone() });
    assert!(!out.changed);
    assert_eq!(out.unresolved_apps, vec![a.clone()]);

    // An unknown id is ignored.
    let out = engine.apply(EngineInput::IdentityUnavailable {
        app_id: exe_id(r"C:\none.exe"),
    });
    assert!(!out.changed);
    assert!(out.unresolved_apps.is_empty());
}

#[test]
fn window_updates_without_visible_effect_do_not_bump() {
    let mut engine = DockEngine::new(minimal_settings());
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    let r = engine.snapshot().revision;

    let mut same = win(1, r"C:\a.exe", "A");
    same.process_id = 4242;
    same.class_name = "Other".into();
    assert!(!engine.apply(updated(same)).changed);
    assert_eq!(engine.snapshot().revision, r);
    assert_eq!(engine.window(WindowId(1)).map(|w| w.process_id), Some(4242));

    let mut renamed = win(1, r"C:\a.exe", "A - edited");
    renamed.is_minimized = true;
    assert!(engine.apply(updated(renamed)).changed);
    let item = &engine.snapshot().items[0];
    assert_eq!(item.windows[0].title, "A - edited");
    assert!(item.windows[0].is_minimized);
}

#[test]
fn foreground_to_an_untracked_window_clears_the_active_marker() {
    let mut engine = DockEngine::new(minimal_settings());
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    engine.apply(foreground(Some(1)));
    assert!(engine.snapshot().items[0].is_active);

    // A tool window or the desktop took focus: not something we track.
    let out = engine.apply(foreground(Some(999)));
    assert!(out.changed);
    assert_eq!(engine.foreground(), None);
    assert!(!engine.snapshot().items[0].is_active);
    assert!(!engine.apply(foreground(None)).changed);
}

#[test]
fn duplicate_window_added_is_treated_as_an_update() {
    let mut engine = DockEngine::new(minimal_settings());
    engine.apply(added(win(1, r"C:\a.exe", "A")));
    engine.apply(added(win(2, r"C:\b.exe", "B")));
    engine.apply(attention(1, true));

    let out = engine.apply(added(win(1, r"C:\a.exe", "A renamed")));
    assert!(out.changed);
    let state = engine.snapshot();
    // Still one window, still first, attention untouched.
    assert_eq!(keys(&state), vec!["app:exe:c:/a.exe", "app:exe:c:/b.exe"]);
    assert_eq!(state.items[0].windows.len(), 1);
    assert_eq!(state.items[0].windows[0].title, "A renamed");
    assert!(state.items[0].needs_attention);
}

#[test]
fn windows_of_lists_shown_windows_in_first_seen_order() {
    let mut engine = DockEngine::new(minimal_settings());
    let a = exe_id(r"C:\a.exe");
    engine.apply(added(win(3, r"C:\a.exe", "third id, first seen")));
    engine.apply(added(win(1, r"C:\b.exe", "other app")));
    let mut minimized = win(2, r"C:\a.exe", "minimised");
    minimized.is_minimized = true;
    engine.apply(added(minimized));
    engine.apply(added(cloaked(win(4, r"C:\a.exe", "cloaked"))));
    let mut invisible = win(5, r"C:\a.exe", "invisible");
    invisible.is_visible = false;
    engine.apply(added(invisible));

    assert_eq!(engine.windows_of(&a), vec![WindowId(3), WindowId(2)]);
    assert_eq!(engine.windows_of(&exe_id(r"C:\b.exe")), vec![WindowId(1)]);
    assert!(engine.windows_of(&exe_id(r"C:\none.exe")).is_empty());
    let item = &engine.snapshot().items[0];
    let listed: Vec<WindowId> = item.windows.iter().map(|w| w.id).collect();
    assert_eq!(listed, engine.windows_of(&a));
}

#[test]
fn primary_action_cycles_through_shown_windows_only() {
    let mut engine = DockEngine::new(minimal_settings());
    engine.apply(added(win(1, r"C:\a.exe", "A1")));
    engine.apply(added(cloaked(win(2, r"C:\a.exe", "A2 on another desktop"))));
    engine.apply(added(win(3, r"C:\a.exe", "A3")));
    engine.apply(foreground(Some(1)));
    assert_eq!(
        engine.primary_action("app:exe:c:/a.exe", DOCK),
        ItemAction::ActivateWindow(WindowId(3))
    );
    engine.apply(foreground(Some(3)));
    assert_eq!(
        engine.primary_action("app:exe:c:/a.exe", DOCK),
        ItemAction::ActivateWindow(WindowId(1))
    );

    // Only the hidden window left: it behaves like a closed app.
    engine.apply(removed(1));
    engine.apply(removed(3));
    assert_eq!(
        engine.primary_action("app:exe:c:/a.exe", DOCK),
        ItemAction::None
    );
}
