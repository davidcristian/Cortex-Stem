//! A scripted, deterministic platform for tests and for running the app
//! without a real desktop. Behaviour is intentionally simple and documented;
//! tests may drive it through [`MockPlatform::push`] and the setters.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use stem_core::{
    AppId, AppIdentity, IconKey, LaunchTarget, MonitorId, MonitorInfo, PlatformEvent, RawWindow,
    Rect, RecycleBinState, ShellAction, ThemeInfo, WindowAction, WindowId,
};

use crate::{
    AppBarGrant, AppBarRequest, AppBarSink, DockBackdrop, DockWindowOptions, EventSink, HitRegion,
    NativeWindowHandle, Platform, PlatformError, PlatformResult, rules,
};

#[derive(Default)]
struct State {
    windows: Vec<RawWindow>,
    foreground: Option<WindowId>,
    monitors: Vec<MonitorInfo>,
    theme: ThemeInfo,
    recycle_bin: RecycleBinState,
    /// When set, `delete_to_recycle_bin` fails with this message.
    recycle_failure: Option<String>,
    /// Windows' own *Automatically hide the taskbar*; off unless
    /// scripted or set.
    taskbar_auto_hide: bool,
    /// What `ask_yes_no` answers; *No* unless scripted.
    yes_no_answer: bool,
    /// The AppBars registered now, in the order they were first
    /// registered, with the request each holds.
    app_bars: Vec<(NativeWindowHandle, AppBarRequest)>,
    sink: Option<EventSink>,
    /// Every call made, for assertions in tests.
    calls: Vec<MockCall>,
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("windows", &self.windows)
            .field("foreground", &self.foreground)
            .field("monitors", &self.monitors)
            .field("theme", &self.theme)
            .field("recycle_bin", &self.recycle_bin)
            .field("recycle_failure", &self.recycle_failure)
            .field("taskbar_auto_hide", &self.taskbar_auto_hide)
            .field("yes_no_answer", &self.yes_no_answer)
            .field("app_bars", &self.app_bars)
            .field("started", &self.sink.is_some())
            .field("calls", &self.calls)
            .finish()
    }
}

/// A record of a side-effecting call, for test assertions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockCall {
    WindowAction(WindowId, WindowAction),
    Launch(LaunchTarget),
    /// `delete_to_recycle_bin` with these paths, as given.
    DeleteToRecycleBin(Vec<String>),
    Shell(ShellAction),
    PrepareDockWindow(NativeWindowHandle, DockWindowOptions),
    SetDockHitRegion(NativeWindowHandle, Option<HitRegion>),
    /// `set_dock_backdrop` with these arguments.
    SetDockBackdrop(NativeWindowHandle, Option<DockBackdrop>),
    /// `taskbar_auto_hide` was read; recorded so tests can check
    /// that the live state is read before it is set.
    TaskbarAutoHide,
    /// `set_taskbar_auto_hide` with this value.
    SetTaskbarAutoHide(bool),
    /// `ask_yes_no` with this title and text; the answer is the
    /// scripted one.
    AskYesNo {
        title: String,
        text: String,
    },
    /// `set_dock_app_bar` with this window and request.
    SetDockAppBar(NativeWindowHandle, Option<AppBarRequest>),
    /// `release_app_bars`.
    ReleaseAppBars,
    /// `focus_dock_window` with this window.
    FocusDockWindow(NativeWindowHandle),
}

#[derive(Debug, Default)]
pub struct MockPlatform {
    state: Mutex<State>,
}

impl MockPlatform {
    /// An empty platform: one monitor, no windows.
    pub fn new() -> Self {
        let this = Self::default();
        this.state.lock().monitors = vec![demo_monitor()];
        this
    }

    /// A platform pre-populated with a handful of fake apps and windows.
    pub fn with_demo_scenario() -> Self {
        let this = Self::new();
        {
            let mut s = this.state.lock();
            s.windows = demo_windows();
            s.foreground = Some(WindowId(2));
            s.recycle_bin = RecycleBinState {
                item_count: 3,
                size_bytes: 1_234_567,
            };
        }
        this
    }

    /// Deliver an arbitrary event to the sink (if started) and update the
    /// internal state where the event implies it.
    pub fn push(&self, event: PlatformEvent) {
        let sink = {
            let mut s = self.state.lock();
            match &event {
                PlatformEvent::WindowsSnapshot(list) => s.windows = list.clone(),
                PlatformEvent::WindowAdded(w) | PlatformEvent::WindowUpdated(w) => {
                    match s.windows.iter_mut().find(|x| x.id == w.id) {
                        Some(x) => *x = w.clone(),
                        None => s.windows.push(w.clone()),
                    }
                }
                PlatformEvent::WindowRemoved(id) => s.windows.retain(|w| w.id != *id),
                PlatformEvent::ForegroundChanged(id) => s.foreground = *id,
                PlatformEvent::MonitorsChanged(m) => s.monitors = m.clone(),
                PlatformEvent::ThemeChanged(t) => s.theme = t.clone(),
                PlatformEvent::RecycleBinChanged(r) => s.recycle_bin = *r,
                PlatformEvent::WindowAttention { .. } | PlatformEvent::FullscreenChanged { .. } => {
                }
            }
            s.sink.clone()
        };
        if let Some(sink) = sink {
            sink(event);
        }
    }

    /// Calls recorded so far (oldest first).
    pub fn calls(&self) -> Vec<MockCall> {
        self.state.lock().calls.clone()
    }

    pub fn clear_calls(&self) {
        self.state.lock().calls.clear();
    }

    /// Make every later `delete_to_recycle_bin` fail with `message` (what
    /// the shell refusing an item or the user cancelling looks like), or
    /// succeed again with `None`.
    pub fn set_recycle_failure(&self, message: Option<&str>) {
        self.state.lock().recycle_failure = message.map(str::to_owned);
    }

    /// Script Windows' own *Automatically hide the taskbar* (what the user
    /// had before Stem) without recording a call.
    pub fn script_taskbar_auto_hide(&self, enabled: bool) {
        self.state.lock().taskbar_auto_hide = enabled;
    }

    /// Script what the user answers to every later `ask_yes_no`;
    /// *No* (`false`) until this is called.
    pub fn script_yes_no_answer(&self, answer: bool) {
        self.state.lock().yes_no_answer = answer;
    }

    /// A window moved or resized: give it a new frame (`None` for a
    /// frame the platform could not read) and report it the way the events
    /// thread does after a move, with one `WindowUpdated`. Unknown ids are
    /// ignored, like a move of a window the thread does not track.
    pub fn move_window(&self, id: WindowId, rect: Option<Rect>) {
        let updated = {
            let mut s = self.state.lock();
            match s.windows.iter_mut().find(|w| w.id == id) {
                Some(w) => {
                    w.rect = rect;
                    Some(w.clone())
                }
                None => None,
            }
        };
        if let Some(raw) = updated {
            self.push(PlatformEvent::WindowUpdated(raw));
        }
    }

    /// The AppBars registered now: each window with the request
    /// its band was granted for, in the order of their first registration.
    pub fn app_bars(&self) -> Vec<(NativeWindowHandle, AppBarRequest)> {
        self.state.lock().app_bars.clone()
    }

    fn record(&self, call: MockCall) {
        self.state.lock().calls.push(call);
    }
}

/// The band the mock's shell grants for `request`: on the request's edge of
/// the work area of the scripted monitor whose bounds it names (as if the
/// stock taskbar, or another bar, held the rest), or of the bounds
/// themselves for a monitor the script does not list; cut to the request's
/// thickness by the same rule as the real platform (`rules::appbar`).
fn mock_band(monitors: &[MonitorInfo], request: &AppBarRequest) -> Rect {
    let area = monitors
        .iter()
        .find(|m| m.bounds == request.monitor)
        .map_or(request.monitor, |m| m.work_area);
    rules::appbar::set_rect(area, request)
}

impl Platform for MockPlatform {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn start(&self, sink: EventSink) -> PlatformResult<()> {
        let initial = {
            let mut s = self.state.lock();
            if s.sink.is_some() {
                return Err(PlatformError::Other("mock platform already started".into()));
            }
            s.sink = Some(Arc::clone(&sink));
            [
                PlatformEvent::MonitorsChanged(s.monitors.clone()),
                PlatformEvent::ThemeChanged(s.theme.clone()),
                PlatformEvent::RecycleBinChanged(s.recycle_bin),
                PlatformEvent::WindowsSnapshot(s.windows.clone()),
                PlatformEvent::ForegroundChanged(s.foreground),
            ]
        };
        for ev in initial {
            sink(ev);
        }
        Ok(())
    }

    fn stop(&self) {
        self.state.lock().sink = None;
    }

    fn windows(&self) -> PlatformResult<Vec<RawWindow>> {
        Ok(self.state.lock().windows.clone())
    }

    fn foreground_window(&self) -> PlatformResult<Option<WindowId>> {
        Ok(self.state.lock().foreground)
    }

    fn window_action(&self, id: WindowId, action: WindowAction) -> PlatformResult<()> {
        self.record(MockCall::WindowAction(id, action));
        let event = {
            let mut s = self.state.lock();
            let Some(idx) = s.windows.iter().position(|w| w.id == id) else {
                return Err(PlatformError::WindowNotFound(id));
            };
            match action {
                WindowAction::Activate => {
                    s.windows[idx].is_minimized = false;
                    s.foreground = Some(id);
                    vec![
                        PlatformEvent::WindowUpdated(s.windows[idx].clone()),
                        PlatformEvent::ForegroundChanged(Some(id)),
                    ]
                }
                WindowAction::Minimize => {
                    s.windows[idx].is_minimized = true;
                    if s.foreground == Some(id) {
                        s.foreground = None;
                    }
                    vec![
                        PlatformEvent::WindowUpdated(s.windows[idx].clone()),
                        PlatformEvent::ForegroundChanged(s.foreground),
                    ]
                }
                WindowAction::Restore => {
                    s.windows[idx].is_minimized = false;
                    vec![PlatformEvent::WindowUpdated(s.windows[idx].clone())]
                }
                WindowAction::Close => {
                    s.windows.remove(idx);
                    if s.foreground == Some(id) {
                        s.foreground = None;
                    }
                    vec![
                        PlatformEvent::WindowRemoved(id),
                        PlatformEvent::ForegroundChanged(s.foreground),
                    ]
                }
            }
        };
        for ev in event {
            self.push(ev);
        }
        Ok(())
    }

    fn identify_window(&self, id: WindowId) -> PlatformResult<AppIdentity> {
        let raw = self
            .state
            .lock()
            .windows
            .iter()
            .find(|w| w.id == id)
            .cloned();
        let raw = raw.ok_or(PlatformError::WindowNotFound(id))?;
        Ok(identity_for(&raw))
    }

    fn identify_target(&self, target: &LaunchTarget) -> PlatformResult<AppIdentity> {
        let (id, name) = match target {
            LaunchTarget::Exe { path, .. } | LaunchTarget::Shortcut { path } => {
                (AppId::for_exe(path), file_stem(path))
            }
            LaunchTarget::StoreApp { app_user_model_id } => (
                AppId::for_app_user_model_id(app_user_model_id),
                app_user_model_id.clone(),
            ),
            LaunchTarget::Uri { uri } => (AppId(format!("uri:{uri}")), uri.clone()),
        };
        Ok(AppIdentity {
            icon_key: Some(IconKey(format!("mock:{}", id.as_str()))),
            id,
            display_name: name,
            launch: Some(target.clone()),
        })
    }

    fn launch(&self, target: &LaunchTarget) -> PlatformResult<()> {
        self.record(MockCall::Launch(target.clone()));
        Ok(())
    }

    fn icon_png(&self, key: &IconKey, size: u32) -> PlatformResult<Option<Vec<u8>>> {
        if key.is_shell() {
            return Ok(None);
        }
        Ok(Some(solid_png(
            size.clamp(8, 256),
            color_for(key.0.as_str()),
        )))
    }

    fn monitors(&self) -> PlatformResult<Vec<MonitorInfo>> {
        Ok(self.state.lock().monitors.clone())
    }

    fn theme(&self) -> PlatformResult<ThemeInfo> {
        Ok(self.state.lock().theme.clone())
    }

    fn recycle_bin(&self) -> PlatformResult<RecycleBinState> {
        Ok(self.state.lock().recycle_bin)
    }

    /// Recorded; then every plain absolute path counts as existing
    /// (`rules::recycle` with nothing missing) and is added to the bin's
    /// item count. Silently, like the real bin: no `RecycleBinChanged` is
    /// pushed, `recycle_bin` reports it (the service asks right after).
    fn delete_to_recycle_bin(&self, paths: &[String]) -> PlatformResult<()> {
        self.record(MockCall::DeleteToRecycleBin(paths.to_vec()));
        let mut s = self.state.lock();
        if let Some(message) = &s.recycle_failure {
            return Err(PlatformError::Other(message.clone()));
        }
        let plan = rules::recycle::plan_recycle(paths, |_| true);
        if plan.recycle.is_empty() {
            return Err(PlatformError::Other(rules::recycle::nothing_to_recycle(
                &plan,
            )));
        }
        s.recycle_bin.item_count += plan.recycle.len() as u64;
        Ok(())
    }

    fn shell_action(&self, action: ShellAction) -> PlatformResult<()> {
        self.record(MockCall::Shell(action));
        if action == ShellAction::EmptyRecycleBin {
            self.push(PlatformEvent::RecycleBinChanged(RecycleBinState::default()));
        }
        Ok(())
    }

    /// The scripted state (`false` unless [`MockPlatform::script_taskbar_auto_hide`]
    /// or a set changed it); the read is recorded.
    fn taskbar_auto_hide(&self) -> PlatformResult<bool> {
        self.record(MockCall::TaskbarAutoHide);
        Ok(self.state.lock().taskbar_auto_hide)
    }

    /// Recorded and applied to the scripted state; nothing real changes.
    fn set_taskbar_auto_hide(&self, enabled: bool) -> PlatformResult<()> {
        self.record(MockCall::SetTaskbarAutoHide(enabled));
        self.state.lock().taskbar_auto_hide = enabled;
        Ok(())
    }

    /// Recorded; answers the scripted value at once (no box, no wait).
    fn ask_yes_no(&self, title: &str, text: &str) -> PlatformResult<bool> {
        self.record(MockCall::AskYesNo {
            title: title.to_owned(),
            text: text.to_owned(),
        });
        Ok(self.state.lock().yes_no_answer)
    }

    fn prepare_dock_window(
        &self,
        handle: NativeWindowHandle,
        options: DockWindowOptions,
    ) -> PlatformResult<()> {
        self.record(MockCall::PrepareDockWindow(handle, options));
        Ok(())
    }

    /// Recorded only: a browser or a mock run has no OS hit-testing.
    fn set_dock_hit_region(
        &self,
        handle: NativeWindowHandle,
        region: Option<HitRegion>,
    ) -> PlatformResult<()> {
        self.record(MockCall::SetDockHitRegion(handle, region));
        Ok(())
    }

    /// Recorded, and kept in [`MockPlatform::app_bars`]; nothing real is
    /// reserved and the scripted work areas do not change. A request is
    /// granted at once, without the real platform's wait, and the grant
    /// ([`mock_band`]) goes to `sink` synchronously, on the calling thread,
    /// after every request, the same one again included, as the real
    /// platform reports after every request it applied. A release reports
    /// nothing, like the real platform.
    fn set_dock_app_bar(
        &self,
        handle: NativeWindowHandle,
        request: Option<AppBarRequest>,
        sink: AppBarSink,
    ) -> PlatformResult<()> {
        let grant = {
            let mut s = self.state.lock();
            s.calls.push(MockCall::SetDockAppBar(handle, request));
            let held = s.app_bars.iter().position(|(h, _)| *h == handle);
            let granted = match (request, held) {
                (None, Some(i)) => {
                    s.app_bars.remove(i);
                    None
                }
                (None, None) => None,
                (Some(request), Some(i)) => {
                    s.app_bars[i].1 = request;
                    Some(request)
                }
                (Some(request), None) => {
                    s.app_bars.push((handle, request));
                    Some(request)
                }
            };
            granted.map(|request| AppBarGrant {
                request,
                rect: mock_band(&s.monitors, &request),
            })
        };
        if let Some(grant) = grant {
            sink(handle, Some(grant));
        }
        Ok(())
    }

    /// Recorded; every registration is dropped at once.
    fn release_app_bars(&self, _timeout: Duration) -> PlatformResult<()> {
        let mut s = self.state.lock();
        s.calls.push(MockCall::ReleaseAppBars);
        s.app_bars.clear();
        Ok(())
    }

    /// Recorded only: a browser or a mock run has no compositor material.
    fn set_dock_backdrop(
        &self,
        handle: NativeWindowHandle,
        backdrop: Option<DockBackdrop>,
    ) -> PlatformResult<()> {
        self.record(MockCall::SetDockBackdrop(handle, backdrop));
        Ok(())
    }

    /// Recorded only: a mock run has no foreground to give, so the call
    /// succeeds and nothing else changes (the scripted foreground window
    /// stays what it was, like the real events thread, which ignores
    /// foreground reports for our own windows).
    fn focus_dock_window(&self, handle: NativeWindowHandle) -> PlatformResult<()> {
        self.record(MockCall::FocusDockWindow(handle));
        Ok(())
    }
}

fn identity_for(raw: &RawWindow) -> AppIdentity {
    let id = stem_core::app_id_for(raw);
    let display_name = raw
        .exe_path
        .as_deref()
        .map(file_stem)
        .unwrap_or_else(|| raw.title.clone());
    let launch = raw.exe_path.clone().map(|path| LaunchTarget::Exe {
        path,
        args: vec![],
        working_dir: None,
    });
    AppIdentity {
        icon_key: Some(IconKey(format!("mock:{}", id.as_str()))),
        id,
        display_name,
        launch,
    }
}

/// File name without extension, treating both `/` and `\` as separators so
/// Windows-style paths behave the same in tests on any host.
fn file_stem(path: &std::path::Path) -> String {
    let s = path.to_string_lossy();
    let name = s.rsplit(['/', '\\']).next().unwrap_or(&s);
    match name.rfind('.') {
        Some(0) | None => name.to_owned(),
        Some(dot) => name[..dot].to_owned(),
    }
}

/// Deterministic colour per key so screenshots are stable.
fn color_for(key: &str) -> [u8; 4] {
    let hash = key.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    let hue = (hash % 360) as f32;
    let (r, g, b) = hsl_to_rgb(hue, 0.65, 0.55);
    [r, g, b, 255]
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match h as u32 {
        0..=59 => (c, x, 0.0),
        60..=119 => (x, c, 0.0),
        120..=179 => (0.0, c, x),
        180..=239 => (0.0, x, c),
        240..=299 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

/// A rounded square PNG in one colour.
pub fn solid_png(size: u32, rgba: [u8; 4]) -> Vec<u8> {
    let radius = (size as f32) * 0.22;
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let inside = rounded_rect_contains(x as f32 + 0.5, y as f32 + 0.5, size as f32, radius);
            data.extend_from_slice(&[rgba[0], rgba[1], rgba[2], if inside { rgba[3] } else { 0 }]);
        }
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, size, size);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .expect("png header for in-memory buffer");
        writer
            .write_image_data(&data)
            .expect("png data for in-memory buffer");
    }
    out
}

fn rounded_rect_contains(x: f32, y: f32, size: f32, r: f32) -> bool {
    let cx = x.clamp(r, size - r);
    let cy = y.clamp(r, size - r);
    (x - cx).powi(2) + (y - cy).powi(2) <= r * r
}

fn demo_monitor() -> MonitorInfo {
    MonitorInfo {
        id: MonitorId("mock-primary".into()),
        name: "Mock Display".into(),
        bounds: Rect::new(0, 0, 2560, 1440),
        work_area: Rect::new(0, 0, 2560, 1392),
        scale_factor: 1.0,
        is_primary: true,
    }
}

fn demo_window(id: u64, exe: &str, title: &str, minimized: bool, rect: Rect) -> RawWindow {
    RawWindow {
        id: WindowId(id),
        title: title.into(),
        class_name: format!("{}Class", file_stem(std::path::Path::new(exe))),
        process_id: 1000 + id as u32,
        exe_path: Some(PathBuf::from(exe)),
        app_user_model_id: None,
        is_visible: true,
        is_minimized: minimized,
        is_cloaked: false,
        on_current_desktop: Some(true),
        monitor: Some(MonitorId("mock-primary".into())),
        rect: Some(rect),
        icon_key: None,
    }
}

/// The demo's windows, each with a plausible frame on the demo monitor:
/// all of them keep clear of the 250 px along the bottom, left and
/// right edges of the work area, where the dock's panel rests on those
/// edges, so nothing overlaps it until a test moves one
/// (`move_window`). The top band is not kept clear (the frames start near
/// the top, as on a real desktop). A minimised window keeps the frame it
/// would be restored to; the overlap rule ignores it while it is minimised.
/// The TS mirror (`src/lib/ipc/mock.ts`) uses the same frames.
fn demo_windows() -> Vec<RawWindow> {
    vec![
        demo_window(
            1,
            r"C:\Windows\explorer.exe",
            "Downloads",
            false,
            Rect::new(260, 90, 1100, 700),
        ),
        demo_window(
            2,
            r"C:\Program Files\Mozilla Firefox\firefox.exe",
            "Stem — Mozilla Firefox",
            false,
            Rect::new(640, 60, 1500, 1000),
        ),
        demo_window(
            3,
            r"C:\Program Files\Mozilla Firefox\firefox.exe",
            "GitHub — Mozilla Firefox",
            true,
            Rect::new(700, 120, 1400, 900),
        ),
        demo_window(
            4,
            r"C:\Users\me\AppData\Local\Programs\Microsoft VS Code\Code.exe",
            "lib.rs — Stem",
            false,
            Rect::new(260, 40, 1600, 1000),
        ),
        demo_window(
            5,
            r"C:\Program Files\WindowsApps\Terminal\WindowsTerminal.exe",
            "PowerShell",
            false,
            Rect::new(1300, 500, 1000, 600),
        ),
        demo_window(
            6,
            r"C:\Program Files\Spotify\Spotify.exe",
            "Spotify Premium",
            true,
            Rect::new(400, 200, 1200, 800),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use parking_lot::Mutex;

    use super::*;

    #[test]
    fn start_emits_initial_snapshot_in_order() {
        let p = MockPlatform::with_demo_scenario();
        let seen: Arc<Mutex<Vec<PlatformEvent>>> = Arc::default();
        let sink_seen = Arc::clone(&seen);
        p.start(Arc::new(move |ev| sink_seen.lock().push(ev)))
            .unwrap();
        let seen = seen.lock();
        assert!(matches!(seen[0], PlatformEvent::MonitorsChanged(_)));
        assert!(matches!(seen[3], PlatformEvent::WindowsSnapshot(ref w) if w.len() == 6));
        assert_eq!(seen[4], PlatformEvent::ForegroundChanged(Some(WindowId(2))));
        assert!(p.start(Arc::new(|_| {})).is_err());
    }

    #[test]
    fn close_action_removes_window_and_records_call() {
        let p = MockPlatform::with_demo_scenario();
        p.window_action(WindowId(2), WindowAction::Close).unwrap();
        assert!(p.windows().unwrap().iter().all(|w| w.id != WindowId(2)));
        assert_eq!(p.foreground_window().unwrap(), None);
        assert_eq!(
            p.calls(),
            vec![MockCall::WindowAction(WindowId(2), WindowAction::Close)]
        );
        assert!(matches!(
            p.window_action(WindowId(2), WindowAction::Close),
            Err(PlatformError::WindowNotFound(_))
        ));
    }

    #[test]
    fn quick_settings_and_notification_center_are_recorded_and_change_nothing() {
        let p = MockPlatform::with_demo_scenario();
        let seen: Arc<Mutex<Vec<PlatformEvent>>> = Arc::default();
        let sink_seen = Arc::clone(&seen);
        p.start(Arc::new(move |ev| sink_seen.lock().push(ev)))
            .unwrap();
        let before = seen.lock().len();
        let windows = p.windows().unwrap();

        p.shell_action(ShellAction::OpenQuickSettings).unwrap();
        p.shell_action(ShellAction::OpenNotificationCenter).unwrap();
        assert_eq!(
            p.calls(),
            vec![
                MockCall::Shell(ShellAction::OpenQuickSettings),
                MockCall::Shell(ShellAction::OpenNotificationCenter),
            ]
        );
        // Windows' flyouts are not windows the dock tracks: no event, the
        // same windows, the same foreground.
        assert_eq!(seen.lock().len(), before);
        assert_eq!(p.windows().unwrap(), windows);
        assert_eq!(p.foreground_window().unwrap(), Some(WindowId(2)));
    }

    #[test]
    fn icons_are_valid_png_and_deterministic() {
        let p = MockPlatform::new();
        let a = p
            .icon_png(&IconKey("mock:exe:x".into()), 48)
            .unwrap()
            .unwrap();
        let b = p
            .icon_png(&IconKey("mock:exe:x".into()), 48)
            .unwrap()
            .unwrap();
        assert_eq!(a, b);
        assert_eq!(&a[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(p.icon_png(&IconKey::shell("start"), 48).unwrap(), None);
    }

    #[test]
    fn delete_to_recycle_bin_records_the_paths_and_fills_the_bin_silently() {
        let p = MockPlatform::new();
        let seen: Arc<Mutex<Vec<PlatformEvent>>> = Arc::default();
        let sink_seen = Arc::clone(&seen);
        p.start(Arc::new(move |ev| sink_seen.lock().push(ev)))
            .unwrap();
        let before = seen.lock().len();
        assert!(p.recycle_bin().unwrap().is_empty());
        let paths = vec![
            r"C:\Users\me\Desktop\notes.txt".to_owned(),
            r"C:\Users\me\Desktop\Folder".to_owned(),
            "relative.txt".to_owned(),
        ];

        p.delete_to_recycle_bin(&paths).unwrap();
        assert_eq!(p.calls(), vec![MockCall::DeleteToRecycleBin(paths)]);
        // The relative path is skipped; the other two are in the bin.
        assert_eq!(p.recycle_bin().unwrap().item_count, 2);
        assert_eq!(seen.lock().len(), before, "no event: the caller asks");

        // Nothing usable: an error, the bin unchanged.
        let err = p.delete_to_recycle_bin(&[r"C:\".to_owned()]).unwrap_err();
        assert_eq!(err.to_string(), r"C:\ is a drive or share root");
        assert_eq!(p.recycle_bin().unwrap().item_count, 2);

        // A scripted refusal: recorded, an error, the bin unchanged.
        p.set_recycle_failure(Some("notes.txt is open in another program"));
        let locked = vec![r"C:\Users\me\locked.docx".to_owned()];
        let err = p.delete_to_recycle_bin(&locked).unwrap_err();
        assert_eq!(err.to_string(), "notes.txt is open in another program");
        assert_eq!(
            p.calls().last(),
            Some(&MockCall::DeleteToRecycleBin(locked.clone()))
        );
        assert_eq!(p.recycle_bin().unwrap().item_count, 2);
        p.set_recycle_failure(None);
        p.delete_to_recycle_bin(&locked).unwrap();
        assert_eq!(p.recycle_bin().unwrap().item_count, 3);
    }

    #[test]
    fn taskbar_auto_hide_is_scripted_follows_sets_and_records_every_call() {
        let p = MockPlatform::new();
        assert!(!p.taskbar_auto_hide().unwrap(), "off by default");
        p.set_taskbar_auto_hide(true).unwrap();
        assert!(p.taskbar_auto_hide().unwrap());
        p.set_taskbar_auto_hide(false).unwrap();
        assert!(!p.taskbar_auto_hide().unwrap());
        assert_eq!(
            p.calls(),
            vec![
                MockCall::TaskbarAutoHide,
                MockCall::SetTaskbarAutoHide(true),
                MockCall::TaskbarAutoHide,
                MockCall::SetTaskbarAutoHide(false),
                MockCall::TaskbarAutoHide,
            ]
        );

        // What the user had before Stem: scripted, not a call.
        p.clear_calls();
        p.script_taskbar_auto_hide(true);
        assert!(p.taskbar_auto_hide().unwrap());
        assert_eq!(p.calls(), vec![MockCall::TaskbarAutoHide]);
    }

    #[test]
    fn ask_yes_no_answers_the_scripted_value_and_records_the_question() {
        let p = MockPlatform::new();
        assert!(
            !p.ask_yes_no("Stem", "Open the log folder?").unwrap(),
            "No by default"
        );
        p.script_yes_no_answer(true);
        assert!(p.ask_yes_no("Stem", "Again?").unwrap());
        assert_eq!(
            p.calls(),
            vec![
                MockCall::AskYesNo {
                    title: "Stem".to_owned(),
                    text: "Open the log folder?".to_owned(),
                },
                MockCall::AskYesNo {
                    title: "Stem".to_owned(),
                    text: "Again?".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn app_bars_are_recorded_granted_on_the_work_area_edge_and_released() {
        use stem_core::settings::DockPosition;

        let p = MockPlatform::new();
        type Seen = Vec<(NativeWindowHandle, Option<AppBarGrant>)>;
        let grants: Arc<Mutex<Seen>> = Arc::default();
        let seen = Arc::clone(&grants);
        let sink: AppBarSink = Arc::new(move |handle, grant| seen.lock().push((handle, grant)));
        let monitor = demo_monitor();
        let bottom = AppBarRequest {
            monitor: monitor.bounds,
            edge: DockPosition::Bottom,
            thickness: 72,
        };
        let a = NativeWindowHandle(0xA);
        let b = NativeWindowHandle(0xB);

        p.set_dock_app_bar(a, Some(bottom), Arc::clone(&sink))
            .unwrap();
        // The demo monitor's work area stops 48 px short of its bottom.
        let band = Rect::new(0, 1392 - 72, 2560, 72);
        assert_eq!(
            grants.lock().clone(),
            vec![(
                a,
                Some(AppBarGrant {
                    request: bottom,
                    rect: band,
                })
            )]
        );
        // The same request again reports the same band again.
        p.set_dock_app_bar(a, Some(bottom), Arc::clone(&sink))
            .unwrap();
        assert_eq!(grants.lock().len(), 2);
        let seen = grants.lock().clone();
        assert_eq!(seen[1], seen[0]);

        // A monitor the script does not list: the band on its bounds.
        let elsewhere = AppBarRequest {
            monitor: Rect::new(-1920, 0, 1920, 1080),
            edge: DockPosition::Left,
            thickness: 80,
        };
        p.set_dock_app_bar(b, Some(elsewhere), Arc::clone(&sink))
            .unwrap();
        assert_eq!(
            grants.lock().last().copied(),
            Some((
                b,
                Some(AppBarGrant {
                    request: elsewhere,
                    rect: Rect::new(-1920, 0, 80, 1080),
                })
            ))
        );
        assert_eq!(p.app_bars(), vec![(a, bottom), (b, elsewhere)]);

        // A release reports nothing and drops the registration.
        p.set_dock_app_bar(a, None, Arc::clone(&sink)).unwrap();
        assert_eq!(grants.lock().len(), 3);
        assert_eq!(p.app_bars(), vec![(b, elsewhere)]);
        p.release_app_bars(Duration::from_millis(100)).unwrap();
        assert!(p.app_bars().is_empty());
        assert_eq!(
            p.calls(),
            vec![
                MockCall::SetDockAppBar(a, Some(bottom)),
                MockCall::SetDockAppBar(a, Some(bottom)),
                MockCall::SetDockAppBar(b, Some(elsewhere)),
                MockCall::SetDockAppBar(a, None),
                MockCall::ReleaseAppBars,
            ]
        );
    }

    #[test]
    fn set_dock_backdrop_records_every_call_in_order() {
        let p = MockPlatform::new();
        let dock = NativeWindowHandle(0x1234);
        let acrylic = DockBackdrop {
            rect: Rect::new(800, 1300, 960, 96),
            corner_radius: 33,
            material: crate::BackdropMaterial::Acrylic,
            dark: true,
        };
        p.set_dock_backdrop(dock, Some(acrylic)).unwrap();
        p.set_dock_backdrop(dock, None).unwrap();
        assert_eq!(
            p.calls(),
            vec![
                MockCall::SetDockBackdrop(dock, Some(acrylic)),
                MockCall::SetDockBackdrop(dock, None),
            ]
        );
    }

    #[test]
    fn focus_dock_window_is_recorded_and_leaves_the_foreground_alone() {
        let p = MockPlatform::with_demo_scenario();
        let before = p.foreground_window().unwrap();
        assert!(before.is_some());
        let dock = NativeWindowHandle(0x4321);
        p.focus_dock_window(dock).unwrap();
        assert_eq!(p.calls(), vec![MockCall::FocusDockWindow(dock)]);
        assert_eq!(p.foreground_window().unwrap(), before);
        // The mock has no native foreground window to report either, so the
        // app layer never takes a mock dock for the focused one.
        assert_eq!(p.foreground_native_window(), None);
    }

    #[test]
    fn identify_target_uses_canonical_app_ids() {
        let p = MockPlatform::new();
        let id = p
            .identify_target(&LaunchTarget::Exe {
                path: PathBuf::from(r"C:\A\b.EXE"),
                args: vec![],
                working_dir: None,
            })
            .unwrap();
        assert_eq!(id.id, AppId::for_exe(std::path::Path::new(r"c:/a/b.exe")));
        assert_eq!(id.display_name, "b");
    }

    #[test]
    fn demo_windows_keep_clear_of_the_bottom_left_and_right_panel_bands() {
        // Where a dock's panel rests on those edges, as in the TS mirror.
        const BAND: i32 = 250;
        let work = demo_monitor().work_area;
        for w in demo_windows() {
            let r = w.rect.unwrap();
            assert!(
                r.bottom() <= work.bottom() - BAND,
                "{}: bottom band",
                w.title
            );
            assert!(r.x >= work.x + BAND, "{}: left band", w.title);
            assert!(r.right() <= work.right() - BAND, "{}: right band", w.title);
        }
    }
}
