//! A scripted, deterministic platform for tests and for running the app
//! without a real desktop. Behaviour is intentionally simple and documented;
//! tests may drive it through [`MockPlatform::push`] and the setters.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use stem_core::{
    AppId, AppIdentity, IconKey, LaunchTarget, MonitorId, MonitorInfo, PlatformEvent, RawWindow,
    Rect, RecycleBinState, ShellAction, ThemeInfo, WindowAction, WindowId,
};

use crate::{
    DockWindowOptions, EventSink, HitRegion, NativeWindowHandle, Platform, PlatformError,
    PlatformResult,
};

#[derive(Default)]
struct State {
    windows: Vec<RawWindow>,
    foreground: Option<WindowId>,
    monitors: Vec<MonitorInfo>,
    theme: ThemeInfo,
    recycle_bin: RecycleBinState,
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
    Shell(ShellAction),
    PrepareDockWindow(NativeWindowHandle, DockWindowOptions),
    SetDockHitRegion(NativeWindowHandle, Option<HitRegion>),
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
                PlatformEvent::WindowAttention { .. } => {}
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

    fn record(&self, call: MockCall) {
        self.state.lock().calls.push(call);
    }
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

    fn shell_action(&self, action: ShellAction) -> PlatformResult<()> {
        self.record(MockCall::Shell(action));
        if action == ShellAction::EmptyRecycleBin {
            self.push(PlatformEvent::RecycleBinChanged(RecycleBinState::default()));
        }
        Ok(())
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

fn demo_window(id: u64, exe: &str, title: &str, minimized: bool) -> RawWindow {
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
        monitor: Some(MonitorId("mock-primary".into())),
        icon_key: None,
    }
}

fn demo_windows() -> Vec<RawWindow> {
    vec![
        demo_window(1, r"C:\Windows\explorer.exe", "Downloads", false),
        demo_window(
            2,
            r"C:\Program Files\Mozilla Firefox\firefox.exe",
            "Stem — Mozilla Firefox",
            false,
        ),
        demo_window(
            3,
            r"C:\Program Files\Mozilla Firefox\firefox.exe",
            "GitHub — Mozilla Firefox",
            true,
        ),
        demo_window(
            4,
            r"C:\Users\me\AppData\Local\Programs\Microsoft VS Code\Code.exe",
            "lib.rs — Stem",
            false,
        ),
        demo_window(
            5,
            r"C:\Program Files\WindowsApps\Terminal\WindowsTerminal.exe",
            "PowerShell",
            false,
        ),
        demo_window(
            6,
            r"C:\Program Files\Spotify\Spotify.exe",
            "Spotify Premium",
            true,
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
}
