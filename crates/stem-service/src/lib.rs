//! `stem-service` — the application logic between the Tauri shell and the OS.
//!
//! It owns the engine, drives it from platform events on a worker thread,
//! resolves app identities, persists settings, and implements the *semantics*
//! of every user command (`primary_click`, `pin_app`, ...). The Tauri crate
//! is a thin adapter over this API, which keeps all of this testable on any
//! host with [`stem_platform::mock::MockPlatform`].
//!
//! Threading model:
//! - the platform delivers events on its own thread into an unbounded channel
//!   of `WorkerMsg`s;
//! - one **worker thread** (`stem-service`) drains *everything* that is
//!   queued, applies it to the engine, and then notifies listeners once.
//!   `State` notifications are rate-limited to one per
//!   [`STATE_EMIT_INTERVAL`]; the last snapshot is always delivered;
//! - one **identity thread** (`dock-identity`) asks the platform who an app
//!   is (icon extraction and shortcut resolution can take milliseconds), so
//!   the worker never waits on the OS. A failed resolution is retried once
//!   after [`IDENTITY_RETRY_DELAY`], then a fallback identity is used so the
//!   engine stops asking. When there is nothing to ask (the app has no
//!   window and no pin any more) the engine is told `IdentityUnavailable`
//!   instead, so a later appearance requests the identity again. Before its
//!   first request it re-identifies stale `exe:` pins of packaged apps once
//!   (`reidentify`), so the rewritten pins are saved and in the
//!   engine before the identities of the old ones are looked up;
//! - the **settings** have one authoritative copy (`Inner::settings`). Every
//!   mutation goes through `Inner::mutate_settings`, which validates,
//!   persists and hands the engine the new document under one lock, so two
//!   quick commands never lose each other's change and two saves never
//!   race on the settings file. The engine's own copy follows
//!   asynchronously through the worker;
//! - commands are called from any thread; they take the engine lock briefly
//!   and perform platform calls without holding it;
//! - listeners run on whichever thread produced the event (the worker for
//!   state, settings, theme and monitors; the caller for `Bounce`). A
//!   panicking listener is logged and isolated; it does not stop the worker.
//!
//! [`DockService::shutdown`] stops the platform, sends `Shutdown` to both
//! threads and joins them. The worker flushes the inputs that were queued
//! before the shutdown message and emits a final `State` if anything changed.

#![forbid(unsafe_code)]

mod reidentify;
pub mod store;

use std::collections::VecDeque;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use parking_lot::Mutex;
use stem_core::{
    AppId, AppIdentity, DockEngine, DockState, EngineInput, IconKey, ItemAction, LaunchTarget,
    MonitorInfo, PlatformEvent, Settings, SettingsError, ShellAction, ThemeInfo, WindowAction,
    WindowId,
};
use stem_platform::{Platform, PlatformError};
use thiserror::Error;

pub use store::{FileSettingsStore, MemorySettingsStore, SettingsStore, StoreError};

/// Minimum spacing between two `State` notifications (≈ 60 per second).
/// Inputs that arrive inside the window are drained and folded into the
/// next snapshot, which is always emitted once the window has passed.
pub const STATE_EMIT_INTERVAL: Duration = Duration::from_millis(16);

/// Delay before the single retry of a failed identity resolution.
pub const IDENTITY_RETRY_DELAY: Duration = Duration::from_secs(2);

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

impl ServiceEvent {
    fn name(&self) -> &'static str {
        match self {
            Self::State(_) => "state",
            Self::Settings(_) => "settings",
            Self::Theme(_) => "theme",
            Self::Monitors(_) => "monitors",
            Self::Bounce { .. } => "bounce",
        }
    }
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
    #[error("could not start the {0} thread: {1}")]
    Thread(&'static str, #[source] std::io::Error),
}

pub type ServiceResult<T> = Result<T, ServiceError>;

/// What the worker thread receives.
enum WorkerMsg {
    Input(EngineInput),
    Shutdown,
}

/// What the identity thread receives.
enum ResolveMsg {
    Resolve(AppId),
    Shutdown,
}

struct Inner {
    platform: Arc<dyn Platform>,
    store: Box<dyn SettingsStore>,
    /// The authoritative settings document; see `mutate_settings`. Never
    /// locked while `engine` is held (the other order is fine).
    settings: Mutex<Settings>,
    engine: Mutex<DockEngine>,
    /// Shared so `notify` can call listeners without holding the lock
    /// (a listener may call back into the service).
    listeners: Mutex<Vec<Arc<Listener>>>,
    worker_tx: Sender<WorkerMsg>,
    resolve_tx: Sender<ResolveMsg>,
    /// Set by `shutdown`; the identity thread stops before its next
    /// platform call instead of working through its queue.
    stopping: AtomicBool,
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
    resolver: Mutex<Option<JoinHandle<()>>>,
}

impl DockService {
    /// Load settings, start the threads and the platform, and begin
    /// processing events.
    pub fn start(
        platform: Arc<dyn Platform>,
        store: Box<dyn SettingsStore>,
    ) -> ServiceResult<Self> {
        let settings = match store.load()? {
            Some(json) => Settings::from_json(&json)?,
            None => Settings::default(),
        };
        log::info!("settings loaded from {}", store.location());

        let (worker_tx, worker_rx) = crossbeam_channel::unbounded::<WorkerMsg>();
        let (resolve_tx, resolve_rx) = crossbeam_channel::unbounded::<ResolveMsg>();
        let inner = Arc::new(Inner {
            platform: Arc::clone(&platform),
            store,
            settings: Mutex::new(settings.clone()),
            engine: Mutex::new(DockEngine::new(settings)),
            listeners: Mutex::new(Vec::new()),
            worker_tx,
            resolve_tx,
            stopping: AtomicBool::new(false),
        });

        let worker = {
            let weak = Arc::downgrade(&inner);
            spawn_thread("stem-service", move || worker_loop(&worker_rx, &weak))?
        };
        let service = Self {
            inner,
            worker: Mutex::new(Some(worker)),
            resolver: Mutex::new(None),
        };
        let resolver = {
            let weak = Arc::downgrade(&service.inner);
            spawn_thread("dock-identity", move || resolver_loop(&resolve_rx, &weak))
        };
        match resolver {
            Ok(handle) => *service.resolver.lock() = Some(handle),
            Err(e) => {
                service.shutdown();
                return Err(e);
            }
        }

        let sink_tx = service.inner.worker_tx.clone();
        let started = platform.start(Arc::new(move |ev: PlatformEvent| {
            // The worker may already be gone during shutdown; dropping the
            // event is the right thing then.
            let _ = sink_tx.send(WorkerMsg::Input(EngineInput::Platform(ev)));
        }));
        if let Err(e) = started {
            service.shutdown();
            return Err(e.into());
        }
        Ok(service)
    }

    /// Register a listener. Listeners run on the worker thread (or on the
    /// thread that called a command, for `Bounce`); keep them cheap and never
    /// block in them. A panic inside a listener is logged and contained.
    ///
    /// Because [`DockService::shutdown`] joins the worker, the thread that
    /// shuts down must not hold a lock a listener may need: a listener could
    /// still be running for the last event.
    pub fn subscribe(&self, listener: Listener) {
        self.inner.listeners.lock().push(Arc::new(listener));
    }

    pub fn platform(&self) -> &Arc<dyn Platform> {
        &self.inner.platform
    }

    pub fn state(&self) -> DockState {
        self.inner.engine.lock().snapshot()
    }

    /// The authoritative settings: what the last successful mutation
    /// produced, which the engine may not have applied yet.
    pub fn settings(&self) -> Settings {
        self.inner.settings.lock().clone()
    }

    pub fn theme(&self) -> ThemeInfo {
        self.inner.engine.lock().theme().clone()
    }

    pub fn monitors(&self) -> Vec<MonitorInfo> {
        self.inner.engine.lock().monitors().to_vec()
    }

    /// Validate, persist, and apply new settings. Returns the validated copy.
    pub fn update_settings(&self, settings: Settings) -> ServiceResult<Settings> {
        self.inner.mutate_settings(|current| {
            *current = settings;
            true
        })
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
        let launch = self
            .inner
            .engine
            .lock()
            .identity(app_id)
            .and_then(|i| i.launch.clone())
            .ok_or_else(|| ServiceError::NotLaunchable(app_id.clone()))?;
        self.inner.mutate_settings(|settings| {
            settings.pin(stem_core::settings::PinnedApp {
                id: app_id.clone(),
                label: None,
                launch,
            })
        })?;
        Ok(())
    }

    /// Pin something the user dropped or chose: a path, a shortcut, a Store app.
    /// Returns the canonical id it was pinned under.
    pub fn pin_target(&self, target: LaunchTarget) -> ServiceResult<AppId> {
        let identity = self.inner.platform.identify_target(&target)?;
        let app_id = identity.id.clone();
        let launch = identity.launch.clone().unwrap_or(target);
        let inner = &self.inner;
        inner.mutate_settings(|settings| {
            if !settings.pin(stem_core::settings::PinnedApp {
                id: app_id.clone(),
                label: None,
                launch,
            }) {
                return false;
            }
            // Make the identity known before the new item renders: this is
            // queued ahead of the `SettingsChanged` that `mutate_settings`
            // sends on the same channel.
            let _ = inner
                .worker_tx
                .send(WorkerMsg::Input(EngineInput::IdentityResolved {
                    app_id: app_id.clone(),
                    identity,
                }));
            true
        })?;
        Ok(app_id)
    }

    pub fn unpin_app(&self, app_id: &AppId) -> ServiceResult<()> {
        let mut was_pinned = false;
        self.inner.mutate_settings(|settings| {
            was_pinned = settings.unpin(app_id);
            was_pinned
        })?;
        if was_pinned {
            Ok(())
        } else {
            Err(ServiceError::NotPinned(app_id.clone()))
        }
    }

    pub fn reorder_pinned(&self, order: &[AppId]) -> ServiceResult<()> {
        self.inner.mutate_settings(|settings| {
            let before = settings.pinned.clone();
            settings.reorder_pinned(order);
            settings.pinned != before
        })?;
        Ok(())
    }

    pub fn icon_png(&self, key: &IconKey, size: u32) -> ServiceResult<Option<Vec<u8>>> {
        Ok(self.inner.platform.icon_png(key, size)?)
    }

    /// Stop the platform and both threads, and wait for them. Inputs queued
    /// before this call are still applied and a final `State` is emitted if
    /// they changed anything. Idempotent; called on drop as well.
    ///
    /// Blocks until listeners that are currently running have returned, so
    /// do not call it while holding anything a listener waits for.
    pub fn shutdown(&self) {
        self.inner.stopping.store(true, Ordering::SeqCst);
        self.inner.platform.stop();
        let _ = self.inner.resolve_tx.send(ResolveMsg::Shutdown);
        let _ = self.inner.worker_tx.send(WorkerMsg::Shutdown);
        join_thread("dock-identity", &self.resolver);
        join_thread("stem-service", &self.worker);
    }
}

impl Drop for DockService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn spawn_thread(
    name: &'static str,
    body: impl FnOnce() + Send + 'static,
) -> ServiceResult<JoinHandle<()>> {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(body)
        .map_err(|e| ServiceError::Thread(name, e))
}

fn join_thread(name: &str, slot: &Mutex<Option<JoinHandle<()>>>) {
    let handle = slot.lock().take();
    if let Some(handle) = handle
        && handle.join().is_err()
    {
        log::error!("{name} thread panicked");
    }
}

impl Inner {
    /// Call every listener with `event`. A panicking listener is logged and
    /// skipped; the others still run.
    fn notify(&self, event: &ServiceEvent) {
        let listeners: Vec<Arc<Listener>> = self.listeners.lock().clone();
        for listener in listeners {
            if let Err(payload) = std::panic::catch_unwind(AssertUnwindSafe(|| listener(event))) {
                log::error!(
                    "a service listener panicked on the {} event: {}",
                    event.name(),
                    panic_message(payload.as_ref())
                );
            }
        }
    }

    /// The one way settings change. `mutate` edits a copy of the current
    /// document and says whether it changed anything; if so the copy is
    /// migrated, validated, persisted and handed to the engine (which then
    /// notifies `Settings` listeners). All of it happens under the settings
    /// lock, so mutations and saves are serialised: a second call always
    /// sees the first one's result, and `settings.json.tmp` has one writer.
    /// Returns the document as it stands afterwards.
    ///
    /// A failed save leaves the authoritative copy untouched (the caller
    /// gets the store error), so memory and disk never disagree.
    fn mutate_settings(
        &self,
        mutate: impl FnOnce(&mut Settings) -> bool,
    ) -> ServiceResult<Settings> {
        let mut current = self.settings.lock();
        let mut next = current.clone();
        if !mutate(&mut next) {
            return Ok(next);
        }
        next.migrate();
        next.validate();
        self.store.save(&next.to_json_pretty())?;
        *current = next.clone();
        let _ = self
            .worker_tx
            .send(WorkerMsg::Input(EngineInput::SettingsChanged(next.clone())));
        Ok(next)
    }

    /// The launch target of a pinned app (from the authoritative settings),
    /// else what its identity says.
    fn pinned_launch_target(&self, app_id: &AppId) -> Option<LaunchTarget> {
        self.settings
            .lock()
            .pinned
            .iter()
            .find(|p| &p.id == app_id)
            .map(|p| p.launch.clone())
    }

    fn launch_target_for(&self, app_id: &AppId) -> Option<LaunchTarget> {
        self.pinned_launch_target(app_id).or_else(|| {
            self.engine
                .lock()
                .identity(app_id)
                .and_then(|i| i.launch.clone())
        })
    }

    /// Apply a batch of inputs, hand new identity requests to the identity
    /// thread, and notify listeners of theme / monitor / settings changes
    /// (the last value of each per batch). Returns whether the snapshot
    /// changed; the caller decides when to emit it.
    fn process_batch(&self, inputs: Vec<EngineInput>) -> bool {
        let mut changed = false;
        let mut settings_changed = false;
        let mut theme = None;
        let mut monitors = None;
        let mut unresolved = Vec::new();

        for input in inputs {
            match &input {
                EngineInput::Platform(PlatformEvent::ThemeChanged(t)) => theme = Some(t.clone()),
                EngineInput::Platform(PlatformEvent::MonitorsChanged(m)) => {
                    monitors = Some(m.clone());
                }
                EngineInput::SettingsChanged(_) => settings_changed = true,
                _ => {}
            }
            // Lock per input so commands never wait for a whole batch.
            let outcome = self.engine.lock().apply(input);
            changed |= outcome.changed;
            unresolved.extend(outcome.unresolved_apps);
        }

        for app_id in unresolved {
            let _ = self.resolve_tx.send(ResolveMsg::Resolve(app_id));
        }
        if let Some(t) = theme {
            self.notify(&ServiceEvent::Theme(t));
        }
        if let Some(m) = monitors {
            self.notify(&ServiceEvent::Monitors(m));
        }
        if settings_changed {
            let settings = self.engine.lock().settings().clone();
            self.notify(&ServiceEvent::Settings(settings));
        }
        changed
    }

    /// Give every stale `exe:` pin of a packaged app (settings written by
    /// older builds) the packaged app's `aumid:` id and a `StoreApp` target,
    /// in place, or drop it when that id is already pinned. Runs once per
    /// start, first thing on the identity thread (never on the UI thread and
    /// never on the worker, which does not wait on the OS). Each candidate is
    /// asked about without any lock held; everything that changed is saved
    /// in one `mutate_settings`, with one info line per pin. A pin the
    /// platform cannot identify, or an error, leaves the pin as it is.
    fn reidentify_stale_pins(&self) {
        let candidates: Vec<stem_core::settings::PinnedApp> = self
            .settings
            .lock()
            .pinned
            .iter()
            .filter(|p| reidentify::is_candidate(p))
            .cloned()
            .collect();
        let mut found = Vec::new();
        let mut identities = Vec::new();
        for pin in &candidates {
            if self.stopping.load(Ordering::SeqCst) {
                return;
            }
            match self.platform.identify_target(&pin.launch) {
                Ok(identity) => match reidentify::reidentified(pin, &identity) {
                    Some(entry) => {
                        identities.push((entry.new_id(), identity));
                        found.push(entry);
                    }
                    None => log::debug!(
                        "pin {} is not a packaged app's ({}); left alone",
                        pin.id.as_str(),
                        identity.id.as_str()
                    ),
                },
                Err(e) => log::debug!(
                    "re-identifying pin {}: identify_target failed: {e}; left alone",
                    pin.id.as_str()
                ),
            }
        }
        if found.is_empty() || self.stopping.load(Ordering::SeqCst) {
            return;
        }

        let mut rewrites = Vec::new();
        let saved = self.mutate_settings(|settings| {
            rewrites = reidentify::rewrite_pins(&mut settings.pinned, &found);
            // Make the new pins' identities known before they render, as
            // `pin_target` does: queued ahead of the `SettingsChanged` that
            // `mutate_settings` sends.
            for rewrite in &rewrites {
                if let reidentify::PinRewrite::Replaced { new, .. } = rewrite
                    && let Some((_, identity)) = identities.iter().find(|(id, _)| id == new)
                {
                    let _ = self
                        .worker_tx
                        .send(WorkerMsg::Input(EngineInput::IdentityResolved {
                            app_id: new.clone(),
                            identity: identity.clone(),
                        }));
                }
            }
            !rewrites.is_empty()
        });
        match saved {
            Ok(_) => {
                for rewrite in &rewrites {
                    log::info!("{rewrite}");
                }
            }
            Err(e) => log::warn!(
                "re-identified {} stale pin(s) but could not save the settings: {e}; they stay as they were",
                rewrites.len()
            ),
        }
    }

    /// Ask the platform who `app_id` is and feed the answer to the engine.
    ///
    /// Returns `Some(due)` instead when every attempted platform call failed
    /// and `may_retry` is set: the caller schedules a retry for `due`. On the
    /// retry a fallback identity built from what the engine already knows is
    /// sent, so the engine stops asking. When there is nothing to ask (the
    /// app has neither a window nor a pin any more) the engine is told
    /// `IdentityUnavailable` instead and will ask again if the app returns.
    fn resolve_identity(&self, app_id: &AppId, may_retry: bool) -> Option<Instant> {
        let pinned = self
            .settings
            .lock()
            .pinned
            .iter()
            .find(|p| &p.id == app_id)
            .cloned();
        let window = self.engine.lock().windows_of(app_id).first().copied();

        let mut tried = false;
        let mut resolved: Option<AppIdentity> = None;
        if let Some(id) = window {
            tried = true;
            match self.platform.identify_window(id) {
                Ok(identity) => resolved = Some(identity),
                Err(e) => log::debug!("identify_window({id:?}) for {app_id:?} failed: {e}"),
            }
        }
        if resolved.is_none()
            && let Some(p) = &pinned
        {
            tried = true;
            match self.platform.identify_target(&p.launch) {
                Ok(identity) => resolved = Some(identity),
                Err(e) => {
                    log::debug!("identify_target({:?}) for {app_id:?} failed: {e}", p.launch);
                }
            }
        }
        if !tried {
            log::debug!(
                "identity of {app_id:?} not needed: the app has no window and is not pinned"
            );
            let _ = self
                .worker_tx
                .send(WorkerMsg::Input(EngineInput::IdentityUnavailable {
                    app_id: app_id.clone(),
                }));
            return None;
        }
        if resolved.is_none() && may_retry {
            log::debug!("identity of {app_id:?} unresolved; retrying in {IDENTITY_RETRY_DELAY:?}");
            return Some(Instant::now() + IDENTITY_RETRY_DELAY);
        }

        let mut identity = resolved.unwrap_or_else(|| {
            log::warn!("identity of {app_id:?} could not be resolved; using a fallback name");
            AppIdentity {
                id: app_id.clone(),
                display_name: pinned
                    .as_ref()
                    .and_then(|p| p.label.clone())
                    .unwrap_or_else(|| fallback_display_name(app_id)),
                icon_key: None,
                launch: pinned.as_ref().map(|p| p.launch.clone()),
            }
        });
        // The engine groups by the id *it* derived; keep that as the key even
        // if the platform preferred a different canonical form.
        identity.id = app_id.clone();
        if identity.launch.is_none() {
            identity.launch = pinned.map(|p| p.launch);
        }
        let _ = self
            .worker_tx
            .send(WorkerMsg::Input(EngineInput::IdentityResolved {
                app_id: app_id.clone(),
                identity,
            }));
        None
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("<non-string panic payload>")
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

/// The worker: drain everything queued, apply it, emit at most one `State`
/// per [`STATE_EMIT_INTERVAL`] (always the latest), stop on `Shutdown`.
fn worker_loop(rx: &Receiver<WorkerMsg>, inner: &Weak<Inner>) {
    let mut last_emit: Option<Instant> = None;
    // A changed snapshot is waiting for the rate limit to let it out.
    let mut state_pending = false;

    loop {
        let first = if state_pending {
            let deadline = last_emit.map_or_else(Instant::now, |t| t + STATE_EMIT_INTERVAL);
            match rx.recv_deadline(deadline) {
                Ok(msg) => Some(msg),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(msg) => Some(msg),
                Err(_) => break,
            }
        };

        // Batch: the message we waited for plus everything already queued.
        let mut inputs = Vec::new();
        let mut shutdown = false;
        let queued = first
            .into_iter()
            .chain(std::iter::from_fn(|| rx.try_recv().ok()));
        for msg in queued {
            match msg {
                WorkerMsg::Input(input) => inputs.push(input),
                WorkerMsg::Shutdown => {
                    shutdown = true;
                    break;
                }
            }
        }

        let Some(inner) = inner.upgrade() else { break };
        state_pending |= inner.process_batch(inputs);
        if state_pending {
            let now = Instant::now();
            let may_emit = shutdown || last_emit.is_none_or(|t| now >= t + STATE_EMIT_INTERVAL);
            if may_emit {
                let snapshot = inner.engine.lock().snapshot();
                inner.notify(&ServiceEvent::State(snapshot));
                last_emit = Some(now);
                state_pending = false;
            }
        }
        if shutdown {
            break;
        }
    }
    log::debug!("stem-service worker stopped");
}

/// The identity thread: re-identify stale pins once, then resolve
/// requests as they come, keep failed ones in a queue of timed retries,
/// stop on `Shutdown` or when the service stops.
fn resolver_loop(rx: &Receiver<ResolveMsg>, inner: &Weak<Inner>) {
    if let Some(inner) = inner.upgrade() {
        inner.reidentify_stale_pins();
    }
    let mut retries: VecDeque<(Instant, AppId)> = VecDeque::new();

    'run: loop {
        let msg = match retries.front() {
            Some((due, _)) => match rx.recv_deadline(*due) {
                Ok(msg) => Some(msg),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match rx.recv() {
                Ok(msg) => Some(msg),
                Err(_) => break,
            },
        };
        let Some(inner) = inner.upgrade() else { break };
        if inner.stopping.load(Ordering::SeqCst) {
            break;
        }
        match msg {
            Some(ResolveMsg::Shutdown) => break,
            Some(ResolveMsg::Resolve(app_id)) => {
                if let Some(due) = inner.resolve_identity(&app_id, true) {
                    retries.push_back((due, app_id));
                }
            }
            None => {
                let now = Instant::now();
                while retries.front().is_some_and(|(due, _)| *due <= now) {
                    if inner.stopping.load(Ordering::SeqCst) {
                        break 'run;
                    }
                    if let Some((_, app_id)) = retries.pop_front() {
                        inner.resolve_identity(&app_id, false);
                    }
                }
            }
        }
    }
    log::debug!("dock-identity resolver stopped");
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use crossbeam_channel::Receiver;
    use pretty_assertions::assert_eq;
    use stem_core::{DockItem, DockItemKind, RawWindow, Rect, RecycleBinState, WindowId};
    use stem_platform::mock::{MockCall, MockPlatform};
    use stem_platform::{DockWindowOptions, EventSink, NativeWindowHandle, PlatformResult};

    use super::*;

    fn start() -> (DockService, Arc<MockPlatform>, Receiver<ServiceEvent>) {
        let platform = Arc::new(MockPlatform::with_demo_scenario());
        let store = Box::new(MemorySettingsStore::default());
        let service = DockService::start(platform.clone(), store).unwrap();
        let rx = record(&service);
        (service, platform, rx)
    }

    /// Subscribe a listener that forwards every event to a channel.
    fn record(service: &DockService) -> Receiver<ServiceEvent> {
        let (tx, rx) = crossbeam_channel::unbounded();
        service.subscribe(Box::new(move |ev| {
            let _ = tx.send(ev.clone());
        }));
        rx
    }

    /// Wait until a state event satisfying `pred` arrives.
    fn wait_state(rx: &Receiver<ServiceEvent>, pred: impl Fn(&DockState) -> bool) -> DockState {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(ServiceEvent::State(s)) = rx.recv_timeout(Duration::from_millis(200))
                && pred(&s)
            {
                return s;
            }
        }
        panic!("timed out waiting for state");
    }

    fn app_items(state: &DockState) -> Vec<&DockItem> {
        state
            .items
            .iter()
            .filter(|i| i.kind == DockItemKind::App)
            .collect()
    }

    /// The demo scenario with every identity resolved.
    fn resolved(state: &DockState) -> bool {
        app_items(state).len() == 5 && app_items(state).iter().all(|i| i.icon_key.is_some())
    }

    fn item_titled<'a>(state: &'a DockState, title: &str) -> Option<&'a DockItem> {
        app_items(state).into_iter().find(|i| i.title == title)
    }

    fn has_window_titled(state: &DockState, title: &str) -> bool {
        state
            .items
            .iter()
            .any(|i| i.windows.iter().any(|w| w.title == title))
    }

    /// Wraps the mock so tests can inject raw events straight into the
    /// service, make `identify_window` fail or stall, and count its calls.
    struct Relay {
        mock: MockPlatform,
        sink: Mutex<Option<EventSink>>,
        flaky_window: Option<WindowId>,
        /// `identify_window(flaky_window)` fails this many more times.
        failures_left: AtomicUsize,
        identify_delay: Duration,
        identify_calls: Mutex<Vec<(WindowId, Instant)>>,
        /// Scripted `identify_target` answers for `Exe` targets, by
        /// normalised path: an identity, or `None` for a failure. Any
        /// other target gets the mock's answer.
        target_answers: Vec<(String, Option<AppIdentity>)>,
        target_calls: Mutex<Vec<LaunchTarget>>,
    }

    impl Relay {
        fn demo() -> Self {
            Self::over(MockPlatform::with_demo_scenario())
        }

        fn over(mock: MockPlatform) -> Self {
            Self {
                mock,
                sink: Mutex::new(None),
                flaky_window: None,
                failures_left: AtomicUsize::new(0),
                identify_delay: Duration::ZERO,
                identify_calls: Mutex::new(Vec::new()),
                target_answers: Vec::new(),
                target_calls: Mutex::new(Vec::new()),
            }
        }

        /// `identify_target(Exe { path })` answers `answer` (`None`: fails).
        fn answering(mut self, path: &str, answer: Option<AppIdentity>) -> Self {
            self.target_answers
                .push((stem_core::normalize_path(Path::new(path)), answer));
            self
        }

        /// How often `identify_target` was asked about the exe at `path`.
        fn target_calls_for(&self, path: &str) -> usize {
            let key = stem_core::normalize_path(Path::new(path));
            self.target_calls
                .lock()
                .iter()
                .filter(|t| {
                    matches!(t, LaunchTarget::Exe { path, .. } if stem_core::normalize_path(path) == key)
                })
                .count()
        }

        fn flaky(mut self, id: WindowId, failures: usize) -> Self {
            self.flaky_window = Some(id);
            self.failures_left = AtomicUsize::new(failures);
            self
        }

        fn slow(mut self, delay: Duration) -> Self {
            self.identify_delay = delay;
            self
        }

        /// Deliver an event to the service without touching the mock's state.
        fn emit(&self, event: PlatformEvent) {
            let sink = self.sink.lock().clone();
            if let Some(sink) = sink {
                sink(event);
            }
        }

        fn identify_calls_for(&self, id: WindowId) -> Vec<Instant> {
            self.identify_calls
                .lock()
                .iter()
                .filter(|(w, _)| *w == id)
                .map(|(_, at)| *at)
                .collect()
        }

        fn window(&self, id: WindowId) -> RawWindow {
            self.mock
                .windows()
                .unwrap()
                .into_iter()
                .find(|w| w.id == id)
                .unwrap()
        }
    }

    impl Platform for Relay {
        fn name(&self) -> &'static str {
            "relay"
        }

        fn start(&self, sink: EventSink) -> PlatformResult<()> {
            *self.sink.lock() = Some(Arc::clone(&sink));
            self.mock.start(sink)
        }

        fn stop(&self) {
            self.sink.lock().take();
            self.mock.stop();
        }

        fn windows(&self) -> PlatformResult<Vec<RawWindow>> {
            self.mock.windows()
        }

        fn foreground_window(&self) -> PlatformResult<Option<WindowId>> {
            self.mock.foreground_window()
        }

        fn window_action(&self, id: WindowId, action: WindowAction) -> PlatformResult<()> {
            self.mock.window_action(id, action)
        }

        fn identify_window(&self, id: WindowId) -> PlatformResult<AppIdentity> {
            self.identify_calls.lock().push((id, Instant::now()));
            if self.flaky_window == Some(id)
                && self
                    .failures_left
                    .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                    .is_ok()
            {
                return Err(PlatformError::Timeout);
            }
            if !self.identify_delay.is_zero() {
                std::thread::sleep(self.identify_delay);
            }
            self.mock.identify_window(id)
        }

        fn identify_target(&self, target: &LaunchTarget) -> PlatformResult<AppIdentity> {
            self.target_calls.lock().push(target.clone());
            if let LaunchTarget::Exe { path, .. } = target {
                let key = stem_core::normalize_path(path);
                if let Some((_, answer)) = self.target_answers.iter().find(|(p, _)| *p == key) {
                    return answer.clone().ok_or(PlatformError::Timeout);
                }
            }
            self.mock.identify_target(target)
        }

        fn launch(&self, target: &LaunchTarget) -> PlatformResult<()> {
            self.mock.launch(target)
        }

        fn icon_png(&self, key: &IconKey, size: u32) -> PlatformResult<Option<Vec<u8>>> {
            self.mock.icon_png(key, size)
        }

        fn monitors(&self) -> PlatformResult<Vec<MonitorInfo>> {
            self.mock.monitors()
        }

        fn theme(&self) -> PlatformResult<ThemeInfo> {
            self.mock.theme()
        }

        fn recycle_bin(&self) -> PlatformResult<RecycleBinState> {
            self.mock.recycle_bin()
        }

        fn shell_action(&self, action: ShellAction) -> PlatformResult<()> {
            self.mock.shell_action(action)
        }

        fn prepare_dock_window(
            &self,
            handle: NativeWindowHandle,
            options: DockWindowOptions,
        ) -> PlatformResult<()> {
            self.mock.prepare_dock_window(handle, options)
        }
    }

    fn start_relay(relay: Arc<Relay>) -> (DockService, Receiver<ServiceEvent>) {
        let service = DockService::start(relay, Box::new(MemorySettingsStore::default())).unwrap();
        let rx = record(&service);
        (service, rx)
    }

    /// A memory store the test keeps a handle to, so it can read back what
    /// the service persisted.
    #[derive(Debug, Default)]
    struct SharedStore(Arc<MemorySettingsStore>);

    impl SettingsStore for SharedStore {
        fn load(&self) -> Result<Option<String>, StoreError> {
            self.0.load()
        }

        fn save(&self, json: &str) -> Result<(), StoreError> {
            self.0.save(json)
        }

        fn location(&self) -> String {
            self.0.location()
        }
    }

    fn exe_target(path: &str) -> LaunchTarget {
        LaunchTarget::Exe {
            path: PathBuf::from(path),
            args: vec![],
            working_dir: None,
        }
    }

    fn pinned_ids(settings: &Settings) -> Vec<AppId> {
        settings.pinned.iter().map(|p| p.id.clone()).collect()
    }

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

    #[test]
    fn primary_click_minimises_active_and_activates_inactive() {
        let (service, platform, rx) = start();
        let state = wait_state(&rx, resolved);
        let firefox_key = item_titled(&state, "firefox").unwrap().key.clone();
        let code_key = item_titled(&state, "Code").unwrap().key.clone();

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
        service.primary_click(&spotify.key).unwrap();
        assert!(matches!(
            platform.calls().as_slice(),
            [MockCall::Launch(LaunchTarget::Exe { .. })]
        ));
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut bounced = false;
        while Instant::now() < deadline {
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

    // ----- settings mutations are serialised ----------------------------------

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
    fn invalid_settings_document_is_an_error_not_a_panic() {
        let platform = Arc::new(MockPlatform::new());
        let store = MemorySettingsStore::with_document("{ not json");
        assert!(matches!(
            DockService::start(platform, Box::new(store)),
            Err(ServiceError::Settings(_))
        ));
    }

    // ----- coalescing and robustness -----------------------------------------

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
        service.primary_click("fixed:launcher").unwrap();
        service.shutdown();
        assert!(service.worker.lock().is_none());
    }

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
        assert!(
            calls[1].duration_since(calls[0]) >= IDENTITY_RETRY_DELAY - Duration::from_millis(100)
        );
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

    // ----- stale `exe:` pins of packaged apps ---------------------------------

    const TERMINAL_EXE: &str = r"C:\Program Files\WindowsApps\Microsoft.WindowsTerminal_1.24.12741.0_x64__8wekyb3d8bbwe\WindowsTerminal.exe";
    const TERMINAL_AUMID: &str = "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App";
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

    /// What `identify_target` answers for the Terminal's exe.
    fn terminal_identity() -> AppIdentity {
        AppIdentity {
            id: AppId::for_app_user_model_id(TERMINAL_AUMID),
            display_name: "Terminal".into(),
            icon_key: Some(IconKey(format!("aumid:{TERMINAL_AUMID}"))),
            launch: Some(LaunchTarget::StoreApp {
                app_user_model_id: TERMINAL_AUMID.into(),
            }),
        }
    }

    /// A running Terminal window: package identity, exe under WindowsApps.
    fn terminal_window() -> RawWindow {
        RawWindow {
            id: WindowId(50),
            title: "PowerShell".into(),
            class_name: "CASCADIA_HOSTING_WINDOW_CLASS".into(),
            process_id: 5050,
            exe_path: Some(PathBuf::from(TERMINAL_EXE)),
            app_user_model_id: Some(TERMINAL_AUMID.into()),
            is_visible: true,
            is_minimized: false,
            is_cloaked: false,
            monitor: Some(stem_core::MonitorId("mock-primary".into())),
            icon_key: None,
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

    fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
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
}
