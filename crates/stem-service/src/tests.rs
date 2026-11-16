//! The service against the mock platform. The shared helpers and fixtures
//! are here; each submodule tests one topic.

mod commands;
mod docks;
mod hidden_apps;
mod identity;
mod scroll;
mod settings;
mod stale_pins;
mod volume;
mod worker;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use stem_core::{
    AppIdentity, DockItem, DockItemKind, IconKey, ItemAction, LaunchTarget, MonitorId,
    PlatformEvent, RawWindow, Rect, RecycleBinState, ScrollDirection, ShellAction, VolumeStatus,
    WindowAction, WindowId,
};
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

fn hidden_ids(settings: &Settings) -> Vec<&str> {
    settings.hidden_apps.iter().map(|h| h.id.as_str()).collect()
}

/// The demo monitor's id (`demo_monitor` in the mock platform).
const DEMO_MONITOR: &str = "mock-primary";

fn demo_monitor_id() -> MonitorId {
    MonitorId(DEMO_MONITOR.into())
}

const TERMINAL_EXE: &str = r"C:\Program Files\WindowsApps\Microsoft.WindowsTerminal_1.24.12741.0_x64__8wekyb3d8bbwe\WindowsTerminal.exe";
const TERMINAL_AUMID: &str = "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App";

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
        on_current_desktop: Some(true),
        monitor: Some(stem_core::MonitorId("mock-primary".into())),
        rect: None,
        icon_key: None,
    }
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}
