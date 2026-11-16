//! The engine's tests, one file per topic in `tests/`. This file holds the
//! helpers that more than one topic uses.

mod basics;
mod clicks;
mod clock_and_battery;
mod clock_format;
mod docks;
mod edge_cases;
mod fullscreen;
mod hidden_apps;
mod overlap;
mod quick_settings;
mod scrolling;
mod volume;

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::snapshot::{battery_title, volume_title};
use super::*;
use crate::events::PlatformEvent;
use crate::model::{
    DockFacts, DockItem, IconKey, LaunchTarget, MonitorId, RawWindow, Rect, ScrollDirection,
    ShellAction, WindowInfo,
};
use crate::settings::{MonitorMode, PinnedApp, ScrollAction};

fn win(id: u64, exe: &str, title: &str) -> RawWindow {
    RawWindow {
        id: WindowId(id),
        title: title.into(),
        class_name: "Window".into(),
        process_id: id as u32,
        exe_path: Some(PathBuf::from(exe)),
        app_user_model_id: None,
        is_visible: true,
        is_minimized: false,
        is_cloaked: false,
        on_current_desktop: None,
        monitor: None,
        rect: None,
        icon_key: None,
    }
}

fn cloaked(mut raw: RawWindow) -> RawWindow {
    raw.is_cloaked = true;
    raw
}

fn pin(exe: &str) -> PinnedApp {
    PinnedApp {
        id: AppId::for_exe(Path::new(exe)),
        label: None,
        launch: LaunchTarget::Exe {
            path: PathBuf::from(exe),
            args: vec![],
            working_dir: None,
        },
    }
}

fn exe_id(exe: &str) -> AppId {
    AppId::for_exe(Path::new(exe))
}

fn identity(id: &AppId, name: &str, icon: &str) -> AppIdentity {
    AppIdentity {
        id: id.clone(),
        display_name: name.into(),
        icon_key: Some(IconKey(icon.into())),
        launch: None,
    }
}

fn keys(state: &DockState) -> Vec<&str> {
    state.items.iter().map(|i| i.key.as_str()).collect()
}

/// The primary monitor's dock window, whose monitor the single-dock
/// tests never register: its clicks consider every window.
const DOCK: &str = "dock";

fn minimal_settings() -> Settings {
    let mut s = Settings::default();
    s.items.launcher = false;
    s.items.recycle_bin = false;
    s
}

fn added(raw: RawWindow) -> EngineInput {
    PlatformEvent::WindowAdded(raw).into()
}

fn updated(raw: RawWindow) -> EngineInput {
    PlatformEvent::WindowUpdated(raw).into()
}

fn removed(id: u64) -> EngineInput {
    PlatformEvent::WindowRemoved(WindowId(id)).into()
}

fn foreground(id: Option<u64>) -> EngineInput {
    PlatformEvent::ForegroundChanged(id.map(WindowId)).into()
}

fn attention(id: u64, flashing: bool) -> EngineInput {
    PlatformEvent::WindowAttention {
        id: WindowId(id),
        flashing,
    }
    .into()
}

// ----- the fullscreen verdict, per dock -------------------------------------

/// The primary demo monitor and a second one to its right.
const PRIMARY: &str = "mock-primary";
const SECOND: &str = r"\\.\DISPLAY3";

fn monitor(id: &str, primary: bool, x: i32) -> MonitorInfo {
    MonitorInfo {
        id: MonitorId(id.into()),
        name: id.into(),
        bounds: Rect::new(x, 0, 2560, 1440),
        work_area: Rect::new(x, 0, 2560, 1392),
        scale_factor: 1.0,
        is_primary: primary,
    }
}

fn monitors(list: Vec<MonitorInfo>) -> EngineInput {
    PlatformEvent::MonitorsChanged(list).into()
}

/// The platform's verdict, with the fullscreen window's monitor.
fn fullscreen_on(monitor: Option<&str>) -> EngineInput {
    PlatformEvent::FullscreenChanged {
        active: true,
        monitor: monitor.map(|m| MonitorId(m.into())),
    }
    .into()
}

fn fullscreen_off() -> EngineInput {
    PlatformEvent::FullscreenChanged {
        active: false,
        monitor: None,
    }
    .into()
}

/// `DockState::docks` entry of `monitor`, if the snapshot has one.
fn facts(state: &DockState, monitor: &str) -> Option<DockFacts> {
    state.docks.iter().find(|d| d.monitor.0 == monitor).cloned()
}

fn is_fullscreen(engine: &DockEngine, monitor: &str) -> bool {
    facts(&engine.snapshot(), monitor).is_some_and(|d| d.fullscreen)
}

fn is_overlapped(engine: &DockEngine, monitor: &str) -> bool {
    facts(&engine.snapshot(), monitor).is_some_and(|d| d.overlapped)
}

// ----- a window over the dock ------------------------------------------------

/// A 560 × 72 panel at the bottom of a 2560 × 1392 work area.
const PANEL: Rect = Rect::new(1000, 1320, 560, 72);
/// A frame that covers the panel's left half.
const OVER: Rect = Rect::new(600, 900, 700, 480);
/// A frame in the top-left corner, clear of the panel.
const CLEAR: Rect = Rect::new(0, 0, 800, 600);

/// The app layer's report about one dock window.
fn panel_of(label: &str, monitor: Option<&str>, rect: Option<Rect>) -> EngineInput {
    EngineInput::DockPanelChanged {
        label: label.to_owned(),
        monitor: monitor.map(|m| MonitorId(m.into())),
        rect,
    }
}

/// The primary dock window's panel, as the single-dock tests report it.
fn panel(rect: Option<Rect>) -> EngineInput {
    panel_of("dock", Some(PRIMARY), rect)
}

fn framed(id: u64, exe: &str, rect: Rect) -> RawWindow {
    let mut raw = win(id, exe, "W");
    raw.rect = Some(rect);
    raw
}

// ----- clock and battery -------------------------------------------------

fn with_clock_and_battery(clock: bool, battery: bool) -> Settings {
    let mut s = Settings::default();
    s.items.clock = clock;
    s.items.battery = battery;
    s
}

fn laptop(percent: u8) -> PowerStatus {
    PowerStatus {
        has_battery: true,
        percent: Some(percent),
        charging: false,
        on_ac: false,
        saver: false,
    }
}

fn power(status: PowerStatus) -> EngineInput {
    PlatformEvent::PowerChanged(status).into()
}

fn item<'a>(state: &'a DockState, key: &str) -> Option<&'a DockItem> {
    state.items.iter().find(|i| i.key == key)
}

// ----- clicks and menus act on the windows the dock shows -------------------

const NOTEPAD: &str = r"C:\Windows\notepad.exe";
const NOTEPAD_KEY: &str = "app:exe:c:/windows/notepad.exe";
/// The second monitor's dock window, as the app layer labels it.
const SECOND_DOCK: &str = "dock-DISPLAY3";

fn on_monitor(mut raw: RawWindow, monitor: &str) -> RawWindow {
    raw.monitor = Some(MonitorId(monitor.into()));
    raw
}

fn ids(windows: &[WindowInfo]) -> Vec<u64> {
    windows.iter().map(|w| w.id.0).collect()
}

/// A dock on each of two monitors (`monitors = all`), with
/// `showOnlyCurrentMonitor` as given; Notepad (pinned or not) has
/// window 1 ("A") on the primary, seen first, and window 2 ("B") on the
/// second monitor.
fn two_docks(pinned: bool, show_only_current_monitor: bool) -> DockEngine {
    let mut settings = minimal_settings();
    settings.monitors = MonitorMode::All;
    settings.behavior.show_only_current_monitor = show_only_current_monitor;
    if pinned {
        settings.pinned = vec![pin(NOTEPAD)];
    }
    let mut engine = DockEngine::new(settings);
    engine.apply(monitors(vec![
        monitor(PRIMARY, true, 0),
        monitor(SECOND, false, 2560),
    ]));
    engine.apply(panel_of(DOCK, Some(PRIMARY), None));
    engine.apply(panel_of(SECOND_DOCK, Some(SECOND), None));
    engine.apply(added(on_monitor(win(1, NOTEPAD, "A"), PRIMARY)));
    engine.apply(added(on_monitor(win(2, NOTEPAD, "B"), SECOND)));
    engine
}

// ----- hidden apps -------------------------------------------------------------

fn hidden(exe: &str, name: &str) -> crate::settings::HiddenApp {
    crate::settings::HiddenApp {
        id: exe_id(exe),
        name: name.into(),
        path: None,
        icon_key: None,
    }
}

// ----- scrolling over an item ---------------------------------------------------

fn cycling(mut settings: Settings) -> Settings {
    settings.behavior.scroll_action = ScrollAction::CycleWindows;
    settings
}

/// One instant for every scroll test that does not look at the time:
/// a memory noted at it never runs out at it.
fn t0() -> Instant {
    static T0: std::sync::LazyLock<Instant> = std::sync::LazyLock::new(Instant::now);
    *T0
}
