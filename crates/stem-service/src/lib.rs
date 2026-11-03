//! `stem-service` — the application logic between the Tauri shell and the OS.
//!
//! It owns the engine, drives it from platform events on a worker thread,
//! resolves app identities, persists settings, and implements the *semantics*
//! of every user command (`primary_click`, `pin_app`, ...). The Tauri crate
//! is a thin adapter over this API, which keeps all of this testable on any
//! host with [`stem_platform::mock::MockPlatform`].
//!
//! Threading model:
//! - the platform delivers events on its own thread into an unbounded channel;
//! - one **worker thread** drains the channel, applies inputs to the engine,
//!   resolves identities (platform calls, possibly slow), and notifies
//!   listeners;
//! - commands are called from any thread; they take the engine lock briefly
//!   and perform platform calls without holding it.

#![forbid(unsafe_code)]

pub mod store;

use std::sync::Arc;
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender};
use parking_lot::Mutex;
use stem_core::{
    AppId, AppIdentity, DockEngine, DockState, EngineInput, IconKey, ItemAction, LaunchTarget,
    MonitorInfo, PlatformEvent, Settings, SettingsError, ShellAction, ThemeInfo, WindowAction,
    WindowId,
};
use stem_platform::{Platform, PlatformError};
use thiserror::Error;

pub use store::{FileSettingsStore, MemorySettingsStore, SettingsStore, StoreError};

/// Something the UI should hear about.
#[derive(Debug, Clone, PartialEq)]
pub enum ServiceEvent {
    State(DockState),
    Settings(Settings),
    Theme(ThemeInfo),
    Monitors(Vec<MonitorInfo>),
    /// Play the launch animation on this item.
    Bounce {
        item_key: String,
    },
}

pub type Listener = Box<dyn Fn(&ServiceEvent) + Send + Sync + 'static>;

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error(transparent)]
    Platform(#[from] PlatformError),
    #[error(transparent)]
    Settings(#[from] SettingsError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("unknown dock item: {0}")]
    UnknownItem(String),
    #[error("app {0:?} has no launch target")]
    NotLaunchable(AppId),
    #[error("app {0:?} is not pinned")]
    NotPinned(AppId),
}

pub type ServiceResult<T> = Result<T, ServiceError>;

struct Inner {
    platform: Arc<dyn Platform>,
    store: Box<dyn SettingsStore>,
    engine: Mutex<DockEngine>,
    listeners: Mutex<Vec<Listener>>,
    tx: Sender<EngineInput>,
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inner")
            .field("platform", &self.platform.name())
            .field("store", &self.store.location())
            .finish()
    }
}

/// Handle to the running service. Clone-free by design: wrap in `Arc` if
/// several owners are needed. Dropping it shuts the service down.
#[derive(Debug)]
pub struct DockService {
    inner: Arc<Inner>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl DockService {
    /// Load settings, start the platform, and begin processing events.
    pub fn start(
        platform: Arc<dyn Platform>,
        store: Box<dyn SettingsStore>,
    ) -> ServiceResult<Self> {
        let settings = match store.load()? {
            Some(json) => Settings::from_json(&json)?,
            None => Settings::default(),
        };
        log::info!("settings loaded from {}", store.location());

        let (tx, rx) = crossbeam_channel::unbounded::<EngineInput>();
        let inner = Arc::new(Inner {
            platform: Arc::clone(&platform),
            store,
            engine: Mutex::new(DockEngine::new(settings)),
            listeners: Mutex::new(Vec::new()),
            tx: tx.clone(),
        });

        let worker = {
            let weak = Arc::downgrade(&inner);
            std::thread::Builder::new()
                .name("stem-service".into())
                .spawn(move || worker_loop(&rx, &weak))
                .map_err(|e| {
                    ServiceError::Store(StoreError::Io {
                        path: "<thread>".into(),
                        source: e,
                    })
                })?
        };

        let sink_tx = tx;
        platform.start(Arc::new(move |ev: PlatformEvent| {
            // The worker may already be gone during shutdown; dropping the
            // event is the right thing then.
            let _ = sink_tx.send(EngineInput::Platform(ev));
        }))?;

        Ok(Self {
            inner,
            worker: Mutex::new(Some(worker)),
        })
    }

    /// Register a listener. Listeners run on the worker thread; keep them cheap.
    pub fn subscribe(&self, listener: Listener) {
        self.inner.listeners.lock().push(listener);
    }

    pub fn platform(&self) -> &Arc<dyn Platform> {
        &self.inner.platform
    }

    pub fn state(&self) -> DockState {
        self.inner.engine.lock().snapshot()
    }

    pub fn settings(&self) -> Settings {
        self.inner.engine.lock().settings().clone()
    }

    pub fn theme(&self) -> ThemeInfo {
        self.inner.engine.lock().theme().clone()
    }

    pub fn monitors(&self) -> Vec<MonitorInfo> {
        self.inner.engine.lock().monitors().to_vec()
    }

    /// Validate, persist, and apply new settings. Returns the validated copy.
    pub fn update_settings(&self, mut settings: Settings) -> ServiceResult<Settings> {
        settings.migrate();
        settings.validate();
        self.inner.persist_and_apply(settings.clone())?;
        Ok(settings)
    }

    /// The dock's "click": launch, activate, minimise, or run a shell action.
    pub fn primary_click(&self, item_key: &str) -> ServiceResult<()> {
        let action = self.inner.engine.lock().primary_action(item_key);
        log::debug!("primary click on {item_key}: {action:?}");
        match action {
            ItemAction::Launch(app_id) => self.launch_app(&app_id),
            ItemAction::ActivateWindow(id) => Ok(self
                .inner
                .platform
                .window_action(id, WindowAction::Activate)?),
            ItemAction::MinimizeWindow(id) => Ok(self
                .inner
                .platform
                .window_action(id, WindowAction::Minimize)?),
            ItemAction::Shell(action) => Ok(self.inner.platform.shell_action(action)?),
            ItemAction::None => Err(ServiceError::UnknownItem(item_key.to_owned())),
        }
    }

    pub fn window_action(&self, id: WindowId, action: WindowAction) -> ServiceResult<()> {
        Ok(self.inner.platform.window_action(id, action)?)
    }

    pub fn shell_action(&self, action: ShellAction) -> ServiceResult<()> {
        Ok(self.inner.platform.shell_action(action)?)
    }

    /// Start (another instance of) an app, pinned or merely running.
    pub fn launch_app(&self, app_id: &AppId) -> ServiceResult<()> {
        let target = self
            .inner
            .launch_target_for(app_id)
            .ok_or_else(|| ServiceError::NotLaunchable(app_id.clone()))?;
        self.inner.platform.launch(&target)?;
        self.inner.notify(&ServiceEvent::Bounce {
            item_key: format!("app:{}", app_id.as_str()),
        });
        Ok(())
    }

    /// Pin a running app (its identity must already be resolved).
    pub fn pin_app(&self, app_id: &AppId) -> ServiceResult<()> {
        let (mut settings, launch) = {
            let engine = self.inner.engine.lock();
            (
                engine.settings().clone(),
                engine.identity(app_id).and_then(|i| i.launch.clone()),
            )
        };
        let launch = launch.ok_or_else(|| ServiceError::NotLaunchable(app_id.clone()))?;
        if settings.pin(stem_core::settings::PinnedApp {
            id: app_id.clone(),
            label: None,
            launch,
        }) {
            self.inner.persist_and_apply(settings)?;
        }
        Ok(())
    }

    /// Pin something the user dropped or chose: a path, a shortcut, a Store app.
    /// Returns the canonical id it was pinned under.
    pub fn pin_target(&self, target: LaunchTarget) -> ServiceResult<AppId> {
        let identity = self.inner.platform.identify_target(&target)?;
        let app_id = identity.id.clone();
        let launch = identity.launch.clone().unwrap_or(target);
        let mut settings = self.settings();
        if settings.pin(stem_core::settings::PinnedApp {
            id: app_id.clone(),
            label: None,
            launch,
        }) {
            // Make the identity known before the new item renders.
            let _ = self.inner.tx.send(EngineInput::IdentityResolved {
                app_id: app_id.clone(),
                identity,
            });
            self.inner.persist_and_apply(settings)?;
        }
        Ok(app_id)
    }

    pub fn unpin_app(&self, app_id: &AppId) -> ServiceResult<()> {
        let mut settings = self.settings();
        if !settings.unpin(app_id) {
            return Err(ServiceError::NotPinned(app_id.clone()));
        }
        self.inner.persist_and_apply(settings)
    }

    pub fn reorder_pinned(&self, order: &[AppId]) -> ServiceResult<()> {
        let mut settings = self.settings();
        settings.reorder_pinned(order);
        self.inner.persist_and_apply(settings)
    }

    pub fn icon_png(&self, key: &IconKey, size: u32) -> ServiceResult<Option<Vec<u8>>> {
        Ok(self.inner.platform.icon_png(key, size)?)
    }

    /// Stop the platform and the worker thread. Called on drop as well.
    pub fn shutdown(&self) {
        self.inner.platform.stop();
        // Closing our sender is not enough while the platform sink holds a
        // clone; `stop` dropped the sink, so the channel drains and ends once
        // `inner.tx` goes away with the last `Arc<Inner>`. Join with a bound.
        if let Some(handle) = self.worker.lock().take() {
            let _ = self
                .inner
                .tx
                .send(EngineInput::SettingsChanged(self.settings())); // wake, no-op change
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while !handle.is_finished() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            if handle.is_finished() {
                let _ = handle.join();
            } else {
                log::warn!("stem-service worker did not stop in time; detaching");
            }
        }
    }
}

impl Drop for DockService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl Inner {
    fn notify(&self, event: &ServiceEvent) {
        for l in self.listeners.lock().iter() {
            l(event);
        }
    }

    fn persist_and_apply(&self, settings: Settings) -> ServiceResult<()> {
        self.store.save(&settings.to_json_pretty())?;
        let _ = self.tx.send(EngineInput::SettingsChanged(settings));
        Ok(())
    }

    fn launch_target_for(&self, app_id: &AppId) -> Option<LaunchTarget> {
        let engine = self.engine.lock();
        engine
            .settings()
            .pinned
            .iter()
            .find(|p| &p.id == app_id)
            .map(|p| p.launch.clone())
            .or_else(|| engine.identity(app_id).and_then(|i| i.launch.clone()))
    }

    /// Apply one input and fan out the consequences.
    fn process(&self, input: EngineInput) {
        let is_settings = matches!(input, EngineInput::SettingsChanged(_));
        let theme = match &input {
            EngineInput::Platform(PlatformEvent::ThemeChanged(t)) => Some(t.clone()),
            _ => None,
        };
        let monitors = match &input {
            EngineInput::Platform(PlatformEvent::MonitorsChanged(m)) => Some(m.clone()),
            _ => None,
        };

        let (outcome, snapshot, settings) = {
            let mut engine = self.engine.lock();
            let outcome = engine.apply(input);
            (outcome, engine.snapshot(), engine.settings().clone())
        };

        for app_id in outcome.unresolved_apps {
            let identity = self.resolve_identity(&app_id);
            let _ = self
                .tx
                .send(EngineInput::IdentityResolved { app_id, identity });
        }

        if let Some(t) = theme {
            self.notify(&ServiceEvent::Theme(t));
        }
        if let Some(m) = monitors {
            self.notify(&ServiceEvent::Monitors(m));
        }
        if is_settings {
            self.notify(&ServiceEvent::Settings(settings));
        }
        if outcome.changed {
            self.notify(&ServiceEvent::State(snapshot));
        }
    }

    /// Ask the platform who this app is; never fails (falls back to what the
    /// engine already knows) so the engine stops asking.
    fn resolve_identity(&self, app_id: &AppId) -> AppIdentity {
        let (pinned, window) = {
            let engine = self.engine.lock();
            let pinned = engine
                .settings()
                .pinned
                .iter()
                .find(|p| &p.id == app_id)
                .cloned();
            let window = engine
                .snapshot()
                .items
                .into_iter()
                .find(|i| i.app_id.as_ref() == Some(app_id))
                .and_then(|i| i.windows.first().map(|w| w.id));
            (pinned, window)
        };

        let from_window = window.and_then(|id| match self.platform.identify_window(id) {
            Ok(i) => Some(i),
            Err(e) => {
                log::debug!("identify_window({id:?}) failed: {e}");
                None
            }
        });
        let from_target = || {
            pinned
                .as_ref()
                .and_then(|p| match self.platform.identify_target(&p.launch) {
                    Ok(i) => Some(i),
                    Err(e) => {
                        log::debug!("identify_target({:?}) failed: {e}", p.launch);
                        None
                    }
                })
        };

        let mut identity = from_window
            .or_else(from_target)
            .unwrap_or_else(|| AppIdentity {
                id: app_id.clone(),
                display_name: pinned
                    .as_ref()
                    .and_then(|p| p.label.clone())
                    .unwrap_or_else(|| fallback_display_name(app_id)),
                icon_key: None,
                launch: pinned.as_ref().map(|p| p.launch.clone()),
            });
        // The engine groups by the id *it* derived; keep that as the key even
        // if the platform preferred a different canonical form.
        identity.id = app_id.clone();
        if identity.launch.is_none() {
            identity.launch = pinned.map(|p| p.launch);
        }
        identity
    }
}

fn fallback_display_name(app_id: &AppId) -> String {
    let s = app_id.as_str();
    let tail = s.rsplit(['/', '\\', ':']).next().unwrap_or(s);
    let tail = tail.strip_suffix(".exe").unwrap_or(tail);
    if tail.is_empty() {
        s.to_owned()
    } else {
        tail.to_owned()
    }
}

fn worker_loop(rx: &Receiver<EngineInput>, inner: &std::sync::Weak<Inner>) {
    for input in rx {
        let Some(inner) = inner.upgrade() else { break };
        inner.process(input);
    }
    log::debug!("stem-service worker stopped");
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use crossbeam_channel::Receiver;
    use pretty_assertions::assert_eq;
    use stem_core::{DockItemKind, WindowId};
    use stem_platform::mock::{MockCall, MockPlatform};

    use super::*;

    fn start() -> (DockService, Arc<MockPlatform>, Receiver<ServiceEvent>) {
        let platform = Arc::new(MockPlatform::with_demo_scenario());
        let store = Box::new(MemorySettingsStore::default());
        let service = DockService::start(platform.clone(), store).unwrap();
        let (tx, rx) = crossbeam_channel::unbounded();
        service.subscribe(Box::new(move |ev| {
            let _ = tx.send(ev.clone());
        }));
        (service, platform, rx)
    }

    /// Wait until a state event satisfying `pred` arrives.
    fn wait_state(rx: &Receiver<ServiceEvent>, pred: impl Fn(&DockState) -> bool) -> DockState {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if let Ok(ServiceEvent::State(s)) = rx.recv_timeout(Duration::from_millis(200))
                && pred(&s)
            {
                return s;
            }
        }
        panic!("timed out waiting for state");
    }

    fn app_items(state: &DockState) -> Vec<&stem_core::DockItem> {
        state
            .items
            .iter()
            .filter(|i| i.kind == DockItemKind::App)
            .collect()
    }

    #[test]
    fn startup_resolves_identities_and_groups_windows() {
        let (service, _platform, rx) = start();
        let state = wait_state(&rx, |s| {
            app_items(s).len() == 5 && app_items(s).iter().all(|i| i.icon_key.is_some())
        });
        let firefox = app_items(&state)
            .into_iter()
            .find(|i| i.title == "firefox")
            .unwrap();
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

    #[test]
    fn primary_click_minimises_active_and_activates_inactive() {
        let (service, platform, rx) = start();
        let state = wait_state(&rx, |s| {
            app_items(s).len() == 5 && app_items(s).iter().all(|i| i.icon_key.is_some())
        });
        let firefox_key = app_items(&state)
            .into_iter()
            .find(|i| i.title == "firefox")
            .unwrap()
            .key
            .clone();
        let code_key = app_items(&state)
            .into_iter()
            .find(|i| i.title == "Code")
            .unwrap()
            .key
            .clone();

        // Firefox has two windows and the first is active -> cycle to the second.
        service.primary_click(&firefox_key).unwrap();
        // Code has one inactive window -> activate.
        service.primary_click(&code_key).unwrap();
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

        // Now Code is active with a single window -> minimise.
        platform.clear_calls();
        service.primary_click(&code_key).unwrap();
        assert_eq!(
            platform.calls(),
            vec![MockCall::WindowAction(WindowId(4), WindowAction::Minimize)]
        );

        // Launcher.
        service.primary_click("fixed:launcher").unwrap();
        assert_eq!(
            platform.calls().last(),
            Some(&MockCall::Shell(ShellAction::OpenStartMenu))
        );
        assert!(matches!(
            service.primary_click("bogus"),
            Err(ServiceError::UnknownItem(_))
        ));
    }

    #[test]
    fn pin_unpin_persist_and_launch_bounces() {
        let (service, platform, rx) = start();
        let state = wait_state(&rx, |s| {
            app_items(s).len() == 5 && app_items(s).iter().all(|i| i.icon_key.is_some())
        });
        let spotify = app_items(&state)
            .into_iter()
            .find(|i| i.title == "Spotify")
            .unwrap()
            .clone();
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
        service.primary_click(&spotify.key).unwrap();
        assert!(matches!(
            platform.calls().as_slice(),
            [MockCall::Launch(LaunchTarget::Exe { .. })]
        ));
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut bounced = false;
        while std::time::Instant::now() < deadline {
            if let Ok(ServiceEvent::Bounce { item_key }) =
                rx.recv_timeout(Duration::from_millis(100))
            {
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
        let store = MemorySettingsStore::with_document(
            r#"{"iconSize": 1000, "items": {"launcher": false}}"#,
        );
        let service = DockService::start(platform, Box::new(store)).unwrap();
        assert_eq!(service.settings().icon_size, 128);
        assert!(!service.settings().items.launcher);

        let mut s = service.settings();
        s.icon_size = 64;
        let saved = service.update_settings(s).unwrap();
        assert_eq!(saved.icon_size, 64);
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
}
