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

mod actions;
mod hidden_apps;
pub mod icon_cache;
mod lifecycle;
#[cfg(test)]
mod perf_tests;
mod pins;
mod queries;
mod reidentify;
mod resolver;
mod scroll;
mod settings_mutation;
pub mod store;
#[cfg(test)]
mod tests;
mod volume;
mod worker;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::Sender;
use parking_lot::Mutex;
use stem_core::{
    AppId, DockEngine, DockState, EngineInput, MonitorInfo, Settings, SettingsError, ThemeInfo,
};
use stem_platform::{Platform, PlatformError};
use thiserror::Error;

pub use actions::over_own_window;
pub use icon_cache::DiskIconCache;
pub use scroll::{SCROLL_INTERVAL, ScrollGate};
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
