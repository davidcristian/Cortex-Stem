//! Start and shutdown of the service: the settings load, the worker and
//! identity threads, the platform, and the listeners.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use parking_lot::Mutex;
use stem_core::{DockEngine, EngineInput, PlatformEvent, Settings};
use stem_platform::Platform;

use crate::resolver::resolver_loop;
use crate::worker::worker_loop;
use crate::{
    DiskIconCache, DockService, Inner, Listener, ResolveMsg, ScrollGate, ServiceError,
    ServiceOptions, ServiceResult, SettingsStore, WorkerMsg,
};

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
    ///
    /// [`icon_cache::SHUTDOWN_WAIT`]: crate::icon_cache::SHUTDOWN_WAIT
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
