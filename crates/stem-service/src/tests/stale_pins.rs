//! Stale `exe:` pins of packaged apps.

use pretty_assertions::assert_eq;

use super::*;

/// A packaged exe the platform fails on.
const BROKEN_EXE: &str =
    r"C:\Program Files\WindowsApps\Vendor.Broken_1.0.0.0_x64__abcdefghjkmnp\broken.exe";
/// A packaged exe the platform keeps as an exe (the mock's answer).
const AMBIGUOUS_EXE: &str =
    r"C:\Program Files\WindowsApps\Vendor.Suite_2.0.0.0_x64__abcdefghjkmnp\helper.exe";

fn exe_pin(path: &str) -> stem_core::settings::PinnedApp {
    stem_core::settings::PinnedApp {
        id: AppId::for_exe(Path::new(path)),
        label: None,
        launch: exe_target(path),
    }
}

fn terminal_pin() -> stem_core::settings::PinnedApp {
    stem_core::settings::PinnedApp {
        id: AppId::for_app_user_model_id(TERMINAL_AUMID),
        label: None,
        launch: LaunchTarget::StoreApp {
            app_user_model_id: TERMINAL_AUMID.into(),
        },
    }
}

/// A settings store that counts saves and keeps the document readable.
#[derive(Debug, Default)]
struct CountingStore {
    doc: Arc<MemorySettingsStore>,
    saves: Arc<AtomicUsize>,
}

impl CountingStore {
    fn with(settings: &Settings) -> Self {
        Self {
            doc: Arc::new(MemorySettingsStore::with_document(
                &settings.to_json_pretty(),
            )),
            saves: Arc::default(),
        }
    }
}

impl SettingsStore for CountingStore {
    fn load(&self) -> Result<Option<String>, StoreError> {
        self.doc.load()
    }

    fn save(&self, json: &str) -> Result<(), StoreError> {
        self.saves.fetch_add(1, Ordering::SeqCst);
        self.doc.save(json)
    }

    fn location(&self) -> String {
        self.doc.location()
    }
}

fn relay_with_terminal() -> Relay {
    let mock = MockPlatform::new();
    mock.push(PlatformEvent::WindowAdded(terminal_window()));
    Relay::over(mock)
        .answering(TERMINAL_EXE, Some(terminal_identity()))
        .answering(BROKEN_EXE, None)
}

#[test]
fn stale_packaged_pins_are_reidentified_once_and_merge_with_their_windows() {
    let firefox = exe_pin(r"C:\Program Files\Mozilla Firefox\firefox.exe");
    let mut stale = exe_pin(TERMINAL_EXE);
    stale.label = Some("Shell".into());
    let broken = exe_pin(BROKEN_EXE);
    let ambiguous = exe_pin(AMBIGUOUS_EXE);
    let settings = Settings {
        pinned: vec![
            firefox.clone(),
            stale.clone(),
            broken.clone(),
            ambiguous.clone(),
        ],
        ..Settings::default()
    };
    let store = CountingStore::with(&settings);
    let (doc, saves) = (Arc::clone(&store.doc), Arc::clone(&store.saves));
    let relay = Arc::new(relay_with_terminal());
    let service = DockService::start(relay.clone(), Box::new(store)).unwrap();
    let rx = record(&service);

    let new_id = AppId::for_app_user_model_id(TERMINAL_AUMID);
    wait_until("the rewritten pin", || {
        service.settings().is_pinned(&new_id)
    });
    // Rewritten in place with its label; the pin the platform failed on
    // and the one it kept as an exe are untouched.
    let mut rewritten = terminal_pin();
    rewritten.label = Some("Shell".into());
    let expected = vec![firefox.clone(), rewritten, broken, ambiguous];
    assert_eq!(service.settings().pinned, expected);
    // Saved once, and the file holds exactly that.
    assert_eq!(saves.load(Ordering::SeqCst), 1);
    let saved = Settings::from_json(&doc.document().unwrap()).unwrap();
    assert_eq!(saved.pinned, expected);

    // One item for the pin and the running window, under the AUMID,
    // with the name the platform gave; the stale exe item is gone.
    let state = wait_state(&rx, |s| {
        app_items(s)
            .iter()
            .any(|i| i.app_id.as_ref() == Some(&new_id) && i.pinned && i.windows.len() == 1)
    });
    let terminal: Vec<&DockItem> = app_items(&state)
        .into_iter()
        .filter(|i| i.windows.iter().any(|w| w.id == WindowId(50)) || i.title == "Shell")
        .collect();
    assert_eq!(terminal.len(), 1, "{terminal:?}");
    assert_eq!(terminal[0].title, "Shell");
    assert!(
        app_items(&state)
            .iter()
            .all(|i| i.app_id.as_ref() != Some(&stale.id))
    );
    // The migration asked once about the Terminal's exe; nothing asked
    // about the old pin afterwards.
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(relay.target_calls_for(TERMINAL_EXE), 1);
    assert_eq!(saves.load(Ordering::SeqCst), 1);
    service.shutdown();

    // A second start changes nothing: no save, and the Terminal's exe is
    // not asked about again (its pin is a StoreApp now).
    let store = CountingStore {
        doc: Arc::clone(&doc),
        saves: Arc::default(),
    };
    let saves = Arc::clone(&store.saves);
    let relay = Arc::new(relay_with_terminal());
    let service = DockService::start(relay.clone(), Box::new(store)).unwrap();
    let rx = record(&service);
    wait_state(&rx, |s| {
        app_items(s)
            .iter()
            .any(|i| i.app_id.as_ref() == Some(&new_id) && i.windows.len() == 1)
    });
    wait_until("the remaining candidates to be asked again", || {
        relay.target_calls_for(BROKEN_EXE) >= 1 && relay.target_calls_for(AMBIGUOUS_EXE) >= 1
    });
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(saves.load(Ordering::SeqCst), 0);
    assert_eq!(relay.target_calls_for(TERMINAL_EXE), 0);
    assert_eq!(service.settings().pinned, expected);
    service.shutdown();
}

#[test]
fn a_stale_pin_of_an_app_that_is_already_pinned_is_dropped() {
    let notepad = exe_pin(r"C:\Windows\notepad.exe");
    let stale = exe_pin(TERMINAL_EXE);
    let settings = Settings {
        pinned: vec![stale, notepad.clone(), terminal_pin()],
        ..Settings::default()
    };
    let store = CountingStore::with(&settings);
    let (doc, saves) = (Arc::clone(&store.doc), Arc::clone(&store.saves));
    let relay = Arc::new(relay_with_terminal());
    let service = DockService::start(relay, Box::new(store)).unwrap();

    let expected = vec![notepad, terminal_pin()];
    wait_until("the stale pin to be dropped", || {
        service.settings().pinned == expected
    });
    assert_eq!(saves.load(Ordering::SeqCst), 1);
    let saved = Settings::from_json(&doc.document().unwrap()).unwrap();
    assert_eq!(saved.pinned, expected);
    service.shutdown();
}
