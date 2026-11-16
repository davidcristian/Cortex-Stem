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
//!   and perform platform calls without holding it. `delete_to_recycle_bin`
//!   blocks for as long as the shell's dialogs are up, so its caller runs it
//!   off the UI thread; it then refreshes the Recycle Bin's state on a
//!   short-lived thread (`stem-recycle-bin-refresh`) that queues the answer
//!   for the worker;
//! - listeners run on whichever thread produced the event (the worker for
//!   state, settings, theme and monitors; the caller for `Bounce`). A
//!   panicking listener is logged and isolated; it does not stop the worker;
//! - with a cache directory ([`ServiceOptions::icon_cache_dir`])
//!   [`DockService::icon_png`] reads icons through the disk cache
//!   ([`icon_cache`]) on the caller's thread, and one **writer thread**
//!   (`stem-icon-cache`) writes and evicts its entries.
//!
//! [`DockService::shutdown`] stops the platform, sends `Shutdown` to both
//! threads and joins them. The worker flushes the inputs that were queued
//! before the shutdown message and emits a final `State` if anything changed.
//! The icon cache's writer then finishes its queue and stops, waited for at
//! most [`icon_cache::SHUTDOWN_WAIT`].

#![forbid(unsafe_code)]

pub mod icon_cache;
#[cfg(test)]
mod perf_tests;
mod reidentify;
pub mod store;

use std::collections::{HashMap, VecDeque};
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use parking_lot::Mutex;
use stem_core::engine::ScrollEffect;
use stem_core::{
    AppId, AppIdentity, DockEngine, DockItem, DockState, EngineInput, IconKey, ItemAction,
    LaunchTarget, MonitorId, MonitorInfo, PlatformEvent, Rect, ScrollDirection, Settings,
    SettingsError, ShellAction, ThemeInfo, VolumeStatus, WindowAction, WindowId,
};
use stem_platform::rules::identity::{exe_icon_key, file_icon_key, is_window_icon_key};
use stem_platform::{Platform, PlatformError};
use thiserror::Error;

pub use icon_cache::DiskIconCache;
pub use store::{FileSettingsStore, MemorySettingsStore, SettingsStore, StoreError};

/// How [`DockService::start_with`] sets the service up beyond the platform
/// and the settings store. The default is what [`DockService::start`] uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServiceOptions {
    /// Directory of the disk icon cache (the app passes
    /// `<local app data>/dev.davidcristian.stem/icons`). `None`, or a
    /// directory that cannot be created, means no disk cache: every icon
    /// comes from the platform.
    pub icon_cache_dir: Option<PathBuf>,
}

/// Minimum spacing between two `State` notifications (≈ 60 per second).
/// Inputs that arrive inside the window are drained and folded into the
/// next snapshot, which is always emitted once the window has passed.
pub const STATE_EMIT_INTERVAL: Duration = Duration::from_millis(16);

/// Delay before the single retry of a failed identity resolution.
pub const IDENTITY_RETRY_DELAY: Duration = Duration::from_secs(2);

/// Shortest time between two activations the wheel over one dock window's
/// items may cause (`docs/ipc/scrolling-over-an-item.md`): a fast
/// spin moves one window per interval, not one per notch.
pub const SCROLL_INTERVAL: Duration = Duration::from_millis(150);

/// The gate of [`DockService::scroll_item`]: at most one activation
/// per dock window (by label) every [`SCROLL_INTERVAL`]. Pure: the caller
/// passes the time, so the rule is tested without waiting.
#[derive(Debug, Default)]
pub struct ScrollGate {
    /// When each dock window last let a scroll activate a window; only the
    /// ones less than [`SCROLL_INTERVAL`] ago are kept.
    last: HashMap<String, Instant>,
}

impl ScrollGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a scroll in the dock window `dock` may activate a window at
    /// `now`. Yes when the dock's last admitted one is at least
    /// [`SCROLL_INTERVAL`] old (or there is none), and `now` becomes its
    /// last; no when it is younger (a `now` before it counts as younger),
    /// and the gate is left as it was, so a dropped scroll does not push
    /// the next admitted one further away. Each dock window has its own
    /// gate. Entries that have run out are dropped on every call, so the
    /// labels of closed dock windows do not pile up.
    pub fn admit(&mut self, dock: &str, now: Instant) -> bool {
        self.last
            .retain(|_, at| now.saturating_duration_since(*at) < SCROLL_INTERVAL);
        if self.last.contains_key(dock) {
            return false;
        }
        self.last.insert(dock.to_owned(), now);
        true
    }
}

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
    #[error("app {0:?} is pinned; unpin it to hide it")]
    Pinned(AppId),
    #[error("unknown app: {0:?}")]
    UnknownApp(AppId),
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
    /// The disk icon cache in front of `platform.icon_png`, if any.
    icon_cache: Option<DiskIconCache>,
    /// `scroll_item`'s 150 ms gate per dock window.
    scroll_gate: Mutex<ScrollGate>,
    /// The platform's last volume step timed out: later timeouts
    /// log at `debug` until a step goes through, so a fast spin over a
    /// stuck volume thread writes one `warn` line, not one per notch.
    volume_stalled: AtomicBool,
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
    /// processing events, with the default [`ServiceOptions`] (no disk
    /// icon cache).
    pub fn start(
        platform: Arc<dyn Platform>,
        store: Box<dyn SettingsStore>,
    ) -> ServiceResult<Self> {
        Self::start_with(platform, store, ServiceOptions::default())
    }

    /// [`DockService::start`] with `options`. The settings are loaded (and
    /// may fail) before anything else is set up, so a settings error leaves
    /// nothing running and a second attempt is safe.
    pub fn start_with(
        platform: Arc<dyn Platform>,
        store: Box<dyn SettingsStore>,
        options: ServiceOptions,
    ) -> ServiceResult<Self> {
        let settings = match store.load()? {
            Some(json) => Settings::from_json(&json)?,
            None => Settings::default(),
        };
        log::info!("settings loaded from {}", store.location());
        let icon_cache = options
            .icon_cache_dir
            .and_then(|dir| match DiskIconCache::open(&dir) {
                Ok(cache) => {
                    log::info!("icon cache: {}", dir.display());
                    Some(cache)
                }
                Err(e) => {
                    log::warn!(
                        "no icon cache at {} ({e}); icons are extracted again at every start",
                        dir.display()
                    );
                    None
                }
            });

        let (worker_tx, worker_rx) = crossbeam_channel::unbounded::<WorkerMsg>();
        let (resolve_tx, resolve_rx) = crossbeam_channel::unbounded::<ResolveMsg>();
        let mut engine = DockEngine::new(settings.clone());
        // Our own windows (the settings window over the dock) never count as
        // overlapping it.
        engine.set_own_process_id(Some(std::process::id()));
        let inner = Arc::new(Inner {
            platform: Arc::clone(&platform),
            store,
            settings: Mutex::new(settings),
            engine: Mutex::new(engine),
            listeners: Mutex::new(Vec::new()),
            worker_tx,
            resolve_tx,
            stopping: AtomicBool::new(false),
            icon_cache,
            scroll_gate: Mutex::new(ScrollGate::new()),
            volume_stalled: AtomicBool::new(false),
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

    /// Where the panel of the dock window `label` rests on screen, in
    /// physical pixels of the virtual desktop (the app layer
    /// converts the UI's CSS rect), with the monitor the app layer placed
    /// the window on; `rect` is `None` while the window is hidden, and
    /// `monitor` and `rect` both `None` once the window is gone (the engine
    /// forgets the label). Queued for the worker like a platform event, so
    /// it is applied in order with the window updates it is compared
    /// against; the engine recomputes that dock's `overlapped` and a `State`
    /// follows only when its facts change. Never blocks.
    pub fn set_dock_panel(&self, label: &str, monitor: Option<MonitorId>, rect: Option<Rect>) {
        log::debug!("dock panel of {label:?} on {monitor:?}: {rect:?}");
        let _ = self
            .inner
            .worker_tx
            .send(WorkerMsg::Input(EngineInput::DockPanelChanged {
                label: label.to_owned(),
                monitor,
                rect,
            }));
    }

    /// Validate, persist, and apply new settings. Returns the validated copy.
    ///
    /// The pinned and hidden apps are kept as they are, whatever `settings`
    /// says (`docs/ipc/`, v1.26): only their own commands (`pin_app`,
    /// `pin_target`, `unpin_app`, `reorder_pinned`, `hide_app`,
    /// `unhide_app`) and [`Self::reset_settings`] change them, so a document
    /// the settings page built before a pin or a hide from the dock cannot
    /// undo it when it arrives after them.
    pub fn update_settings(&self, settings: Settings) -> ServiceResult<Settings> {
        self.inner.mutate_settings(|current| {
            let pinned = std::mem::take(&mut current.pinned);
            let hidden_apps = std::mem::take(&mut current.hidden_apps);
            *current = settings;
            current.pinned = pinned;
            current.hidden_apps = hidden_apps;
            true
        })
    }

    /// Restore and persist the defaults, the pinned and hidden apps
    /// included (they are empty there). Returns the validated copy.
    pub fn reset_settings(&self) -> ServiceResult<Settings> {
        self.inner.mutate_settings(|current| {
            *current = Settings::default();
            true
        })
    }

    /// Change the settings in place, under the lock every settings change
    /// takes, then validate, persist and apply them; returns the validated
    /// copy. For an edit of one field (the tray's click-through toggle) that
    /// must not write back a copy read before another change landed.
    pub fn modify_settings(&self, edit: impl FnOnce(&mut Settings)) -> ServiceResult<Settings> {
        self.inner.mutate_settings(|current| {
            edit(current);
            true
        })
    }

    /// The dock's "click" on `item_key` in the dock window `dock` (its
    /// label): launch, activate, minimise, or run a shell action. Only the
    /// windows that dock lists take part (`DockEngine::primary_action`):
    /// with a dock per monitor and `showOnlyCurrentMonitor` on, the
    /// ones on its own monitor.
    pub fn primary_click(&self, item_key: &str, dock: &str) -> ServiceResult<()> {
        self.primary_click_over(item_key, dock, || false)
    }

    /// [`Self::primary_click`] for a click while one of Stem's own windows
    /// other than a dock may be in front (the settings window):
    /// `own_window_in_front` says whether one is, and is asked only when the
    /// click would minimise ([`over_own_window`]). The app layer can tell;
    /// the engine cannot, since it never takes Stem's own windows for the
    /// foreground.
    pub fn primary_click_over(
        &self,
        item_key: &str,
        dock: &str,
        own_window_in_front: impl FnOnce() -> bool,
    ) -> ServiceResult<()> {
        let action = self.inner.engine.lock().primary_action(item_key, dock);
        let action = over_own_window(action, own_window_in_front);
        log::debug!("primary click on {item_key} in {dock:?}: {action:?}");
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

    /// One wheel notch `direction` over `item_key` in the dock window `dock`
    /// (its label; `docs/ipc/scrolling-over-an-item.md`). With
    /// `behavior.scrollAction = cycleWindows` and an app item with windows
    /// on that dock, the engine picks the next or previous one
    /// (`DockEngine::scroll_action`) and it is activated
    /// (`WindowAction::Activate`, which restores a minimised one), unless
    /// that dock window's gate ([`ScrollGate`]) let one through less than
    /// [`SCROLL_INTERVAL`] ago: then the notch is dropped. The engine is
    /// told which window the notch tried (`note_scroll_activation`), so
    /// the dock's next notch goes on from it even when Windows refused it
    /// or its foreground report is late. Returns the window activated, or
    /// `None` when the notch did nothing (the setting is `none`, a fixed
    /// item, a separator, no windows there, dropped by the gate). An
    /// unknown key, a hidden app's among them, is
    /// [`ServiceError::UnknownItem`], as for [`Self::primary_click`]; a
    /// failed activation is the platform's error, and still counts for the
    /// gate and as the window tried.
    ///
    /// Over the volume item a notch steps the volume, whatever
    /// `behavior.scrollAction` says and without the gate: every notch
    /// counts ([`Self::step_volume`]); it returns `None` (no window was
    /// activated). It waits on the platform's volume thread, bounded: call
    /// it off the UI thread and off an async runtime's workers.
    pub fn scroll_item(
        &self,
        item_key: &str,
        dock: &str,
        direction: ScrollDirection,
    ) -> ServiceResult<Option<WindowId>> {
        self.scroll_item_at(item_key, dock, direction, Instant::now())
    }

    /// [`Self::scroll_item`] at the time `now` (tests pass their own).
    fn scroll_item_at(
        &self,
        item_key: &str,
        dock: &str,
        direction: ScrollDirection,
        now: Instant,
    ) -> ServiceResult<Option<WindowId>> {
        let effect = self
            .inner
            .engine
            .lock()
            .scroll_action(item_key, dock, direction);
        let id = match effect {
            ScrollEffect::Activate(id) => id,
            ScrollEffect::StepVolume(direction) => {
                self.step_volume(direction)?;
                return Ok(None);
            }
            ScrollEffect::Nothing => {
                log::debug!("scroll {direction:?} over {item_key} in {dock:?}: nothing to do");
                return Ok(None);
            }
            ScrollEffect::UnknownItem => {
                return Err(ServiceError::UnknownItem(item_key.to_owned()));
            }
        };
        if !self.inner.scroll_gate.lock().admit(dock, now) {
            log::debug!(
                "scroll {direction:?} over {item_key} in {dock:?}: dropped, less than \
                 {SCROLL_INTERVAL:?} after the last one"
            );
            return Ok(None);
        }
        log::debug!("scroll {direction:?} over {item_key} in {dock:?}: activating {id:?}");
        // Before the platform call, so that this activation's own
        // foreground report ends it: a refused activation leaves the
        // foreground where it was, and the dock's next notch must still
        // go on from `id`.
        self.inner.engine.lock().note_scroll_activation(dock, id);
        self.inner
            .platform
            .window_action(id, WindowAction::Activate)?;
        Ok(Some(id))
    }

    /// One wheel notch over the volume item (docs/ipc/widget-items.md):
    /// `Platform::step_volume`, which reads the volume now (not the
    /// engine's last reading, which a burst of notches outruns), applies
    /// `stem_core::engine::volume_step` and sets the level, then the mute
    /// (a notch up unmutes), as one operation on the platform's side, so
    /// notches that arrive together add up. Returns the volume it set, or
    /// `None` when there was nothing to do: no output device, the end of
    /// the range, or a platform that did not answer in time. That last one
    /// drops the notch with a log line (one `warn` per streak) instead of
    /// an error, so a fast spin over a stuck volume thread does not send
    /// the page a failure per notch; the platform does not run the step
    /// later. Other platform errors are returned.
    pub fn step_volume(&self, direction: ScrollDirection) -> ServiceResult<Option<VolumeStatus>> {
        match self.inner.platform.step_volume(direction) {
            Ok(stepped) => {
                self.inner.volume_stalled.store(false, Ordering::Relaxed);
                log::debug!("volume step {direction:?}: {stepped:?}");
                Ok(stepped)
            }
            Err(PlatformError::Timeout) => {
                if self.inner.volume_stalled.swap(true, Ordering::Relaxed) {
                    log::debug!("volume step {direction:?}: dropped, the platform is still busy");
                } else {
                    log::warn!(
                        "volume step {direction:?}: the platform did not answer in time; the \
                         notch is dropped (and the next ones, logged at debug, until one goes \
                         through)"
                    );
                }
                Ok(None)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Mute or unmute the default output device (the volume item's
    /// *Mute* and *Unmute* menu entries). Fails without an output device.
    pub fn set_volume_muted(&self, muted: bool) -> ServiceResult<()> {
        Ok(self.inner.platform.set_volume_muted(muted)?)
    }

    /// The engine's last volume reading (`DockState::volume`): what
    /// the volume item's menu is built from (*Mute* or *Unmute*).
    pub fn volume(&self) -> Option<VolumeStatus> {
        self.inner.engine.lock().volume()
    }

    /// The item `item_key` as the dock window `dock` lists it, which its
    /// context menu is built from (`DockEngine::dock_item`): an app
    /// item with only the windows that dock lists. `None` for an unknown
    /// key.
    pub fn dock_item(&self, item_key: &str, dock: &str) -> Option<DockItem> {
        self.inner.engine.lock().dock_item(item_key, dock)
    }

    /// The windows of `app_id` the dock window `dock` lists, now
    /// (`DockEngine::windows_on_dock`): what its menu's *Close all
    /// windows* closes.
    pub fn windows_on_dock(&self, app_id: &AppId, dock: &str) -> Vec<WindowId> {
        self.inner.engine.lock().windows_on_dock(app_id, dock)
    }

    pub fn window_action(&self, id: WindowId, action: WindowAction) -> ServiceResult<()> {
        Ok(self.inner.platform.window_action(id, action)?)
    }

    pub fn shell_action(&self, action: ShellAction) -> ServiceResult<()> {
        Ok(self.inner.platform.shell_action(action)?)
    }

    /// Move dropped files and folders to the Recycle Bin: a file
    /// drop on its item. An empty list does nothing.
    ///
    /// Blocks while the platform does, which can include the user answering
    /// the shell's dialogs (`Platform::delete_to_recycle_bin`): call it off
    /// the UI thread. Afterwards, whatever the outcome (part of a refused
    /// batch may already be in the bin), the bin is queried once on a
    /// short-lived thread and the answer handed to the engine, so the item's
    /// icon turns "full" without waiting for the platform's next poll and
    /// without delaying this call's result by a slow query.
    pub fn delete_to_recycle_bin(&self, paths: &[String]) -> ServiceResult<()> {
        if paths.is_empty() {
            log::debug!("delete_to_recycle_bin with no paths: nothing to do");
            return Ok(());
        }
        let result = self.inner.platform.delete_to_recycle_bin(paths);
        self.inner.refresh_recycle_bin();
        Ok(result?)
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

    /// Stop showing an app on every dock (`docs/ipc/hidden-apps.md`):
    /// `{ id, name, path, iconKey }` joins `Settings::hidden_apps`,
    /// saved like a pin, and the engine drops its item when the new
    /// settings reach it. `name` is what its item shows now
    /// (`DockEngine::app_name`: the resolved display name, else its first
    /// window's title; the id's file name when that is blank); `path` and
    /// `icon_key` are what [`hidden_app_facts`] finds. Hiding a
    /// hidden app again changes nothing. Rejects a pinned app (`Pinned`)
    /// and an app the engine knows nothing of, with neither an identity nor
    /// a window (`UnknownApp`).
    pub fn hide_app(&self, app_id: &AppId) -> ServiceResult<()> {
        let mut outcome = Ok(());
        let inner = &self.inner;
        inner.mutate_settings(|settings| {
            if settings.is_pinned(app_id) {
                outcome = Err(ServiceError::Pinned(app_id.clone()));
                return false;
            }
            if settings.is_hidden(app_id) {
                return false;
            }
            // Settings, then engine: the lock order `Inner::settings` allows.
            let (name, path, icon_key) = {
                let engine = inner.engine.lock();
                let Some(name) = engine.app_name(app_id) else {
                    outcome = Err(ServiceError::UnknownApp(app_id.clone()));
                    return false;
                };
                let (path, icon_key) = hidden_app_facts(&engine, app_id);
                (name, path, icon_key)
            };
            let name = if name.trim().is_empty() {
                fallback_display_name(app_id)
            } else {
                name
            };
            log::info!("hiding {app_id:?} ({name:?}, {path:?}) from the dock");
            settings.hide(stem_core::settings::HiddenApp {
                id: app_id.clone(),
                name,
                path,
                icon_key,
            })
        })?;
        outcome
    }

    /// Show a hidden app again: its entry leaves
    /// `Settings::hidden_apps` and the settings are saved. An id that is not
    /// listed is not an error and saves nothing.
    pub fn unhide_app(&self, app_id: &AppId) -> ServiceResult<()> {
        self.inner.mutate_settings(|settings| {
            let shown = settings.unhide(app_id);
            if !shown {
                log::debug!("unhide_app: {app_id:?} is not hidden; nothing to do");
            }
            shown
        })?;
        Ok(())
    }

    pub fn reorder_pinned(&self, order: &[AppId]) -> ServiceResult<()> {
        self.inner.mutate_settings(|settings| {
            let before = settings.pinned.clone();
            settings.reorder_pinned(order);
            settings.pinned != before
        })?;
        Ok(())
    }

    /// `key`'s icon as PNG bytes whose longer edge is at most `size`, from
    /// the disk icon cache when the service has one and holds a valid entry
    /// (see [`icon_cache`] for which keys are cached and when an
    /// entry is valid), else from the platform. Blocks the calling thread
    /// for as long as the platform's extraction takes on a miss; never on
    /// a cache write.
    pub fn icon_png(&self, key: &IconKey, size: u32) -> ServiceResult<Option<Vec<u8>>> {
        let platform = &self.inner.platform;
        let png = match &self.inner.icon_cache {
            Some(cache) => cache.get_or_load(key, size, || platform.icon_png(key, size))?,
            None => platform.icon_png(key, size)?,
        };
        Ok(png)
    }

    /// Stop the platform and both threads, and wait for them. Inputs queued
    /// before this call are still applied and a final `State` is emitted if
    /// they changed anything. The icon cache's writer then works through
    /// the writes it has queued and stops, within a bound
    /// ([`icon_cache::SHUTDOWN_WAIT`]; what does not fit is dropped and
    /// misses at the next start). Icons are still served afterwards,
    /// without being written. Idempotent; called on drop as well.
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
        if let Some(cache) = &self.inner.icon_cache {
            cache.shutdown();
        }
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

    /// Ask the platform for the Recycle Bin's state on a short-lived thread
    /// (`SHQueryRecycleBinW` walks every drive and can take seconds) and
    /// queue the answer for the worker like a platform event. A duplicate of
    /// what the platform's poll reports later changes nothing: the engine
    /// keeps the latest state and bumps the revision only when the icon flips.
    fn refresh_recycle_bin(&self) {
        let platform = Arc::clone(&self.platform);
        let worker_tx = self.worker_tx.clone();
        let spawned = std::thread::Builder::new()
            .name("stem-recycle-bin-refresh".into())
            .spawn(move || match platform.recycle_bin() {
                Ok(state) => {
                    // The worker may be gone (shutdown): nothing to update then.
                    let _ = worker_tx.send(WorkerMsg::Input(EngineInput::Platform(
                        PlatformEvent::RecycleBinChanged(state),
                    )));
                }
                Err(e) => log::debug!("recycle bin refresh failed: {e}"),
            });
        if let Err(e) = spawned {
            log::debug!(
                "no recycle bin refresh (thread not started: {e}); the poll will report it"
            );
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
    /// changed; the caller decides when to emit it. "Changed" is the
    /// engine's verdict per input, by snapshot comparison, so a
    /// `WindowUpdated` that only moves a window counts exactly when the move
    /// flips some dock's `overlapped`, and a `DockPanelChanged` the
    /// same way.
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
                // The platform reads the window's facts again and derives
                // the id the engine would (`app_id_for`): another id means
                // the facts changed under the request (an AppUserModelID
                // that showed up late), so this identity is another app's
                // and must not be stored under `app_id`. The engine regroups
                // the window once it sees the new facts; until then this
                // counts as unresolved (the pin, the retry, the fallback).
                Ok(identity) if identity.id != *app_id => log::debug!(
                    "window {id:?} of {app_id:?} now identifies as {:?}; not stored under {app_id:?}",
                    identity.id
                ),
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
        // if the platform preferred a different canonical form for the pin's
        // target (a window's identity with another id never gets here).
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

/// What a dock click does while one of Stem's own windows other than a dock
/// may be in front. The engine never takes Stem's own windows for
/// the foreground, so while the settings window is in front its active
/// window is the app behind it: a click that would minimise that app as
/// the active one (`behavior.clickActiveMinimizes`) brings it forward
/// instead, as the stock taskbar does for a window that is not in front.
/// `own_window_in_front` is asked only for such a click. A dock in front
/// (the focus shortcut's session) is not one of those windows: Enter on the
/// active app's item still minimises it. Every other action is unchanged.
pub fn over_own_window(
    action: ItemAction,
    own_window_in_front: impl FnOnce() -> bool,
) -> ItemAction {
    match action {
        ItemAction::MinimizeWindow(id) if own_window_in_front() => {
            log::debug!(
                "a Stem window other than a dock is in front: {id:?}, the active window behind it, comes forward instead of minimising"
            );
            ItemAction::ActivateWindow(id)
        }
        other => other,
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("<non-string panic payload>")
}

/// What a hidden app keeps besides its name (`HiddenApp::path` and
/// `HiddenApp::icon_key`): the program's path as Windows reported it, from
/// the window that stands for the app (`DockEngine::app_window`), else the
/// file its identity launches (an `exe` or a shortcut); and the icon its
/// item showed, its identity's else that window's, as the engine picks it
/// for the item, except a key bound to one window (`hwnd:`,
/// `rules::identity::is_window_icon_key`), which is never kept: it is saved
/// for good, and once the window is gone its handle may be another
/// process's. That file's own icon (`exe:` / `file:`) stands in when the
/// path is known; else there is none. Pure.
fn hidden_app_facts(engine: &DockEngine, app_id: &AppId) -> (Option<String>, Option<IconKey>) {
    let window = engine.app_window(app_id);
    let identity = engine.identity(app_id);
    let (path, file_icon) = match window.and_then(|w| w.exe_path.as_deref()) {
        Some(exe) => {
            let exe = exe.to_string_lossy().into_owned();
            let icon = exe_icon_key(&exe);
            (Some(exe), Some(icon))
        }
        None => match identity.and_then(|i| i.launch.as_ref()) {
            Some(LaunchTarget::Exe { path, .. }) => {
                let path = path.to_string_lossy().into_owned();
                let icon = exe_icon_key(&path);
                (Some(path), Some(icon))
            }
            Some(LaunchTarget::Shortcut { path }) => {
                let path = path.to_string_lossy().into_owned();
                let icon = file_icon_key(&path);
                (Some(path), Some(icon))
            }
            _ => (None, None),
        },
    };
    let icon_key = [
        identity.and_then(|i| i.icon_key.clone()),
        window.and_then(|w| w.icon_key.clone()),
    ]
    .into_iter()
    .flatten()
    .find(|key| !is_window_icon_key(key))
    .or(file_icon);
    (path, icon_key)
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

    /// The primary monitor's dock window. Most tests never register its
    /// monitor, so its clicks consider every window.
    const DOCK: &str = "dock";

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
        /// `window_action(refused_window, Activate)` fails and changes
        /// nothing, as Windows refusing to bring an elevated window forward.
        refused_window: Option<WindowId>,
        /// Scripted `identify_window` answers, by window: the identity the
        /// platform would give after the window's facts changed.
        window_answers: Vec<(WindowId, AppIdentity)>,
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
                refused_window: None,
                window_answers: Vec::new(),
            }
        }

        /// Activating `id` fails (see `refused_window`).
        fn refusing(mut self, id: WindowId) -> Self {
            self.refused_window = Some(id);
            self
        }

        /// `identify_window(id)` answers `identity`.
        fn answering_window(mut self, id: WindowId, identity: AppIdentity) -> Self {
            self.window_answers.push((id, identity));
            self
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

        /// A volume thread that never answers in time.
        fn step_volume(&self, _direction: ScrollDirection) -> PlatformResult<Option<VolumeStatus>> {
            Err(PlatformError::Timeout)
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
            if action == WindowAction::Activate && self.refused_window == Some(id) {
                return Err(PlatformError::Other(format!(
                    "could not bring window {} to the foreground",
                    id.0
                )));
            }
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
            if let Some((_, identity)) = self.window_answers.iter().find(|(w, _)| *w == id) {
                return Ok(identity.clone());
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

        fn delete_to_recycle_bin(&self, paths: &[String]) -> PlatformResult<()> {
            self.mock.delete_to_recycle_bin(paths)
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

    // ----- hidden apps ----------------------------------------------------------

    fn hidden_ids(settings: &Settings) -> Vec<&str> {
        settings.hidden_apps.iter().map(|h| h.id.as_str()).collect()
    }

    #[test]
    fn a_hidden_app_is_saved_like_a_pin_loses_its_item_and_stays_hidden_after_a_restart() {
        let platform = Arc::new(MockPlatform::with_demo_scenario());
        let store = SharedStore::default();
        let document = Arc::clone(&store.0);
        let service = DockService::start(platform.clone(), Box::new(store)).unwrap();
        let rx = record(&service);
        let state = wait_state(&rx, resolved);
        let spotify = item_titled(&state, "Spotify").unwrap().clone();
        let spotify_id = spotify.app_id.clone().unwrap();

        service.hide_app(&spotify_id).unwrap();
        // The authoritative copy and the file both have it, with its name,
        // its program's path as the platform reported it and its icon.
        let settings = service.settings();
        assert_eq!(hidden_ids(&settings), vec![spotify_id.as_str()]);
        assert_eq!(settings.hidden_apps[0].name, "Spotify");
        let exe = platform.windows().unwrap()[5].exe_path.clone().unwrap();
        assert_eq!(
            settings.hidden_apps[0].path.as_deref(),
            Some(exe.to_string_lossy().as_ref())
        );
        assert_ne!(
            settings.hidden_apps[0].path.as_deref(),
            Some(&spotify_id.as_str()["exe:".len()..]),
            "the original case, not the id's"
        );
        assert!(spotify.icon_key.is_some());
        assert_eq!(settings.hidden_apps[0].icon_key, spotify.icon_key);
        let saved = Settings::from_json(&document.document().unwrap()).unwrap();
        assert_eq!(saved, settings);
        // The page hears of it as of any settings change, then of the state.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match rx.recv_deadline(deadline) {
                Ok(ServiceEvent::Settings(s)) if s.is_hidden(&spotify_id) => break,
                Ok(_) => {}
                Err(e) => panic!("no settings event: {e}"),
            }
        }
        let state = wait_state(&rx, |s| item_titled(s, "Spotify").is_none());
        assert_eq!(app_items(&state).len(), 4);

        // Its window is still tracked: a click on its old key does nothing,
        // and the platform is not asked to do anything.
        assert_eq!(
            service.windows_on_dock(&spotify_id, DOCK),
            vec![WindowId(6)]
        );
        platform.clear_calls();
        assert!(matches!(
            service.primary_click(&spotify.key, DOCK),
            Err(ServiceError::UnknownItem(_))
        ));
        assert_eq!(platform.calls(), vec![]);
        assert_eq!(service.dock_item(&spotify.key, DOCK), None);

        // A second window of it brings no item either.
        let mut second = platform.windows().unwrap()[5].clone();
        assert_eq!(second.id, WindowId(6));
        second.id = WindowId(60);
        second.title = "Spotify mini player".into();
        platform.push(PlatformEvent::WindowAdded(second));
        wait_until("the second window", || {
            service.windows_on_dock(&spotify_id, DOCK) == [WindowId(6), WindowId(60)]
        });
        assert!(item_titled(&service.state(), "Spotify").is_none());
        assert_eq!(service.state().revision, state.revision, "nothing to show");

        // Hiding it again saves nothing.
        let before = document.document();
        service.hide_app(&spotify_id).unwrap();
        assert_eq!(document.document(), before);
        assert_eq!(hidden_ids(&service.settings()), vec![spotify_id.as_str()]);
        service.shutdown();

        // A restart reads the list back: still no item.
        let restarted = DockService::start(
            Arc::new(MockPlatform::with_demo_scenario()),
            Box::new(MemorySettingsStore::with_document(&before.unwrap())),
        )
        .unwrap();
        let rx = record(&restarted);
        assert!(restarted.settings().is_hidden(&spotify_id));
        let state = wait_state(&rx, |s| {
            app_items(s).len() == 4 && app_items(s).iter().all(|i| i.icon_key.is_some())
        });
        assert!(item_titled(&state, "Spotify").is_none());

        // Shown again: the item comes back; showing it twice is no error.
        restarted.unhide_app(&spotify_id).unwrap();
        assert!(restarted.settings().hidden_apps.is_empty());
        wait_state(&rx, |s| item_titled(s, "Spotify").is_some());
        restarted.unhide_app(&spotify_id).unwrap();
        restarted.shutdown();
    }

    #[test]
    fn a_pinned_or_unknown_app_cannot_be_hidden_and_a_pin_shows_a_hidden_app() {
        let platform = Arc::new(MockPlatform::with_demo_scenario());
        let store = SharedStore::default();
        let document = Arc::clone(&store.0);
        let service = DockService::start(platform, Box::new(store)).unwrap();
        let rx = record(&service);
        let state = wait_state(&rx, resolved);
        let code = item_titled(&state, "Code").unwrap().app_id.clone().unwrap();

        // Pinned (in the authoritative copy, before the engine has it).
        service.pin_app(&code).unwrap();
        let before = document.document();
        let err = service.hide_app(&code).unwrap_err();
        assert!(matches!(&err, ServiceError::Pinned(id) if *id == code));
        assert!(err.to_string().contains("is pinned"), "{err}");
        assert_eq!(document.document(), before, "nothing saved");
        assert!(service.settings().hidden_apps.is_empty());

        // Unknown: no window, no identity.
        let nobody = AppId::for_exe(Path::new(r"C:\Apps\nobody.exe"));
        assert!(matches!(
            service.hide_app(&nobody),
            Err(ServiceError::UnknownApp(id)) if id == nobody
        ));
        assert_eq!(document.document(), before);
        // Showing an app that is not hidden is no error and saves nothing.
        service.unhide_app(&nobody).unwrap();
        assert_eq!(document.document(), before);

        // Pinning a hidden app drops it from the list (validation), so it
        // is shown as a pin.
        let terminal = item_titled(&state, "WindowsTerminal")
            .unwrap()
            .app_id
            .clone()
            .unwrap();
        service.hide_app(&terminal).unwrap();
        assert!(service.settings().is_hidden(&terminal));
        service.pin_app(&terminal).unwrap();
        let settings = service.settings();
        assert!(settings.is_pinned(&terminal));
        assert!(settings.hidden_apps.is_empty());
        wait_state(&rx, |s| {
            app_items(s)
                .iter()
                .any(|i| i.app_id.as_ref() == Some(&terminal) && i.pinned)
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
        service.primary_click("fixed:launcher", DOCK).unwrap();
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

    // ----- the platform's fullscreen verdict, per dock -------------------------

    /// The demo monitor's id (`demo_monitor` in the mock platform).
    const DEMO_MONITOR: &str = "mock-primary";

    fn demo_monitor_id() -> MonitorId {
        MonitorId(DEMO_MONITOR.into())
    }

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

    #[test]
    fn a_hidden_apps_path_and_icon_come_from_its_window_else_its_identity() {
        let mut engine = DockEngine::new(Settings::default());
        let exe = TERMINAL_EXE;
        let id = AppId::for_app_user_model_id(TERMINAL_AUMID);
        assert_eq!(hidden_app_facts(&engine, &id), (None, None));

        // A window without an icon of its own and no identity yet: its
        // path, and the program's own icon.
        let program_icon = Some(IconKey(format!("exe:{TERMINAL_EXE}")));
        engine.apply(EngineInput::Platform(PlatformEvent::WindowAdded(
            terminal_window(),
        )));
        assert_eq!(
            hidden_app_facts(&engine, &id),
            (Some(exe.into()), program_icon.clone())
        );
        // A window icon (`hwnd:`) is never kept: its handle outlives the
        // window and may be another process's later.
        let with_icon = RawWindow {
            icon_key: Some(IconKey("hwnd:50:5050".into())),
            ..terminal_window()
        };
        engine.apply(EngineInput::Platform(PlatformEvent::WindowUpdated(
            with_icon.clone(),
        )));
        assert_eq!(hidden_app_facts(&engine, &id).1, program_icon);
        // Nor is it without a path to stand in: no icon then.
        let unreadable = RawWindow {
            id: WindowId(51),
            exe_path: None,
            app_user_model_id: None,
            class_name: "ProtectedClass".into(),
            icon_key: Some(IconKey("hwnd:51:5151".into())),
            ..with_icon
        };
        let protected = stem_core::app_id_for(&unreadable);
        engine.apply(EngineInput::Platform(PlatformEvent::WindowAdded(
            unreadable,
        )));
        engine.apply(EngineInput::IdentityResolved {
            app_id: protected.clone(),
            identity: AppIdentity {
                id: protected.clone(),
                display_name: "Protected".into(),
                icon_key: Some(IconKey("hwnd:51:5151".into())),
                launch: None,
            },
        });
        assert_eq!(hidden_app_facts(&engine, &protected), (None, None));
        // The identity's icon wins, as on the item; the path stays the window's.
        engine.apply(EngineInput::IdentityResolved {
            app_id: id.clone(),
            identity: terminal_identity(),
        });
        assert_eq!(
            hidden_app_facts(&engine, &id),
            (
                Some(exe.into()),
                Some(IconKey(format!("aumid:{TERMINAL_AUMID}")))
            )
        );
        // No window left: a Store app's identity names no file.
        engine.apply(EngineInput::Platform(PlatformEvent::WindowRemoved(
            WindowId(50),
        )));
        assert_eq!(
            hidden_app_facts(&engine, &id),
            (None, Some(IconKey(format!("aumid:{TERMINAL_AUMID}"))))
        );

        // An app known only by its identity, which launches a program: that
        // program's path, as written.
        let program = Path::new(r"C:\Tools\Foo.EXE");
        let foo = AppId::for_exe(program);
        engine.apply(EngineInput::IdentityResolved {
            app_id: foo.clone(),
            identity: AppIdentity {
                id: foo.clone(),
                display_name: "Foo".into(),
                icon_key: None,
                launch: Some(LaunchTarget::Exe {
                    path: program.to_path_buf(),
                    args: vec![],
                    working_dir: None,
                }),
            },
        });
        assert_eq!(
            hidden_app_facts(&engine, &foo),
            (
                Some(r"C:\Tools\Foo.EXE".into()),
                Some(IconKey(r"exe:C:\Tools\Foo.EXE".into()))
            )
        );
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
            on_current_desktop: Some(true),
            monitor: Some(stem_core::MonitorId("mock-primary".into())),
            rect: None,
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

    // ----- scrolling over an item ---------------------------------------------------

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
                    if s.behavior.scroll_action
                        == stem_core::settings::ScrollAction::CycleWindows =>
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

    // ----- the volume item ----------------------------------------------------------

    /// The demo with the volume item on, once the engine shows it.
    fn start_with_volume() -> (DockService, Arc<MockPlatform>, Receiver<ServiceEvent>) {
        let (service, platform, rx) = start();
        let mut settings = service.settings();
        settings.items.volume = true;
        service.update_settings(settings).unwrap();
        wait_state(&rx, |s| s.items.iter().any(|i| i.key == "fixed:volume"));
        (service, platform, rx)
    }

    fn level(percent: u8, muted: bool) -> Option<VolumeStatus> {
        Some(VolumeStatus { percent, muted })
    }

    #[test]
    fn a_scroll_over_the_volume_item_steps_it_two_points_a_notch_without_the_gate() {
        use ScrollDirection::{Next, Previous};
        let (service, platform, rx) = start_with_volume();
        assert_eq!(
            service.settings().behavior.scroll_action,
            stem_core::settings::ScrollAction::None
        );
        assert_eq!(service.volume(), level(60, false));
        let t0 = Instant::now();
        // Five notches up at the same instant: each counts, from the last.
        for _ in 0..5 {
            assert_eq!(
                service
                    .scroll_item_at("fixed:volume", DOCK, Previous, t0)
                    .unwrap(),
                None
            );
        }
        assert_eq!(platform.volume_status().unwrap(), level(70, false));
        let state = wait_state(&rx, |s| s.volume == level(70, false));
        assert_eq!(
            state
                .items
                .iter()
                .find(|i| i.key == "fixed:volume")
                .map(|i| i.title.as_str()),
            Some("Volume 70 %")
        );
        // Down, on another dock window too, and the gate of app items is
        // left alone.
        service
            .scroll_item_at("fixed:volume", "dock-DISPLAY3", Next, t0)
            .unwrap();
        assert_eq!(platform.volume_status().unwrap(), level(68, false));
        // One platform step per notch, never a read-then-set of its own.
        assert_eq!(
            platform.calls(),
            [Previous, Previous, Previous, Previous, Previous, Next]
                .into_iter()
                .map(MockCall::StepVolume)
                .collect::<Vec<_>>()
        );
        assert!(service.inner.scroll_gate.lock().last.is_empty());
        service.shutdown();
    }

    #[test]
    fn a_volume_step_unmutes_going_up_clamps_and_does_nothing_without_a_device() {
        use ScrollDirection::{Next, Previous};
        let (service, platform, _rx) = start_with_volume();
        // Muted at 99: up raises to 100 and unmutes.
        platform.change_volume(level(99, true));
        assert_eq!(service.step_volume(Previous).unwrap(), level(100, false));
        assert_eq!(platform.volume_status().unwrap(), level(100, false));
        // At the top already: nothing changes.
        assert_eq!(service.step_volume(Previous).unwrap(), None);
        assert_eq!(platform.volume_status().unwrap(), level(100, false));
        // Muted at 1: down lowers to 0 and stays muted; then nothing more.
        platform.change_volume(level(1, true));
        assert_eq!(service.step_volume(Next).unwrap(), level(0, true));
        assert_eq!(service.step_volume(Next).unwrap(), None);
        assert_eq!(platform.volume_status().unwrap(), level(0, true));

        // No output device: the notch resolves and changes nothing.
        platform.change_volume(None);
        assert_eq!(
            service
                .scroll_item_at("fixed:volume", DOCK, Previous, Instant::now())
                .unwrap(),
            None
        );
        assert_eq!(platform.volume_status().unwrap(), None);
        assert!(
            platform
                .calls()
                .iter()
                .all(|c| matches!(c, MockCall::StepVolume(_)))
        );
        // The menu's Mute fails without a device; with one it is recorded.
        assert!(service.set_volume_muted(true).is_err());
        platform.change_volume(level(30, false));
        service.set_volume_muted(true).unwrap();
        assert_eq!(platform.volume_status().unwrap(), level(30, true));
        service.shutdown();
    }

    #[test]
    fn a_volume_step_the_platform_does_not_answer_in_time_is_dropped_not_an_error() {
        // The relay's volume thread never answers in time.
        let relay = Arc::new(Relay::demo());
        let (service, rx) = start_relay(Arc::clone(&relay));
        let mut settings = service.settings();
        settings.items.volume = true;
        service.update_settings(settings).unwrap();
        wait_state(&rx, |s| s.items.iter().any(|i| i.key == "fixed:volume"));
        for direction in [ScrollDirection::Previous, ScrollDirection::Next] {
            assert_eq!(
                service
                    .scroll_item_at("fixed:volume", DOCK, direction, Instant::now())
                    .unwrap(),
                None
            );
            assert_eq!(service.step_volume(direction).unwrap(), None);
        }
        assert!(service.inner.volume_stalled.load(Ordering::Relaxed));
        service.shutdown();
    }

    #[test]
    fn a_scroll_over_the_volume_item_with_its_setting_off_is_an_unknown_item() {
        let (service, platform, rx) = start();
        wait_state(&rx, resolved);
        assert!(matches!(
            service.scroll_item_at("fixed:volume", DOCK, ScrollDirection::Next, Instant::now()),
            Err(ServiceError::UnknownItem(_))
        ));
        assert_eq!(platform.calls(), vec![]);
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
}
