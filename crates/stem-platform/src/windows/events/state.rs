//! The state of the events thread ([`ThreadState`]), its setup and the
//! events it emits at start.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use stem_core::{MonitorId, PlatformEvent, RawWindow, ThemeInfo, WindowId};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_LOCATIONCHANGE;

use super::hooks::{WinEventHook, location_event_proc};
use super::power_status::PowerNotification;
use super::timer::ThreadTimer;
use super::window::EventWindow;
use crate::rules::events::{self as rules, DesktopSwitchDetector, TitleDebouncer};
use crate::rules::locale as locale_rules;
use crate::rules::power as power_rules;
use crate::rules::recheck::RecheckSchedule;
use crate::windows::{actions, util, virtual_desktop};
use crate::{EventSink, PlatformResult};

/// Everything the loop owns. Field order is drop order: the timers, then
/// hooks, then the power-setting registrations, then the window, then the
/// virtual desktop manager, then the COM apartment last.
pub(super) struct ThreadState {
    sink: EventSink,
    pub(super) thread_id: u32,
    started: Instant,
    /// Windows reported to the engine (snapshot or `WindowAdded`) and not
    /// yet removed, each with the facts last reported about it (what a
    /// coalesced re-query after a move compares against).
    pub(super) known: HashMap<WindowId, RawWindow>,
    /// Tracked windows that moved or resized since the last flush.
    pub(super) moved: HashSet<WindowId>,
    /// The [`LOCATION_SETTLE`](super::LOCATION_SETTLE) timer, while a flush
    /// of `moved` is pending.
    pub(super) location_timer: Option<ThreadTimer>,
    /// The foreground window as this thread last saw it (the raw `HWND`
    /// value; 0 for none): a move of it re-evaluates fullscreen.
    pub(super) foreground_hwnd: isize,
    pub(super) titles: TitleDebouncer,
    pub(super) title_timer: Option<ThreadTimer>,
    /// Windows not ready when they appeared, keyed by `WindowId.0`.
    pub(super) rechecks: RecheckSchedule,
    /// The timer armed for a re-check, with the due time (ms) it was armed
    /// for; never later than `rechecks.next_due()`.
    pub(super) recheck_timer: Option<(ThreadTimer, u64)>,
    /// The last fullscreen verdict emitted, with the monitor it
    /// named; `None` before the first.
    pub(super) fullscreen: Option<(bool, Option<MonitorId>)>,
    /// The [`FULLSCREEN_SETTLE`](super::FULLSCREEN_SETTLE) timer, while an
    /// evaluation is pending.
    pub(super) fullscreen_timer: Option<ThreadTimer>,
    /// Cloak events counted towards a virtual-desktop switch.
    pub(super) switch: DesktopSwitchDetector,
    /// The [`DESKTOP_SWITCH_SETTLE`](super::DESKTOP_SWITCH_SETTLE) timer,
    /// while a snapshot is pending.
    pub(super) switch_timer: Option<ThreadTimer>,
    /// The [`CLOCK_FORMAT_RECHECK`](super::CLOCK_FORMAT_RECHECK) timer,
    /// while the second read after an `intl` broadcast is pending.
    pub(super) clock_format_timer: Option<ThreadTimer>,
    pub(super) last_theme: Option<ThemeInfo>,
    /// The power readings already emitted.
    pub(super) power: power_rules::ChangeFilter,
    /// The battery-saver state of the last `GUID_POWER_SAVING_STATUS`
    /// notification; `None` before the first.
    pub(super) notified_saver: Option<bool>,
    /// The regional-format readings already emitted.
    pub(super) clock_format: locale_rules::ChangeFilter,
    _hooks: Vec<WinEventHook>,
    /// Before `_window`: unregistered while the window still exists.
    _power_notifications: Vec<PowerNotification>,
    _window: EventWindow,
    /// This thread's `IVirtualDesktopManager` slot: released here,
    /// before the apartment it was created in.
    _desktops: virtual_desktop::ThreadManager,
    _com: util::ComApartment,
}

impl ThreadState {
    pub(super) fn setup(sink: EventSink) -> PlatformResult<Self> {
        let com = util::ComApartment::sta();
        // Inside the apartment, released before it (field order below); the
        // manager itself is created by the first query.
        let desktops = virtual_desktop::ThreadManager::enable();
        let window = EventWindow::create()?;
        let power_notifications = PowerNotification::register_all(window.hwnd);
        let mut hooks = Vec::with_capacity(rules::HOOK_RANGES.len() + 1);
        for &(min, max) in rules::HOOK_RANGES {
            hooks.push(WinEventHook::install(min, max)?);
        }
        // Moves and resizes of top-level windows, through
        // their own callback.
        hooks.push(WinEventHook::install_with(
            EVENT_OBJECT_LOCATIONCHANGE,
            EVENT_OBJECT_LOCATIONCHANGE,
            Some(location_event_proc),
        )?);
        // SAFETY: GetCurrentThreadId has no preconditions.
        let thread_id = unsafe { GetCurrentThreadId() };
        Ok(Self {
            sink,
            thread_id,
            started: Instant::now(),
            known: HashMap::new(),
            moved: HashSet::new(),
            location_timer: None,
            foreground_hwnd: 0,
            titles: TitleDebouncer::default(),
            title_timer: None,
            rechecks: RecheckSchedule::default(),
            recheck_timer: None,
            fullscreen: None,
            fullscreen_timer: None,
            switch: DesktopSwitchDetector::default(),
            switch_timer: None,
            clock_format_timer: None,
            last_theme: None,
            power: power_rules::ChangeFilter::default(),
            notified_saver: None,
            clock_format: locale_rules::ChangeFilter::default(),
            _hooks: hooks,
            _power_notifications: power_notifications,
            _window: window,
            _desktops: desktops,
            _com: com,
        })
    }

    pub(super) fn emit(&self, event: PlatformEvent) {
        (self.sink)(event);
    }

    pub(super) fn now_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// Same order as the mock platform (monitors, theme, windows,
    /// foreground, power, clock format), minus the recycle bin, which the
    /// poll thread emits; the first fullscreen verdict comes before
    /// the power reading, so the engine starts right.
    pub(super) fn emit_initial(&mut self) {
        self.emit_monitors();
        self.refresh_theme(true);
        self.emit_snapshot();
        self.emit(PlatformEvent::ForegroundChanged(
            actions::foreground_window(),
        ));
        self.check_fullscreen();
        self.refresh_power("start");
        self.refresh_clock_format("start");
    }
}
