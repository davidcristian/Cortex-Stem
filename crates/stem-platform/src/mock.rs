//! A scripted, deterministic platform for tests and for running the app
//! without a real desktop. Behaviour is intentionally simple and documented;
//! tests may drive it through [`MockPlatform::push`] and the setters.

mod appbar;
mod call;
mod demo;
mod desktop;
mod icon;
mod identity;
mod platform;
mod script;
mod volume;

#[cfg(test)]
mod tests;

use parking_lot::Mutex;
use stem_core::{
    ClockFormat, MonitorInfo, PlatformEvent, PowerStatus, RawWindow, RecycleBinState, ThemeInfo,
    VolumeStatus, WindowId,
};

pub use call::MockCall;
use demo::{demo_monitor, demo_power, demo_volume, demo_windows};
pub use icon::solid_png;

use crate::{AppBarRequest, EventSink, NativeWindowHandle};

#[derive(Default)]
struct State {
    windows: Vec<RawWindow>,
    foreground: Option<WindowId>,
    monitors: Vec<MonitorInfo>,
    theme: ThemeInfo,
    recycle_bin: RecycleBinState,
    /// The power reading: no battery unless scripted (the demo has
    /// a laptop's).
    power: PowerStatus,
    /// The regional format: nothing known unless scripted, in the
    /// demo too (the TS mirror's `MockOptions.clockFormat` defaults to the
    /// same), so the page's own locale formats the clock.
    clock_format: ClockFormat,
    /// The default output device's volume: no device unless
    /// scripted (the demo has one at 60 %, as the TS mirror's
    /// `MockOptions.volume` defaults to).
    volume: Option<VolumeStatus>,
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
            .field("power", &self.power)
            .field("clock_format", &self.clock_format)
            .field("volume", &self.volume)
            .field("recycle_failure", &self.recycle_failure)
            .field("taskbar_auto_hide", &self.taskbar_auto_hide)
            .field("yes_no_answer", &self.yes_no_answer)
            .field("app_bars", &self.app_bars)
            .field("started", &self.sink.is_some())
            .field("calls", &self.calls)
            .finish()
    }
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

    /// A platform pre-populated with a handful of fake apps and windows, a
    /// laptop's battery ([`demo_power`]) and an output device at 60 %
    /// ([`demo_volume`]).
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
            s.power = demo_power();
            s.volume = Some(demo_volume());
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
                PlatformEvent::PowerChanged(p) => s.power = *p,
                PlatformEvent::ClockFormatChanged(f) => s.clock_format = f.clone(),
                PlatformEvent::VolumeChanged(v) => s.volume = *v,
                PlatformEvent::WindowAttention { .. } | PlatformEvent::FullscreenChanged { .. } => {
                }
            }
            s.sink.clone()
        };
        if let Some(sink) = sink {
            sink(event);
        }
    }
}
