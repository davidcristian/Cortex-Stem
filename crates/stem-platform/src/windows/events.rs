//! The event thread.
//!
//! Contract:
//! - [`EventHookThread::start`] spawns a thread that: creates a hidden
//!   top-level window, installs `SetWinEventHook` out-of-context hooks for
//!   `EVENT_OBJECT_CREATE/DESTROY/SHOW/HIDE/CLOAKED/UNCLOAKED/NAMECHANGE`,
//!   `EVENT_SYSTEM_FOREGROUND`, `EVENT_SYSTEM_MINIMIZESTART/END`, registers a
//!   shell hook window (`RegisterShellHookWindow`) for `HSHELL_WINDOWCREATED`,
//!   `HSHELL_FLASH` and activations (the second source; see *Sources*
//!   below), and then runs a `GetMessageW` loop.
//! - Hook callbacks only translate the event and push a [`stem_core::PlatformEvent`]
//!   into `sink`; all filtering goes through `enumerate::query_window_checked`.
//! - Late-visible windows: a window that a create/show event or
//!   `HSHELL_WINDOWCREATED` finds hidden or cloaked, while it passes every
//!   other eligibility rule, is queried again 250 ms, 1 s and 3 s later on a
//!   thread timer (`rules::recheck`) and reported with the same
//!   `WindowAdded` a show produces once it qualifies. At most 64 windows are
//!   pending; one trace line per re-check, a debug line when a late window
//!   is added.
//! - Foreground reports for windows of our own process (the dock after a
//!   click, the settings window) are dropped without emitting anything, so
//!   the app that was active stays active for the engine; the
//!   WinEvent hook already skips them (`WINEVENT_SKIPOWNPROCESS`), the shell
//!   hook does not.
//! - The thread emits an initial `WindowsSnapshot`, `MonitorsChanged`,
//!   `ThemeChanged`, `ForegroundChanged` after setup; the initial
//!   `RecycleBinChanged` comes from the poll thread (below).
//! - `WM_DISPLAYCHANGE`/`WM_DPICHANGED` -> `MonitorsChanged` + `WindowsSnapshot`.
//! - `WM_SETTINGCHANGE` ("ImmersiveColorSet") and `WM_DWMCOLORIZATIONCOLORCHANGED`
//!   -> `ThemeChanged` (via `theme::current_theme`).
//! - Clock format: `WM_SETTINGCHANGE` whose `lParam` names the
//!   `intl` area (`rules::locale::is_intl_area`; the window procedure
//!   copies at most [`SETTING_AREA_MAX`] units of the string out while it
//!   is valid) re-reads `locale::current` at once and once more
//!   [`CLOCK_FORMAT_RECHECK`] later (one one-shot timer, re-armed by each
//!   such broadcast), and emits `ClockFormatChanged` only when the reading
//!   differs from the last one emitted (`rules::locale::ChangeFilter`); the
//!   first reading is emitted right after the start snapshot, after the
//!   power reading. The second read is there because Microsoft does not document
//!   that the process's copy of the user's locale data is current when the
//!   broadcast arrives: a stale first read would equal the last reading and
//!   be dropped. Debug line per emission: `clock format: <reading>
//!   (<cause>)`.
//! - Window moves: one more `SetWinEventHook`, for
//!   `EVENT_OBJECT_LOCATIONCHANGE` alone, whose callback keeps only
//!   `OBJID_WINDOW`/`CHILDID_SELF` reports about top-level windows
//!   (`rules::events::location_change_tracked`) and posts. The loop collects
//!   the windows it already tracks (`known`) into a small set and, once per
//!   [`LOCATION_SETTLE`] (150 ms, a one-shot [`ThreadTimer`] armed by the
//!   first report of a burst), re-queries each of them
//!   (`enumerate::query_moved_window`, which reuses the desktop answer
//!   last reported instead of asking the shell) and emits
//!   `WindowUpdated` only when its facts differ from the last report (the
//!   frame included), so a window being dragged costs the engine at most
//!   about seven updates a second and a window that is not tracked costs
//!   nothing here (the re-checks handle newcomers). The engine turns
//!   the frames into `DockState::overlapped`.
//! - Fullscreen: the thread keeps the platform's verdict on whether
//!   the foreground window owns the whole screen (`fullscreen::evaluate`
//!   over `rules::fullscreen`) and emits `FullscreenChanged` when it
//!   changes, plus once right after the start snapshot. It is re-evaluated
//!   on every foreground report (WinEvent or shell hook, our own windows
//!   included), on a display change, and whenever the foreground window
//!   moves or resizes (the location reports above about the window this
//!   thread last saw in the foreground). Each trigger arms one
//!   [`FULLSCREEN_SETTLE`] timer (150 ms) if none is running, so a burst (F11
//!   in a browser fires dozens of location changes) costs one evaluation,
//!   against the live `GetForegroundWindow`. The verdict carries the
//!   window's monitor, so a fullscreen window that changes monitor
//!   is a change too. `info` on each change (`fullscreen on: <class>
//!   <title> on <monitor>` / `fullscreen off`).
//! - Virtual desktops: every query but the move path's fills
//!   `RawWindow::on_current_desktop` from the shell's
//!   `IVirtualDesktopManager` (`windows::virtual_desktop`), whose
//!   per-thread instance this thread enables in `setup`, inside its COM
//!   apartment, and releases before it; the move path keeps the answer last
//!   reported (a move does not change the desktop). The call has no
//!   timeout: a slow one pauses the asking for 30 s (`None` meanwhile), a
//!   hung one blocks this thread until Explorer answers (see
//!   `windows::virtual_desktop`). A desktop switch has no event of
//!   its own: its signature is the shell cloaking and uncloaking top-level
//!   windows in a burst, and each of those events already re-queries its
//!   window. When two of them arrive within 100 ms
//!   (`rules::events::DesktopSwitchDetector`) the thread arms one
//!   [`DESKTOP_SWITCH_SETTLE`] timer (400 ms) and, when it fires, emits
//!   one `WindowsSnapshot` (a re-enumeration, with a fresh manager), so a
//!   window whose event was missed or whose answer was taken mid-switch is
//!   put right; any snapshot resets the detector. Debug line: `desktop
//!   switch settled; re-enumerating`.
//! - Power: the hidden window is registered
//!   (`RegisterPowerSettingNotification`, `DEVICE_NOTIFY_WINDOW_HANDLE`)
//!   for `GUID_BATTERY_PERCENTAGE_REMAINING`, `GUID_ACDC_POWER_SOURCE` and
//!   `GUID_POWER_SAVING_STATUS`; a registration that fails is logged at
//!   `warn` and the others stay (the `PBT_APMPOWERSTATUSCHANGE` broadcast
//!   still arrives). `WM_POWERBROADCAST` with `PBT_POWERSETTINGCHANGE`,
//!   `PBT_APMPOWERSTATUSCHANGE` or `PBT_APMRESUMEAUTOMATIC` re-reads
//!   `power::current` and emits `PowerChanged` only when the reading differs
//!   from the last one emitted (`rules::power::ChangeFilter`); the first
//!   reading is emitted right after the start snapshot. The procedure
//!   answers `TRUE`.
//!   For a `PBT_POWERSETTINGCHANGE` it copies one value out of `lParam` (a
//!   `POWERBROADCAST_SETTING`, valid only during the call; at most
//!   [`POWER_SETTING_DATA_MAX`] bytes of data are looked at): the battery
//!   saver state of a `GUID_POWER_SAVING_STATUS` notification
//!   (`rules::power::notified_saver`), posted with the code. From then on
//!   the last such value takes the place of `SystemStatusFlag` in every reading
//!   (`rules::power::with_notified_saver`): Microsoft documents
//!   `PBT_APMPOWERSTATUSCHANGE` for power-source and battery-level changes
//!   only, so nothing is known to follow a saver toggle that would correct
//!   a `SystemStatusFlag` read too early. Debug lines:
//!   `GUID_POWER_SAVING_STATUS: battery saver on|off` per notification, and
//!   `SystemStatusFlag says battery saver …` when a reading's flag
//!   disagrees with it. Each registration is a guard
//!   ([`PowerNotification`]), unregistered on this thread before the window
//!   is destroyed. Debug line per emission: `power: <reading> (<cause>)`.
//! - A second thread (`dock-recycle-bin`, [`RecycleBinPoller`]) polls
//!   `recycle_bin::query` every [`RECYCLE_BIN_POLL`] and emits
//!   `RecycleBinChanged` on change. `SHQueryRecycleBinW(NULL)` walks every
//!   drive and can block for seconds (network or sleeping drives), so the
//!   events thread never calls it: a slow query must not delay window events.
//! - [`EventHookThread::stop`] stops the poller, posts `WM_QUIT`, unhooks,
//!   and joins (bounded).
//!
//! # Design
//!
//! One OS thread (`stem-platform-events`) owns everything: a hidden window,
//! the WinEvent hooks, the power-setting registrations, the timers and the
//! message loop. All of it is released on that thread, in reverse order, when
//! the loop exits (RAII guards: [`WinEventHook`], [`PowerNotification`],
//! [`EventWindow`], [`ThreadTimer`]).
//!
//! **Hidden top-level window instead of a message-only window.**
//! `WM_SETTINGCHANGE`, `WM_DISPLAYCHANGE`, `WM_THEMECHANGED`,
//! `WM_DWMCOLORIZATIONCOLORCHANGED` and `WM_POWERBROADCAST`
//! (`PBT_APMPOWERSTATUSCHANGE`, the resume codes) are broadcasts to
//! *top-level* windows, and
//! message-only windows (`HWND_MESSAGE` parent) are documented not to receive
//! broadcasts. The window is therefore an ordinary top-level window that is
//! never shown (`WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`, 0×0). It belongs to
//! our own process and is invisible, so `enumerate` rejects it.
//!
//! **The hook callback and the window procedure do the minimum.** Both run
//! on this thread (out-of-context hooks are delivered through the installing
//! thread's message loop; a window procedure runs on the thread that owns the
//! window). They only filter and translate (`rules::events`) and then post a
//! `WM_APP`-range *thread* message to their own queue with
//! `PostThreadMessageW(GetCurrentThreadId())`. The loop handles those
//! messages with full access to [`ThreadState`] (sink, debouncer, the set of
//! windows already reported). `WINEVENTPROC` has no user pointer, which is
//! usually solved with a process-wide static holding the sink; routing
//! through the thread's own queue makes that static unnecessary, so nothing
//! here assumes a single instance (each thread has its own queue). The only
//! process-wide value is the registered `SHELLHOOK` message id, which is a
//! per-process constant by definition.
//!
//! **Translation.** The event → intent table and the title debounce policy
//! live in `rules::events` and are unit-tested on Linux. `WindowRemoved` is
//! only emitted for windows this thread reported earlier (`known`, which
//! also keeps the facts last reported about each, for the move path's
//! comparison), which keeps child-control `DESTROY` storms off the engine's
//! channel. A window that stops qualifying (hidden, cloaked by its app,
//! title emptied) is reported as removed; one that starts qualifying on any
//! update is reported as added, so a missed `SHOW` is healed by the next
//! event about it.
//!
//! **Re-checks.** Some windows are shown before they can be judged:
//! a Store app's `ApplicationFrameWindow` launched in the background was
//! seen rejected as `NotVisible` at both its create and its show event,
//! after which no event about it arrived until it took the foreground. The
//! schedule (which windows, when, when to give up, the 64-entry bound) is
//! `rules::recheck`; this thread keeps one [`ThreadTimer`] armed for the
//! earliest due re-check, and when it fires re-queries the due handles
//! through the same path a show takes (`query_and_report`). A destroy
//! (`EVENT_OBJECT_DESTROY`) forgets a pending window; any query that finds
//! it eligible, rejected for a final reason, or gone resolves it; a hide
//! does not cancel it. A snapshot resolves every window it lists.
//!
//! **Thread safety.** [`EventHookThread`] stores only the thread id, the
//! `JoinHandle`s and channel ends; raw handles (`HWND`, `HWINEVENTHOOK`,
//! timer ids) are `!Send` and never leave the events thread.
//!
//! # Sources
//!
//! Two OS mechanisms feed the loop, through the same queue and the same
//! `query_window_checked` path. Which drives which `PlatformEvent`:
//!
//! | `PlatformEvent`      | WinEvent (primary)                                    | Shell hook (second source) |
//! |----------------------|-------------------------------------------------------|----------------------------|
//! | `WindowAdded`        | `EVENT_OBJECT_CREATE`, `EVENT_OBJECT_SHOW` (+ re-checks) | `HSHELL_WINDOWCREATED` |
//! | `WindowRemoved`      | `EVENT_OBJECT_DESTROY`, `EVENT_OBJECT_HIDE`, any query that finds the window no longer eligible | — (`HSHELL_WINDOWDESTROYED` ignored, below) |
//! | `WindowUpdated`      | `EVENT_OBJECT_NAMECHANGE` (debounced), `EVENT_SYSTEM_MINIMIZESTART/END`, `EVENT_OBJECT_CLOAKED/UNCLOAKED`, `EVENT_OBJECT_LOCATIONCHANGE` (coalesced) | — (`HSHELL_REDRAW` ignored, below) |
//! | `ForegroundChanged`  | `EVENT_SYSTEM_FOREGROUND`                             | `HSHELL_WINDOWACTIVATED`, `HSHELL_RUDEAPPACTIVATED` |
//! | `WindowAttention`    | — (no WinEvent exists for a flash)                    | `HSHELL_FLASH` |
//!
//! **Duplicates.** The two sources report the same facts about the same
//! windows a few milliseconds apart, in no fixed order. They are coalesced
//! by `known`: the first source to find a new window eligible produces the
//! one `WindowAdded` (and the debug line `window 0x… added by <source>`);
//! the other finds it tracked and produces a `WindowUpdated` with the same
//! facts, which the engine's upsert absorbs (no fact changed, so nothing
//! downstream moves). A foreground switch reported by both yields two
//! `ForegroundChanged` with the same id, likewise absorbed, and arms the
//! fullscreen timer once. No duplicate `WindowAdded` can reach the engine
//! from this thread: `report_queried` consults `known` for every emission.
//!
//! **When one source is silent.** The WinEvent hooks are required:
//! `start` fails if one cannot be installed. The shell hook is optional: a
//! failed `RegisterShellHookWindow` logs a warning and the dock runs on
//! WinEvents alone, losing only the attention cue (`HSHELL_FLASH` has no
//! WinEvent equivalent; the other three shell codes duplicate a WinEvent).
//! A WinEvent missed for one window (for example a Store app's frame
//! judged too early) is healed by `HSHELL_WINDOWCREATED`, by the re-checks,
//! and by the foreground path, which reports an untracked candidate before
//! `ForegroundChanged`; a window missed by every event is listed by the next
//! snapshot (display change). Which source gets to a window first is what
//! the trace log shows: every received event is logged once
//! as `<EVENT_…> for window 0x…` or `<HSHELL_…> for window 0x…`, and every
//! shell code that is dropped as `<HSHELL_…> (0x…) ignored`.
//!
//! **Deliberately not translated.** `HSHELL_WINDOWDESTROYED` is left to
//! `EVENT_OBJECT_DESTROY`: the WinEvent is delivered for every window (the
//! callback accepts it even when `GetAncestor` can no longer see the
//! window), `EVENT_OBJECT_HIDE` covers a window that hides rather than
//! dies, and a removal is idempotent (`remove_window` emits only for a
//! tracked window), so the shell's code could only repeat a removal that
//! has happened or is about to. `HSHELL_REDRAW` (the taskbar's "title or
//! icon changed") is left out because titles already arrive as
//! `EVENT_OBJECT_NAMECHANGE` through the debouncer and window icons are
//! read once per session by the UI (`hwnd:` keys are not re-fetched), so it
//! would add a `WindowUpdated` with unchanged facts per repaint. The
//! remaining codes (`HSHELL_GETMINRECT`, `HSHELL_ACTIVATESHELLWINDOW`,
//! `HSHELL_TASKMAN`, `HSHELL_LANGUAGE`, `HSHELL_SYSMENU`, `HSHELL_ENDTASK`,
//! `HSHELL_ACCESSIBILITYSTATE`, `HSHELL_APPCOMMAND`,
//! `HSHELL_WINDOWREPLACED/REPLACING`, `HSHELL_MONITORCHANGED`) are about
//! the shell's own UI or carry no window; all are named by
//! `rules::events::shell_hook_name` and dropped after their trace line.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TryRecvError, bounded};
use stem_core::{MonitorId, PlatformEvent, RawWindow, RecycleBinState, ThemeInfo, WindowId};
use windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::{
    HPOWERNOTIFY, POWERBROADCAST_SETTING, RegisterPowerSettingNotification,
    UnregisterPowerSettingNotification,
};
use windows::Win32::System::SystemServices::{
    GUID_ACDC_POWER_SOURCE, GUID_BATTERY_PERCENTAGE_REMAINING, GUID_POWER_SAVING_STATUS,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Accessibility::{
    HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent, WINEVENTPROC,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CHILDID_SELF, CreateWindowExW, DEVICE_NOTIFY_WINDOW_HANDLE, DefWindowProcW,
    DeregisterShellHookWindow, DestroyWindow, DispatchMessageW, EVENT_OBJECT_CLOAKED,
    EVENT_OBJECT_CREATE, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE, EVENT_OBJECT_LOCATIONCHANGE,
    EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_SHOW, EVENT_OBJECT_UNCLOAKED, EVENT_SYSTEM_FOREGROUND,
    EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZESTART, GA_ROOT, GetAncestor,
    GetForegroundWindow, GetMessageW, HSHELL_ACCESSIBILITYSTATE, HSHELL_ACTIVATESHELLWINDOW,
    HSHELL_APPCOMMAND, HSHELL_ENDTASK, HSHELL_GETMINRECT, HSHELL_HIGHBIT, HSHELL_LANGUAGE,
    HSHELL_MONITORCHANGED, HSHELL_REDRAW, HSHELL_SYSMENU, HSHELL_TASKMAN, HSHELL_WINDOWACTIVATED,
    HSHELL_WINDOWCREATED, HSHELL_WINDOWDESTROYED, HSHELL_WINDOWREPLACED, HSHELL_WINDOWREPLACING,
    KillTimer, MSG, OBJID_WINDOW, PBT_POWERSETTINGCHANGE, PostThreadMessageW, RegisterClassExW,
    RegisterShellHookWindow, RegisterWindowMessageW, SPI_SETWORKAREA, SetTimer, TranslateMessage,
    UnregisterClassW, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_APP, WM_DISPLAYCHANGE,
    WM_DWMCOLORIZATIONCOLORCHANGED, WM_POWERBROADCAST, WM_QUIT, WM_SETTINGCHANGE, WM_THEMECHANGED,
    WM_TIMER, WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_OVERLAPPED,
};

use super::{
    actions, enumerate, fullscreen, locale, monitors, power, recycle_bin, theme, util,
    virtual_desktop,
};
use crate::rules::events::{
    self as rules, DesktopSwitchDetector, Intent, TitleDebouncer, TopLevel,
};
use crate::rules::locale as locale_rules;
use crate::rules::power as power_rules;
use crate::rules::recheck::{self, Check, RecheckSchedule, Scheduled};
use crate::{EventSink, PlatformError, PlatformResult};

// The host-independent constants in `rules::events` mirror winuser.h; prove
// them against the SDK here, where the `windows` crate is available.
const _: () = {
    assert!(rules::EVENT_SYSTEM_FOREGROUND == EVENT_SYSTEM_FOREGROUND);
    assert!(rules::EVENT_SYSTEM_MINIMIZESTART == EVENT_SYSTEM_MINIMIZESTART);
    assert!(rules::EVENT_SYSTEM_MINIMIZEEND == EVENT_SYSTEM_MINIMIZEEND);
    assert!(rules::EVENT_OBJECT_CREATE == EVENT_OBJECT_CREATE);
    assert!(rules::EVENT_OBJECT_DESTROY == EVENT_OBJECT_DESTROY);
    assert!(rules::EVENT_OBJECT_SHOW == EVENT_OBJECT_SHOW);
    assert!(rules::EVENT_OBJECT_HIDE == EVENT_OBJECT_HIDE);
    assert!(rules::EVENT_OBJECT_NAMECHANGE == EVENT_OBJECT_NAMECHANGE);
    assert!(rules::EVENT_OBJECT_CLOAKED == EVENT_OBJECT_CLOAKED);
    assert!(rules::EVENT_OBJECT_UNCLOAKED == EVENT_OBJECT_UNCLOAKED);
    assert!(rules::EVENT_OBJECT_LOCATIONCHANGE == EVENT_OBJECT_LOCATIONCHANGE);
    assert!(rules::OBJID_WINDOW == OBJID_WINDOW.0);
    assert!(rules::CHILDID_SELF == CHILDID_SELF as i32);
    assert!(rules::HSHELL_WINDOWCREATED == HSHELL_WINDOWCREATED);
    assert!(rules::HSHELL_WINDOWDESTROYED == HSHELL_WINDOWDESTROYED);
    assert!(rules::HSHELL_ACTIVATESHELLWINDOW == HSHELL_ACTIVATESHELLWINDOW);
    assert!(rules::HSHELL_WINDOWACTIVATED == HSHELL_WINDOWACTIVATED);
    assert!(rules::HSHELL_GETMINRECT == HSHELL_GETMINRECT);
    assert!(rules::HSHELL_REDRAW == HSHELL_REDRAW);
    assert!(rules::HSHELL_TASKMAN == HSHELL_TASKMAN);
    assert!(rules::HSHELL_LANGUAGE == HSHELL_LANGUAGE);
    assert!(rules::HSHELL_SYSMENU == HSHELL_SYSMENU);
    assert!(rules::HSHELL_ENDTASK == HSHELL_ENDTASK);
    assert!(rules::HSHELL_ACCESSIBILITYSTATE == HSHELL_ACCESSIBILITYSTATE);
    assert!(rules::HSHELL_APPCOMMAND == HSHELL_APPCOMMAND);
    assert!(rules::HSHELL_WINDOWREPLACED == HSHELL_WINDOWREPLACED);
    assert!(rules::HSHELL_WINDOWREPLACING == HSHELL_WINDOWREPLACING);
    assert!(rules::HSHELL_MONITORCHANGED == HSHELL_MONITORCHANGED);
    assert!(rules::HSHELL_HIGHBIT == HSHELL_HIGHBIT);
};

/// Thread messages (`MSG::hwnd == NULL`) posted by the hook callback and the
/// window procedure to the loop running on the same thread.
/// `wParam` = event id, `lParam` = `HWND`.
const MSG_WINEVENT: u32 = WM_APP + 1;
/// `wParam` = `HSHELL_*` code, `lParam` = `HWND`.
const MSG_SHELLHOOK: u32 = WM_APP + 2;
/// `wParam` = the `WM_SETTINGCHANGE` action (`SPI_*`, or 0); `lParam` =
/// [`SETTING_AREA_INTL`] when the message named the `intl` area,
/// else 0.
const MSG_SETTINGCHANGE: u32 = WM_APP + 3;
const MSG_THEMECHANGE: u32 = WM_APP + 4;
const MSG_DISPLAYCHANGE: u32 = WM_APP + 5;
/// A top-level window moved or resized; `lParam` = its `HWND`.
const MSG_LOCATIONCHANGE: u32 = WM_APP + 6;
/// A `WM_POWERBROADCAST` that may change the power reading;
/// `wParam` = the `PBT_*` code, `lParam` = the battery-saver state a
/// `GUID_POWER_SAVING_STATUS` notification carried
/// (`rules::power::saver_to_lparam`).
const MSG_POWERCHANGE: u32 = WM_APP + 7;

/// `MSG_SETTINGCHANGE`'s `lParam` for a `WM_SETTINGCHANGE` that named the
/// `intl` area.
const SETTING_AREA_INTL: isize = 1;

/// The most UTF-16 units of a `WM_SETTINGCHANGE` area name the window
/// procedure reads, up to its NUL. The one name it looks for is
/// `intl`; a longer name is not read to its end and is no match.
const SETTING_AREA_MAX: usize = 32;

/// The most bytes of a `POWERBROADCAST_SETTING`'s data the window procedure
/// looks at. The one value it reads is a `DWORD`; a larger
/// `DataLength` is cut to this, which `rules::power::notified_saver` then
/// rejects as not a `DWORD`.
const POWER_SETTING_DATA_MAX: usize = 16;

/// The power settings the hidden window is registered for, with
/// their names for the log.
const POWER_SETTINGS: [(&windows::core::GUID, &str); 3] = [
    (
        &GUID_BATTERY_PERCENTAGE_REMAINING,
        "GUID_BATTERY_PERCENTAGE_REMAINING",
    ),
    (&GUID_ACDC_POWER_SOURCE, "GUID_ACDC_POWER_SOURCE"),
    (&GUID_POWER_SAVING_STATUS, "GUID_POWER_SAVING_STATUS"),
];

const CLASS_NAME: &str = "DockPlatformEvents";
const WINDOW_TITLE: &str = "Stem platform events";
const SHELL_HOOK_MESSAGE_NAME: &str = "SHELLHOOK";

/// How often the Recycle Bin is queried (on its own thread).
pub const RECYCLE_BIN_POLL: Duration = Duration::from_secs(5);
/// How long after a foreground or location report the fullscreen verdict
/// is taken: one evaluation per burst, after the geometry settled.
pub const FULLSCREEN_SETTLE: Duration = Duration::from_millis(150);
/// How long after the first location report of a burst the moved windows
/// are re-queried: a window being dragged is reported to the engine
/// at most once per this interval.
pub const LOCATION_SETTLE: Duration = Duration::from_millis(150);
/// How long after a burst of cloak events (a virtual-desktop switch)
/// the windows are enumerated again: once the shell has finished cloaking
/// and uncloaking, one `WindowsSnapshot` puts every window right.
pub const DESKTOP_SWITCH_SETTLE: Duration =
    Duration::from_millis(DesktopSwitchDetector::SNAPSHOT_DELAY_MS);
/// How long after a `WM_SETTINGCHANGE` for `intl` the regional format is
/// read a second time: late enough for locale data that was not yet
/// current at the first read, soon enough that the clock follows within a
/// second.
pub const CLOCK_FORMAT_RECHECK: Duration = Duration::from_millis(500);
const START_TIMEOUT: Duration = Duration::from_secs(5);
const STOP_TIMEOUT: Duration = Duration::from_secs(2);

/// `RegisterWindowMessageW("SHELLHOOK")`: a per-process constant.
static SHELL_HOOK_MESSAGE: OnceLock<u32> = OnceLock::new();

/// Handle to the running event thread and its recycle-bin poller. Dropping
/// it without `stop` asks both threads to quit but does not wait for them;
/// prefer `stop`.
#[derive(Debug)]
pub struct EventHookThread {
    thread_id: u32,
    join: Option<JoinHandle<()>>,
    done: Receiver<()>,
    recycle_bin: Option<RecycleBinPoller>,
}

impl EventHookThread {
    pub fn start(sink: EventSink) -> PlatformResult<Self> {
        monitors::ensure_dpi_awareness();
        let (ready_tx, ready_rx) = bounded::<PlatformResult<u32>>(1);
        let (done_tx, done_rx) = bounded::<()>(1);
        let events_sink = Arc::clone(&sink);
        let join = std::thread::Builder::new()
            .name("stem-platform-events".into())
            .spawn(move || thread_main(events_sink, &ready_tx, &done_tx))
            .map_err(|e| PlatformError::Other(format!("spawn events thread: {e}")))?;
        match ready_rx.recv_timeout(START_TIMEOUT) {
            Ok(Ok(thread_id)) => {
                log::info!("events thread started (id {thread_id})");
                let mut this = Self {
                    thread_id,
                    join: Some(join),
                    done: done_rx,
                    recycle_bin: None,
                };
                match RecycleBinPoller::start(sink) {
                    Ok(poller) => this.recycle_bin = Some(poller),
                    // The dock works without the bin state; it just never
                    // flips the icon.
                    Err(e) => log::warn!("recycle-bin poll thread unavailable: {e}"),
                }
                Ok(this)
            }
            Ok(Err(e)) => {
                if join.join().is_err() {
                    log::error!("events thread panicked during setup");
                }
                Err(e)
            }
            Err(_) => {
                log::error!("events thread did not report readiness within {START_TIMEOUT:?}");
                Err(PlatformError::Timeout)
            }
        }
    }

    pub fn stop(mut self) {
        if let Some(poller) = self.recycle_bin.take() {
            poller.stop();
        }
        self.post_quit();
        match self.done.recv_timeout(STOP_TIMEOUT) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                if let Some(join) = self.join.take()
                    && join.join().is_err()
                {
                    log::error!("events thread panicked");
                }
                log::info!("events thread stopped");
            }
            Err(RecvTimeoutError::Timeout) => {
                log::warn!("events thread did not stop within {STOP_TIMEOUT:?}; detaching it");
            }
        }
    }

    fn post_quit(&self) {
        // SAFETY: WM_QUIT with zero parameters to a thread id we own; the
        // call fails harmlessly if the thread has already gone.
        if let Err(e) = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) }
        {
            log::debug!("PostThreadMessageW(WM_QUIT) failed: {e}");
        }
    }
}

impl Drop for EventHookThread {
    fn drop(&mut self) {
        // The poller, if still present, is dropped with us, which drops its
        // stop sender and ends its loop.
        if self.join.is_some() {
            self.post_quit();
        }
    }
}

/// The `dock-recycle-bin` thread: queries the Recycle Bin every
/// [`RECYCLE_BIN_POLL`], starting right away (the initial emission), and
/// calls the sink with `RecycleBinChanged` only when the state differs from
/// the last one it emitted. Stopped by dropping the stop sender; `stop`
/// also waits (bounded) for the thread, which may be inside a slow
/// `SHQueryRecycleBinW` — in that case it is detached and finishes on its
/// own without emitting (it checks for the stop before every emission).
#[derive(Debug)]
struct RecycleBinPoller {
    /// Dropping it wakes the thread and tells it to stop.
    stop: Option<Sender<()>>,
    join: Option<JoinHandle<()>>,
    done: Receiver<()>,
}

impl RecycleBinPoller {
    fn start(sink: EventSink) -> PlatformResult<Self> {
        let (stop_tx, stop_rx) = bounded::<()>(0);
        let (done_tx, done_rx) = bounded::<()>(1);
        let join = std::thread::Builder::new()
            .name("dock-recycle-bin".into())
            .spawn(move || {
                recycle_bin_poll_loop(&sink, &stop_rx);
                let _ = done_tx.send(());
            })
            .map_err(|e| PlatformError::Other(format!("spawn recycle-bin thread: {e}")))?;
        Ok(Self {
            stop: Some(stop_tx),
            join: Some(join),
            done: done_rx,
        })
    }

    fn stop(mut self) {
        self.stop.take();
        match self.done.recv_timeout(STOP_TIMEOUT) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                if let Some(join) = self.join.take()
                    && join.join().is_err()
                {
                    log::error!("recycle-bin poll thread panicked");
                }
                log::debug!("recycle-bin poll thread stopped");
            }
            Err(RecvTimeoutError::Timeout) => {
                log::warn!(
                    "recycle-bin poll thread did not stop within {STOP_TIMEOUT:?} (slow \
                     SHQueryRecycleBinW?); detaching it"
                );
            }
        }
    }
}

/// Poll until the stop sender is dropped. Errors are logged once per streak.
fn recycle_bin_poll_loop(sink: &EventSink, stop: &Receiver<()>) {
    let mut last: Option<RecycleBinState> = None;
    let mut error_logged = false;
    loop {
        match recycle_bin::query() {
            Ok(state) => {
                error_logged = false;
                if last != Some(state) {
                    // The query may have taken a while: do not emit into a
                    // platform that was stopped meanwhile.
                    if stop.try_recv() == Err(TryRecvError::Disconnected) {
                        break;
                    }
                    last = Some(state);
                    sink(PlatformEvent::RecycleBinChanged(state));
                }
            }
            Err(e) => {
                if !error_logged {
                    log::debug!("recycle bin unavailable: {e}");
                    error_logged = true;
                }
            }
        }
        // The sleep; a dropped sender ends it early.
        if stop.recv_timeout(RECYCLE_BIN_POLL) != Err(RecvTimeoutError::Timeout) {
            break;
        }
    }
    log::debug!("recycle-bin poll loop exiting");
}

fn thread_main(sink: EventSink, ready: &Sender<PlatformResult<u32>>, done: &Sender<()>) {
    match ThreadState::setup(sink) {
        Ok(mut state) => {
            if ready.send(Ok(state.thread_id)).is_ok() {
                state.emit_initial();
                state.run();
            }
            log::debug!("events thread releasing hooks, timers and window");
            drop(state);
        }
        Err(e) => {
            let _ = ready.send(Err(e));
        }
    }
    let _ = done.send(());
}

/// `SetWinEventHook` guard; unhooked on drop (on the installing thread).
struct WinEventHook(HWINEVENTHOOK);

impl WinEventHook {
    /// A hook for the translation table's events, delivered to [`win_event_proc`].
    fn install(event_min: u32, event_max: u32) -> PlatformResult<Self> {
        Self::install_with(event_min, event_max, Some(win_event_proc))
    }

    fn install_with(
        event_min: u32,
        event_max: u32,
        callback: WINEVENTPROC,
    ) -> PlatformResult<Self> {
        // SAFETY: the callback is a static `extern "system"` function with the
        // WINEVENTPROC signature; out-of-context hooks need no module handle.
        let hook = unsafe {
            SetWinEventHook(
                event_min,
                event_max,
                None,
                callback,
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        };
        if hook.is_invalid() {
            return Err(PlatformError::Other(format!(
                "SetWinEventHook({event_min:#06x}..={event_max:#06x}) failed"
            )));
        }
        Ok(Self(hook))
    }
}

impl Drop for WinEventHook {
    fn drop(&mut self) {
        // SAFETY: the hook was installed by this thread and is removed once.
        if !unsafe { UnhookWinEvent(self.0) }.as_bool() {
            log::warn!("UnhookWinEvent failed");
        }
    }
}

/// A `RegisterPowerSettingNotification` of the hidden window;
/// unregistered on drop, on the events thread, before the window goes.
struct PowerNotification {
    handle: HPOWERNOTIFY,
    name: &'static str,
}

impl PowerNotification {
    /// Register `hwnd` for every setting of [`POWER_SETTINGS`]; the ones that
    /// fail are logged and left out.
    fn register_all(hwnd: HWND) -> Vec<Self> {
        let mut registered = Vec::with_capacity(POWER_SETTINGS.len());
        for (guid, name) in POWER_SETTINGS {
            // SAFETY: `hwnd` is a live window owned by this thread, passed as
            // the recipient handle DEVICE_NOTIFY_WINDOW_HANDLE asks for; the
            // GUID is a static the call only reads.
            let result = unsafe {
                RegisterPowerSettingNotification(HANDLE(hwnd.0), guid, DEVICE_NOTIFY_WINDOW_HANDLE)
            };
            match result {
                Ok(handle) => registered.push(Self { handle, name }),
                Err(e) => log::warn!(
                    "RegisterPowerSettingNotification({name}) failed: {e}; the power status \
                     broadcast still updates the battery"
                ),
            }
        }
        log::debug!(
            "power settings registered: {}",
            registered
                .iter()
                .map(|n| n.name)
                .collect::<Vec<_>>()
                .join(", ")
        );
        registered
    }
}

impl Drop for PowerNotification {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful
        // RegisterPowerSettingNotification and is unregistered once.
        if let Err(e) = unsafe { UnregisterPowerSettingNotification(self.handle) } {
            log::debug!(
                "UnregisterPowerSettingNotification({}) failed: {e}",
                self.name
            );
        }
    }
}

/// The hidden window that receives broadcasts and shell-hook messages.
struct EventWindow {
    hwnd: HWND,
    class_name: Vec<u16>,
    instance: HINSTANCE,
    shell_hook_registered: bool,
}

impl EventWindow {
    fn create() -> PlatformResult<Self> {
        // SAFETY: a NULL module name asks for our own executable's handle.
        let module = unsafe { GetModuleHandleW(None) }?;
        let instance = HINSTANCE::from(module);
        let class_name = util::to_wide(CLASS_NAME);
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(event_wndproc),
            hInstance: instance,
            lpszClassName: util::pcwstr(&class_name),
            ..Default::default()
        };
        // SAFETY: `class` is fully initialised and the name buffer outlives the call.
        if unsafe { RegisterClassExW(&class) } == 0 {
            let err = windows::core::Error::from_thread();
            if err.code() != util::hresult_of(ERROR_CLASS_ALREADY_EXISTS) {
                return Err(err.into());
            }
            // Left over from a previous start in this process; the procedure
            // is the same static function, so reusing the class is fine.
        }
        let title = util::to_wide(WINDOW_TITLE);
        // SAFETY: the class is registered; the string buffers outlive the call;
        // no parent, menu or creation parameter is needed.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                util::pcwstr(&class_name),
                util::pcwstr(&title),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                None,
            )
        }?;
        let mut window = Self {
            hwnd,
            class_name,
            instance,
            shell_hook_registered: false,
        };
        let message_name = util::to_wide(SHELL_HOOK_MESSAGE_NAME);
        // SAFETY: the name buffer outlives the call.
        let message = unsafe { RegisterWindowMessageW(util::pcwstr(&message_name)) };
        if message == 0 {
            log::warn!(
                "RegisterWindowMessageW(SHELLHOOK) failed; flash/activation shell events off"
            );
        } else {
            let _ = SHELL_HOOK_MESSAGE.set(message);
            // SAFETY: `hwnd` is a live window owned by this thread.
            window.shell_hook_registered = unsafe { RegisterShellHookWindow(hwnd) }.as_bool();
            if !window.shell_hook_registered {
                log::warn!("RegisterShellHookWindow failed; flash/activation shell events off");
            }
        }
        Ok(window)
    }
}

impl Drop for EventWindow {
    fn drop(&mut self) {
        if self.shell_hook_registered {
            // SAFETY: balances the successful RegisterShellHookWindow on this window.
            if !unsafe { DeregisterShellHookWindow(self.hwnd) }.as_bool() {
                log::debug!("DeregisterShellHookWindow failed");
            }
        }
        // SAFETY: the window was created by this thread and is destroyed once.
        if let Err(e) = unsafe { DestroyWindow(self.hwnd) } {
            log::warn!("DestroyWindow(events window) failed: {e}");
        }
        let class_name = util::pcwstr(&self.class_name);
        // SAFETY: the class name buffer is alive; the only window of the class
        // has just been destroyed.
        let unregistered = unsafe { UnregisterClassW(class_name, Some(self.instance)) };
        if let Err(e) = unregistered {
            log::debug!("UnregisterClassW failed: {e}");
        }
    }
}

/// A thread timer (`SetTimer` with no window); `WM_TIMER` arrives as a
/// thread message with `wParam` = the id. Killed on drop.
struct ThreadTimer(usize);

impl ThreadTimer {
    fn start(interval: Duration) -> Option<Self> {
        let millis = u32::try_from(interval.as_millis())
            .unwrap_or(u32::MAX)
            .max(1);
        // SAFETY: a NULL window and NULL callback create a thread timer whose
        // messages are delivered to this thread's queue.
        let id = unsafe { SetTimer(None, 0, millis, None) };
        if id == 0 {
            log::warn!("SetTimer({millis} ms) failed");
            return None;
        }
        Some(Self(id))
    }

    fn id(&self) -> usize {
        self.0
    }
}

impl Drop for ThreadTimer {
    fn drop(&mut self) {
        // SAFETY: the id came from SetTimer(None, ..) on this thread.
        if let Err(e) = unsafe { KillTimer(None, self.0) } {
            log::debug!("KillTimer failed: {e}");
        }
    }
}

/// What a successful or failed query of one window was reported as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Report {
    /// `WindowAdded`: the window was not tracked before.
    Added,
    /// `WindowUpdated`.
    Updated,
    /// Nothing: the facts equal the last report (only on the move path).
    Unchanged,
    /// `WindowRemoved` (or nothing, when it was not tracked): it no longer qualifies.
    Removed,
}

/// Everything the loop owns. Field order is drop order: the timers, then
/// hooks, then the power-setting registrations, then the window, then the
/// virtual desktop manager, then the COM apartment last.
struct ThreadState {
    sink: EventSink,
    thread_id: u32,
    started: Instant,
    /// Windows reported to the engine (snapshot or `WindowAdded`) and not
    /// yet removed, each with the facts last reported about it (what a
    /// coalesced re-query after a move compares against).
    known: HashMap<WindowId, RawWindow>,
    /// Tracked windows that moved or resized since the last flush.
    moved: HashSet<WindowId>,
    /// The [`LOCATION_SETTLE`] timer, while a flush of `moved` is pending.
    location_timer: Option<ThreadTimer>,
    /// The foreground window as this thread last saw it (the raw `HWND`
    /// value; 0 for none): a move of it re-evaluates fullscreen.
    foreground_hwnd: isize,
    titles: TitleDebouncer,
    title_timer: Option<ThreadTimer>,
    /// Windows not ready when they appeared, keyed by `WindowId.0`.
    rechecks: RecheckSchedule,
    /// The timer armed for a re-check, with the due time (ms) it was armed
    /// for; never later than `rechecks.next_due()`.
    recheck_timer: Option<(ThreadTimer, u64)>,
    /// The last fullscreen verdict emitted, with the monitor it
    /// named; `None` before the first.
    fullscreen: Option<(bool, Option<MonitorId>)>,
    /// The [`FULLSCREEN_SETTLE`] timer, while an evaluation is pending.
    fullscreen_timer: Option<ThreadTimer>,
    /// Cloak events counted towards a virtual-desktop switch.
    switch: DesktopSwitchDetector,
    /// The [`DESKTOP_SWITCH_SETTLE`] timer, while a snapshot is pending.
    switch_timer: Option<ThreadTimer>,
    /// The [`CLOCK_FORMAT_RECHECK`] timer, while the second read after an
    /// `intl` broadcast is pending.
    clock_format_timer: Option<ThreadTimer>,
    last_theme: Option<ThemeInfo>,
    /// The power readings already emitted.
    power: power_rules::ChangeFilter,
    /// The battery-saver state of the last `GUID_POWER_SAVING_STATUS`
    /// notification; `None` before the first.
    notified_saver: Option<bool>,
    /// The regional-format readings already emitted.
    clock_format: locale_rules::ChangeFilter,
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
    fn setup(sink: EventSink) -> PlatformResult<Self> {
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

    fn emit(&self, event: PlatformEvent) {
        (self.sink)(event);
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// Same order as the mock platform (monitors, theme, windows,
    /// foreground, power, clock format), minus the recycle bin, which the
    /// poll thread emits; the first fullscreen verdict comes before
    /// the power reading, so the engine starts right.
    fn emit_initial(&mut self) {
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

    fn run(&mut self) {
        loop {
            let mut msg = MSG::default();
            // SAFETY: `msg` is a live out-pointer; a NULL filter window asks
            // for every message of this thread.
            let result = unsafe { GetMessageW(&mut msg, None, 0, 0) };
            match result.0 {
                0 => break,
                -1 => {
                    log::error!(
                        "GetMessageW failed: {}; stopping",
                        windows::core::Error::from_thread()
                    );
                    break;
                }
                _ => {}
            }
            if msg.hwnd.is_invalid() {
                self.on_thread_message(&msg);
                continue;
            }
            // SAFETY: `msg` was filled in by GetMessageW.
            let _translated = unsafe { TranslateMessage(&msg) };
            // SAFETY: `msg` was filled in by GetMessageW.
            let _result = unsafe { DispatchMessageW(&msg) };
        }
    }

    fn on_thread_message(&mut self, msg: &MSG) {
        let hwnd = HWND(msg.lParam.0 as *mut core::ffi::c_void);
        match msg.message {
            MSG_WINEVENT => self.on_win_event(msg.wParam.0 as u32, hwnd),
            MSG_SHELLHOOK => self.on_shell_hook(msg.wParam.0 as u32, hwnd),
            MSG_SETTINGCHANGE => {
                if msg.wParam.0 as u32 == SPI_SETWORKAREA.0 {
                    // The stock taskbar changed size or auto-hide state.
                    self.emit_monitors();
                }
                self.refresh_theme(false);
                if msg.lParam.0 == SETTING_AREA_INTL {
                    self.on_intl_change();
                }
            }
            MSG_THEMECHANGE => self.refresh_theme(false),
            MSG_DISPLAYCHANGE => {
                self.emit_monitors();
                self.emit_snapshot();
                // The monitor bounds a frame is judged against changed.
                self.schedule_fullscreen_check();
            }
            MSG_LOCATIONCHANGE => self.on_location_change(hwnd),
            MSG_POWERCHANGE => {
                if let Some(on) = power_rules::saver_from_lparam(msg.lParam.0) {
                    log::debug!(
                        "GUID_POWER_SAVING_STATUS: battery saver {}",
                        if on { "on" } else { "off" }
                    );
                    self.notified_saver = Some(on);
                }
                self.refresh_power(power_rules::power_broadcast_name(msg.wParam.0 as u32));
            }
            WM_TIMER => self.on_timer(msg.wParam.0),
            _ => {}
        }
    }

    fn on_win_event(&mut self, event: u32, hwnd: HWND) {
        let cause = rules::event_name(event);
        // One line per event that passed the callback's filter, so a trace
        // log can be counted per source against the shell hook's.
        log::trace!("{cause} for window {:#x}", hwnd.0 as isize);
        match rules::event_intent(event) {
            Intent::Added => self.on_window_shown(hwnd, cause),
            Intent::Updated => {
                if event == EVENT_OBJECT_CLOAKED || event == EVENT_OBJECT_UNCLOAKED {
                    self.on_cloak_event();
                }
                self.refresh_window(hwnd, cause);
            }
            Intent::Removed => {
                let id = util::window_id(hwnd);
                if event == EVENT_OBJECT_DESTROY && self.rechecks.forget(id.0) {
                    // A hide keeps a pending re-check; a destroy ends it.
                    log::trace!("window {:#x} destroyed; its re-checks are dropped", id.0);
                }
                self.remove_window(id);
            }
            Intent::TitleChanged => self.on_title_changed(util::window_id(hwnd)),
            Intent::Foreground => self.emit_foreground(hwnd),
            Intent::Attention | Intent::Ignore => {}
        }
    }

    fn on_shell_hook(&mut self, code: u32, hwnd: HWND) {
        let name = rules::shell_hook_name(code);
        let intent = rules::shell_hook_intent(code);
        if intent == Intent::Ignore {
            // Dropped, but counted. `lParam` is not a window for
            // every code (`HSHELL_GETMINRECT`, `HSHELL_APPCOMMAND`,
            // `HSHELL_LANGUAGE`), hence the raw value.
            log::trace!("{name} ({code:#x}) ignored; lParam {:#x}", hwnd.0 as isize);
            return;
        }
        log::trace!("{name} for window {:#x}", hwnd.0 as isize);
        match intent {
            Intent::Added => self.on_window_shown(hwnd, name),
            Intent::Foreground => self.emit_foreground(hwnd),
            Intent::Attention => {
                let id = util::window_id(hwnd);
                if !self.known.contains_key(&id) {
                    self.refresh_window(hwnd, name);
                }
                if self.known.contains_key(&id) {
                    self.emit(PlatformEvent::WindowAttention { id, flashing: true });
                }
            }
            Intent::Removed | Intent::Updated | Intent::TitleChanged | Intent::Ignore => {}
        }
    }

    fn on_timer(&mut self, timer_id: usize) {
        if self
            .title_timer
            .as_ref()
            .is_some_and(|t| t.id() == timer_id)
        {
            self.flush_titles();
        }
        if self
            .recheck_timer
            .as_ref()
            .is_some_and(|(t, _)| t.id() == timer_id)
        {
            self.run_rechecks();
        }
        if self
            .fullscreen_timer
            .as_ref()
            .is_some_and(|t| t.id() == timer_id)
        {
            self.check_fullscreen();
        }
        if self
            .location_timer
            .as_ref()
            .is_some_and(|t| t.id() == timer_id)
        {
            self.flush_moved();
        }
        if self
            .switch_timer
            .as_ref()
            .is_some_and(|t| t.id() == timer_id)
        {
            self.on_desktop_switch_settled();
        }
        if self
            .clock_format_timer
            .as_ref()
            .is_some_and(|t| t.id() == timer_id)
        {
            // Thread timers repeat; this one is one-shot.
            self.clock_format_timer = None;
            self.refresh_clock_format("re-read after intl");
        }
    }

    /// An `EVENT_OBJECT_CLOAKED` or `EVENT_OBJECT_UNCLOAKED` about a
    /// top-level window: the second within 100 ms is a virtual
    /// desktop switch in progress, so one snapshot is taken
    /// [`DESKTOP_SWITCH_SETTLE`] from now; when no timer can be had it is
    /// taken at once rather than never.
    fn on_cloak_event(&mut self) {
        let now = self.now_ms();
        if !self.switch.observe(now) {
            return;
        }
        log::trace!("cloak burst: a desktop switch; snapshot in {DESKTOP_SWITCH_SETTLE:?}");
        match ThreadTimer::start(DESKTOP_SWITCH_SETTLE) {
            Some(timer) => self.switch_timer = Some(timer),
            None => self.on_desktop_switch_settled(),
        }
    }

    /// The switch timer fired: enumerate everything again, so the windows
    /// of both desktops are reported as the shell sees them now.
    fn on_desktop_switch_settled(&mut self) {
        // Thread timers repeat; this one is one-shot.
        self.switch_timer = None;
        log::debug!("desktop switch settled; re-enumerating");
        self.emit_snapshot();
    }

    /// A top-level window moved or resized: a tracked one is
    /// re-queried at the next flush; the foreground one also re-evaluates
    /// fullscreen. Untracked windows cost nothing here.
    fn on_location_change(&mut self, hwnd: HWND) {
        if hwnd.0 as isize == self.foreground_hwnd {
            self.schedule_fullscreen_check();
        }
        let id = util::window_id(hwnd);
        if !self.known.contains_key(&id) {
            return;
        }
        if self.moved.insert(id) && self.moved.len() == 1 {
            log::trace!("window {:#x} is moving; flush in {LOCATION_SETTLE:?}", id.0);
        }
        self.schedule_location_flush();
    }

    /// Make sure the moved windows are re-queried [`LOCATION_SETTLE`] from
    /// now. A timer already armed is kept, so a burst costs one query per
    /// window; when no timer can be had the flush runs at once rather than
    /// never.
    fn schedule_location_flush(&mut self) {
        if self.location_timer.is_some() {
            return;
        }
        match ThreadTimer::start(LOCATION_SETTLE) {
            Some(timer) => self.location_timer = Some(timer),
            None => self.flush_moved(),
        }
    }

    /// The location timer fired: re-query every window that moved since the
    /// last flush and is still tracked, and report the ones whose facts
    /// changed (`WindowUpdated`) or that no longer qualify (`WindowRemoved`).
    fn flush_moved(&mut self) {
        // Thread timers repeat; this one is one-shot.
        self.location_timer = None;
        let mut ids: Vec<WindowId> = self.moved.drain().collect();
        ids.sort_unstable();
        let mut updated = 0usize;
        let mut removed = 0usize;
        for id in &ids {
            if !self.known.contains_key(id) {
                continue;
            }
            match self.query_window(util::hwnd(*id), true).1 {
                Report::Updated => updated += 1,
                Report::Removed => removed += 1,
                Report::Added | Report::Unchanged => {}
            }
        }
        if !ids.is_empty() {
            log::trace!(
                "{} moved window(s) re-queried: {updated} updated, {removed} no longer qualify",
                ids.len()
            );
        }
    }

    /// Something that can change the fullscreen verdict happened:
    /// make sure one evaluation runs [`FULLSCREEN_SETTLE`] from now. A timer
    /// already armed is kept, so a burst costs one evaluation; when no timer
    /// can be had the evaluation runs at once rather than never.
    fn schedule_fullscreen_check(&mut self) {
        if self.fullscreen_timer.is_some() {
            return;
        }
        match ThreadTimer::start(FULLSCREEN_SETTLE) {
            Some(timer) => self.fullscreen_timer = Some(timer),
            None => self.check_fullscreen(),
        }
    }

    /// Judge the live foreground window and emit `FullscreenChanged`
    /// when the verdict differs from the last one emitted, or when it is the
    /// first.
    fn check_fullscreen(&mut self) {
        // Thread timers repeat; this one is one-shot.
        self.fullscreen_timer = None;
        // SAFETY: GetForegroundWindow has no preconditions; NULL is a valid answer.
        let foreground = unsafe { GetForegroundWindow() };
        self.foreground_hwnd = foreground.0 as isize;
        let verdict = fullscreen::evaluate(foreground);
        let next = (verdict.fullscreen, verdict.monitor);
        if self.fullscreen.as_ref() == Some(&next) {
            return;
        }
        let first = self.fullscreen.is_none();
        self.fullscreen = Some(next.clone());
        let (active, monitor) = next;
        if active {
            log::info!(
                "fullscreen on: {} {:?} on {}",
                verdict.class_name,
                verdict.title,
                monitor
                    .as_ref()
                    .map_or("an unknown monitor", |m| m.0.as_str())
            );
        } else if first {
            log::debug!("fullscreen off at start");
        } else {
            log::info!("fullscreen off");
        }
        self.emit(PlatformEvent::FullscreenChanged { active, monitor });
    }

    /// Re-query a window and report it as added, updated or removed, and
    /// resolve a pending re-check that the result settles. Returns how the
    /// window stands and whether this call added it.
    fn query_and_report(&mut self, hwnd: HWND) -> (Check, bool) {
        let (check, report) = self.query_window(hwnd, false);
        (check, report == Report::Added)
    }

    /// The query behind [`Self::query_and_report`] and the move path:
    /// with `only_changes` a `WindowUpdated` goes out only when the
    /// facts differ from the last report, so a window dragged around costs
    /// the engine nothing while its frame is unchanged between two flushes,
    /// and a tracked window keeps the desktop answer last reported instead
    /// of asking the shell again (a move does not change the
    /// desktop, and the shell's call has no timeout).
    fn query_window(&mut self, hwnd: HWND, only_changes: bool) -> (Check, Report) {
        let id = util::window_id(hwnd);
        let queried = match self.known.get(&id).filter(|_| only_changes) {
            Some(last) => enumerate::query_moved_window(hwnd, last.on_current_desktop),
            None => enumerate::query_window_checked(hwnd),
        };
        let (check, report) = match queried {
            Ok(raw) => (Check::Eligible, self.report_queried(id, raw, only_changes)),
            Err(check) => {
                if only_changes {
                    log::trace!("moved window {:#x} no longer qualifies: {check:?}", id.0);
                }
                self.remove_window(id);
                (check, Report::Removed)
            }
        };
        self.rechecks.observe(id.0, check);
        (check, report)
    }

    /// Emit what a successful query of `id` means: `WindowAdded` for a
    /// window not tracked yet, else `WindowUpdated`, unless `only_changes`
    /// and the facts equal the last report. `known` keeps the facts either
    /// way.
    fn report_queried(&mut self, id: WindowId, raw: RawWindow, only_changes: bool) -> Report {
        match self.known.get(&id) {
            None => {
                self.known.insert(id, raw.clone());
                self.emit(PlatformEvent::WindowAdded(raw));
                Report::Added
            }
            Some(previous) if only_changes && *previous == raw => Report::Unchanged,
            Some(_) => {
                if only_changes {
                    log::trace!(
                        "moved window {:#x}: WindowUpdated, frame {:?}",
                        id.0,
                        raw.rect
                    );
                }
                self.known.insert(id, raw.clone());
                self.emit(PlatformEvent::WindowUpdated(raw));
                Report::Updated
            }
        }
    }

    /// [`Self::query_and_report`] for an event about the window (`cause`
    /// names it for the log). A window the re-check schedule was waiting for
    /// that this call adds is logged as late.
    fn refresh_window(&mut self, hwnd: HWND, cause: &str) -> Check {
        let id = util::window_id(hwnd);
        let pending_since = self.rechecks.pending_since(id.0);
        let (check, added) = self.query_and_report(hwnd);
        // One `added` line per `WindowAdded`, naming the source that won
        // (one per window, whichever source came first).
        if added {
            match pending_since {
                Some(since) => log::debug!(
                    "late window {:#x} added by {cause}, {} ms after it was first rejected",
                    id.0,
                    self.now_ms().saturating_sub(since)
                ),
                None => log::debug!("window {:#x} added by {cause}", id.0),
            }
        }
        check
    }

    /// A show-type event (`EVENT_OBJECT_CREATE`, `EVENT_OBJECT_SHOW`,
    /// `HSHELL_WINDOWCREATED`): report the window if it qualifies; when it
    /// is only not ready yet (hidden or cloaked, every other rule passing),
    /// schedule its re-checks.
    fn on_window_shown(&mut self, hwnd: HWND, cause: &str) {
        let id = util::window_id(hwnd);
        let check = self.refresh_window(hwnd, cause);
        let now = self.now_ms();
        if let Scheduled::Yes { evicted } = self.rechecks.on_show(id.0, check, now) {
            log::trace!(
                "window {:#x} not ready at {cause} ({check:?}); re-checks {:?} ms later",
                id.0,
                recheck::DELAYS_MS
            );
            if let Some(old) = evicted {
                log::trace!(
                    "re-check queue full ({} windows); dropped the oldest, {old:#x}",
                    recheck::MAX_PENDING
                );
            }
            self.arm_recheck_timer();
        }
    }

    /// The re-check timer fired: query every due window again, the same way
    /// a show does, and re-arm for the next one.
    fn run_rechecks(&mut self) {
        // Thread timers repeat; stop this one, the next is armed below.
        self.recheck_timer = None;
        let now = self.now_ms();
        for due in self.rechecks.take_due(now) {
            let (check, added) = self.query_and_report(util::hwnd(WindowId(due.id)));
            let outcome = match check {
                Check::Eligible if added => "added",
                Check::Eligible => "already listed",
                Check::Transient(_) if due.is_last => "still not ready; giving up",
                Check::Transient(_) => "still not ready",
                Check::Final(_) => "will not qualify; dropped",
                Check::Gone => "gone; dropped",
            };
            log::trace!(
                "re-check {}/{} of window {:#x}: {check:?}, {outcome}",
                due.attempt,
                recheck::ATTEMPTS,
                due.id
            );
            if added {
                log::debug!(
                    "late window {:#x} added on re-check {}/{}, {} ms after it was first rejected",
                    due.id,
                    due.attempt,
                    recheck::ATTEMPTS,
                    now.saturating_sub(due.since_ms)
                );
            }
        }
        self.arm_recheck_timer();
    }

    /// Make sure a timer fires by the earliest pending re-check. A timer
    /// armed for an earlier time is kept: if what it was armed for has been
    /// resolved meanwhile, it finds nothing due and `run_rechecks` re-arms
    /// it or lets it stop.
    fn arm_recheck_timer(&mut self) {
        let Some(due) = self.rechecks.next_due() else {
            return;
        };
        if self
            .recheck_timer
            .as_ref()
            .is_some_and(|&(_, armed_for)| armed_for <= due)
        {
            return;
        }
        let wait = due.saturating_sub(self.now_ms()).max(1);
        self.recheck_timer =
            ThreadTimer::start(Duration::from_millis(wait)).map(|timer| (timer, due));
    }

    fn remove_window(&mut self, id: WindowId) {
        self.titles.forget(id.0);
        self.moved.remove(&id);
        if self.known.remove(&id).is_some() {
            // The pair of the `added` line; the trace line before it
            // names the event that caused it.
            log::debug!("window {:#x} removed", id.0);
            self.emit(PlatformEvent::WindowRemoved(id));
        }
    }

    fn on_title_changed(&mut self, id: WindowId) {
        let now = self.now_ms();
        if self.titles.record(id.0, now) && self.title_timer.is_none() {
            self.title_timer = ThreadTimer::start(Duration::from_millis(TitleDebouncer::WINDOW_MS));
        }
    }

    fn flush_titles(&mut self) {
        let now = self.now_ms();
        for id in self.titles.take_due(now) {
            self.refresh_window(util::hwnd(WindowId(id)), "EVENT_OBJECT_NAMECHANGE");
        }
        // Re-arm for the next pending flush, or stop the timer.
        self.title_timer = self.titles.next_due().and_then(|due| {
            ThreadTimer::start(Duration::from_millis(due.saturating_sub(now).max(1)))
        });
    }

    fn emit_foreground(&mut self, hwnd: HWND) {
        // A foreground change is when a dock that has silently
        // dropped out of the topmost band gets covered; the check is posted.
        super::dock_window::assert_topmost_all();
        // Moves of this window re-evaluate fullscreen from now on, and
        // the verdict is taken once the switch has settled. Our own windows
        // count too: the settings window in front of a game ends fullscreen.
        self.foreground_hwnd = hwnd.0 as isize;
        self.schedule_fullscreen_check();
        if !hwnd.is_invalid() && util::is_own_process_window(hwnd) {
            // The dock (or the settings window) got the foreground: not an
            // app the user switched to, so the previous foreground stays.
            log::trace!(
                "foreground report for our own window {:#x} ignored",
                hwnd.0 as isize
            );
            return;
        }
        let id = (!hwnd.is_invalid() && enumerate::is_taskbar_candidate(hwnd))
            .then(|| util::window_id(hwnd));
        if let Some(id) = id
            && !self.known.contains_key(&id)
        {
            // A candidate we never saw shown (missed event): report it first
            // so the engine can mark it active.
            self.refresh_window(hwnd, "the foreground change");
        }
        self.emit(PlatformEvent::ForegroundChanged(id));
    }

    fn emit_snapshot(&mut self) {
        // Whatever triggered this snapshot, it is the one a cloak burst was
        // waiting for.
        self.switch.reset();
        self.switch_timer = None;
        match enumerate::enumerate_taskbar_windows() {
            Ok(windows) => {
                self.known = windows.iter().map(|w| (w.id, w.clone())).collect();
                self.moved.clear();
                self.titles = TitleDebouncer::default();
                self.title_timer = None;
                // Listed now; windows still not ready stay pending.
                for w in &windows {
                    self.rechecks.forget(w.id.0);
                }
                self.emit(PlatformEvent::WindowsSnapshot(windows));
            }
            Err(e) => log::warn!("window enumeration failed: {e}"),
        }
    }

    fn emit_monitors(&self) {
        match monitors::enumerate_monitors() {
            Ok(monitors) => self.emit(PlatformEvent::MonitorsChanged(monitors)),
            Err(e) => log::warn!("monitor enumeration failed: {e}"),
        }
    }

    /// Read the power status, with battery saver as the last
    /// `GUID_POWER_SAVING_STATUS` notification said once one has arrived,
    /// and emit `PowerChanged` when it is the first reading or differs from
    /// the last one emitted; `cause` names what asked, for the log. A failed
    /// read is logged and emits nothing.
    fn refresh_power(&mut self, cause: &str) {
        match power::current() {
            Ok(read) => {
                let status = power_rules::with_notified_saver(read, self.notified_saver);
                if status.saver != read.saver {
                    log::debug!(
                        "SystemStatusFlag says battery saver {}, the last \
                         GUID_POWER_SAVING_STATUS notification {}; using the notification",
                        if read.saver { "on" } else { "off" },
                        if status.saver { "on" } else { "off" }
                    );
                }
                if self.power.observe(status) {
                    log::debug!("power: {status:?} ({cause})");
                    self.emit(PlatformEvent::PowerChanged(status));
                } else {
                    log::trace!("power unchanged ({cause})");
                }
            }
            Err(e) => log::warn!("GetSystemPowerStatus failed ({cause}): {e}"),
        }
    }

    /// A `WM_SETTINGCHANGE` named the `intl` area: the regional
    /// format or one of its formats changed. Read it now, and once more
    /// [`CLOCK_FORMAT_RECHECK`] from now (a pending second read is moved to
    /// then; replacing the timer kills the old one).
    fn on_intl_change(&mut self) {
        self.refresh_clock_format("WM_SETTINGCHANGE intl");
        self.clock_format_timer = ThreadTimer::start(CLOCK_FORMAT_RECHECK);
    }

    /// Read the user's regional format and emit
    /// `ClockFormatChanged` when it is the first reading or differs from the
    /// last one emitted; `cause` names what asked, for the log.
    fn refresh_clock_format(&mut self, cause: &str) {
        let format = locale::current();
        if self.clock_format.observe(&format) {
            log::debug!("clock format: {format:?} ({cause})");
            self.emit(PlatformEvent::ClockFormatChanged(format));
        } else {
            log::trace!("clock format unchanged ({cause})");
        }
    }

    /// Emit `ThemeChanged` when the theme differs from the last one emitted
    /// (or always, when `force`). Errors are skipped: the theme module is
    /// another task's and may still be a stub.
    fn refresh_theme(&mut self, force: bool) {
        match theme::current_theme() {
            Ok(theme) => {
                if force || self.last_theme.as_ref() != Some(&theme) {
                    self.last_theme = Some(theme.clone());
                    self.emit(PlatformEvent::ThemeChanged(theme));
                }
            }
            Err(e) => log::debug!("theme unavailable: {e}"),
        }
    }
}

/// The battery-saver state a `PBT_POWERSETTINGCHANGE` carries:
/// `rules::power::notified_saver` of the `POWERBROADCAST_SETTING` behind
/// `lparam`, copied out while the window procedure runs; `None` for a NULL
/// `lparam` and for every other setting. Reads the header and at most
/// [`POWER_SETTING_DATA_MAX`] bytes of the data, without assuming alignment.
///
/// # Safety
///
/// `lparam` is NULL or points at a readable `POWERBROADCAST_SETTING` whose
/// `DataLength` bytes of data follow at `Data`, as the `lParam` of a
/// `WM_POWERBROADCAST` with `PBT_POWERSETTINGCHANGE` does while it is being
/// handled.
unsafe fn power_setting_saver(lparam: LPARAM) -> Option<bool> {
    let base = lparam.0 as *const u8;
    if base.is_null() {
        return None;
    }
    // The fields' addresses, by the structure's own offsets; no reference to
    // the structure is made, and nothing assumes its alignment.
    let field = |offset: usize| base.wrapping_add(offset);
    let guid = field(core::mem::offset_of!(POWERBROADCAST_SETTING, PowerSetting));
    let length = field(core::mem::offset_of!(POWERBROADCAST_SETTING, DataLength));
    let data = field(core::mem::offset_of!(POWERBROADCAST_SETTING, Data));
    // SAFETY: the caller guarantees a readable header, so the GUID's bytes
    // are readable; read unaligned.
    let guid = unsafe { guid.cast::<windows::core::GUID>().read_unaligned() };
    // SAFETY: as above, for the length field.
    let length = unsafe { length.cast::<u32>().read_unaligned() };
    let length = (length as usize).min(POWER_SETTING_DATA_MAX);
    // SAFETY: the caller guarantees `DataLength` readable bytes at `Data`;
    // `length` is no more than that, the bytes are `u8` (no alignment), and
    // the slice is dropped before this function returns.
    let data = unsafe { core::slice::from_raw_parts(data, length) };
    power_rules::notified_saver(guid.to_u128(), data)
}

/// Whether the area a `WM_SETTINGCHANGE` names is `intl`:
/// `rules::locale::is_intl_area` of the string behind `lparam`, read while the
/// window procedure runs, unit by unit up to its NUL and at most
/// [`SETTING_AREA_MAX`] units, without assuming alignment; `false` for a
/// NULL `lparam` and for a name not ended within the bound.
///
/// # Safety
///
/// `lparam` is NULL or points at a readable, NUL-terminated UTF-16 string,
/// as the `lParam` of a `WM_SETTINGCHANGE` does while it is being handled.
unsafe fn setting_is_intl(lparam: LPARAM) -> bool {
    let base = lparam.0 as *const u16;
    if base.is_null() {
        return false;
    }
    let mut area = [0u16; SETTING_AREA_MAX];
    for (i, slot) in area.iter_mut().enumerate() {
        let at = base.wrapping_add(i);
        // SAFETY: the caller guarantees a NUL-terminated string; no unit
        // before this one was a NUL, so unit `i` is part of the string (at
        // most its terminator) and readable. Read unaligned.
        let unit = unsafe { at.read_unaligned() };
        if unit == 0 {
            return locale_rules::is_intl_area(&area[..i]);
        }
        *slot = unit;
    }
    false
}

/// Post a thread message to the current thread's own queue. Used from the
/// hook callback and the window procedure, which both run on the events thread.
fn post_to_self(message: u32, wparam: WPARAM, lparam: LPARAM) {
    // SAFETY: GetCurrentThreadId has no preconditions.
    let thread_id = unsafe { GetCurrentThreadId() };
    // SAFETY: posting plain integers to our own queue; a full queue is
    // reported through the Result and the event is dropped.
    if let Err(e) = unsafe { PostThreadMessageW(thread_id, message, wparam, lparam) } {
        log::warn!("PostThreadMessageW({message:#x}) failed: {e}");
    }
}

/// `WINEVENTPROC`: filter, translate, post. Runs on the events thread.
unsafe extern "system" fn win_event_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _event_thread: u32,
    _event_time_ms: u32,
) {
    if hwnd.is_invalid() {
        return;
    }
    // SAFETY: GetAncestor accepts any handle value and returns NULL for
    // windows that are already gone.
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    let top_level = if root.is_invalid() {
        TopLevel::Unknown
    } else if root == hwnd {
        TopLevel::Yes
    } else {
        TopLevel::No
    };
    if rules::win_event_intent(event, id_object, id_child, top_level) == Intent::Ignore {
        return;
    }
    post_to_self(
        MSG_WINEVENT,
        WPARAM(event as usize),
        LPARAM(hwnd.0 as isize),
    );
}

/// `WINEVENTPROC` of the `EVENT_OBJECT_LOCATIONCHANGE` hook:
/// keep only moves and resizes of top-level windows as a whole
/// (`rules::events::location_change_tracked`), and post. Everything else
/// (caret and cursor moves, child controls, windows already gone) is
/// dropped here, before it costs a message; which windows matter is the
/// loop's business (`on_location_change`). Runs on the events thread.
unsafe extern "system" fn location_event_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _event_thread: u32,
    _event_time_ms: u32,
) {
    if event != EVENT_OBJECT_LOCATIONCHANGE || hwnd.is_invalid() {
        return;
    }
    // SAFETY: GetAncestor accepts any handle value and returns NULL for
    // windows that are already gone.
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    let top_level = if root.is_invalid() {
        TopLevel::Unknown
    } else if root == hwnd {
        TopLevel::Yes
    } else {
        TopLevel::No
    };
    if !rules::location_change_tracked(id_object, id_child, top_level) {
        return;
    }
    post_to_self(MSG_LOCATIONCHANGE, WPARAM(0), LPARAM(hwnd.0 as isize));
}

/// Window procedure of the hidden window: forward the interesting messages
/// to the loop as thread messages, default-handle everything else.
unsafe extern "system" fn event_wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_SETTINGCHANGE => {
            // lParam may point at a string that is only valid during this
            // call; the action code travels on, and whether the string named
            // the `intl` area, read here.
            // SAFETY: the lParam of WM_SETTINGCHANGE is NULL or a
            // NUL-terminated string valid while this runs: the system copies
            // it into this process for a sender elsewhere, and refuses to
            // post the message (it can only be sent).
            let intl = unsafe { setting_is_intl(lparam) };
            let area = if intl { SETTING_AREA_INTL } else { 0 };
            post_to_self(MSG_SETTINGCHANGE, wparam, LPARAM(area));
            LRESULT(0)
        }
        WM_DWMCOLORIZATIONCOLORCHANGED | WM_THEMECHANGED => {
            post_to_self(MSG_THEMECHANGE, WPARAM(0), LPARAM(0));
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            post_to_self(MSG_DISPLAYCHANGE, WPARAM(0), LPARAM(0));
            LRESULT(0)
        }
        // The power source, the battery's charge, battery saver or a
        // resume. The loop asks for the whole status again; lParam (a
        // POWERBROADCAST_SETTING for a setting change) is only valid during
        // this call, so the one value taken from it, battery saver's new
        // state, is copied out here and posted with the code. TRUE is the
        // documented answer to a processed broadcast.
        WM_POWERBROADCAST if power_rules::power_broadcast_cause(wparam.0 as u32).is_some() => {
            let saver = if wparam.0 as u32 == PBT_POWERSETTINGCHANGE {
                // SAFETY: for PBT_POWERSETTINGCHANGE, lParam is the
                // POWERBROADCAST_SETTING the system passes with this call
                // (with its DataLength bytes of data), valid while it runs.
                unsafe { power_setting_saver(lparam) }
            } else {
                None
            };
            post_to_self(
                MSG_POWERCHANGE,
                wparam,
                LPARAM(power_rules::saver_to_lparam(saver)),
            );
            LRESULT(1)
        }
        m if SHELL_HOOK_MESSAGE.get() == Some(&m) => {
            post_to_self(MSG_SHELLHOOK, wparam, lparam);
            LRESULT(0)
        }
        // SAFETY: forwarding the arguments we were given to the default procedure.
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `s` as UTF-16 with its NUL, one byte into a buffer, so the string is
    /// not aligned the way a `u16` is; the string starts at byte 1.
    fn unaligned_wide(s: &str) -> Vec<u8> {
        let mut buffer = vec![0u8];
        for unit in s.encode_utf16().chain(std::iter::once(0)) {
            buffer.extend_from_slice(&unit.to_ne_bytes());
        }
        buffer
    }

    fn area_is_intl(s: &str) -> bool {
        let buffer = unaligned_wide(s);
        // SAFETY: the pointer is a NUL-terminated UTF-16 string inside
        // `buffer`, alive for the call.
        unsafe { setting_is_intl(LPARAM(buffer[1..].as_ptr() as isize)) }
    }

    #[test]
    fn the_intl_area_is_read_out_of_a_setting_change() {
        assert!(area_is_intl("intl"));
        assert!(area_is_intl("Intl"));
        for other in ["", "ImmersiveColorSet", "Policy", "int", "intl2"] {
            assert!(!area_is_intl(other), "{other:?}");
        }
        // A name longer than the bound is not read to its end: no match,
        // whatever follows.
        assert!(!area_is_intl(&"x".repeat(SETTING_AREA_MAX + 8)));
        assert!(!area_is_intl(&"i".repeat(SETTING_AREA_MAX)));
        // An aligned string reads the same.
        let aligned: Vec<u16> = "intl".encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: as above, for an aligned buffer.
        assert!(unsafe { setting_is_intl(LPARAM(aligned.as_ptr() as isize)) });
        // SAFETY: NULL is allowed and reads nothing.
        assert!(!unsafe { setting_is_intl(LPARAM(0)) });
    }

    #[test]
    fn the_power_settings_are_the_three_the_contract_names() {
        let names: Vec<&str> = POWER_SETTINGS.iter().map(|(_, name)| *name).collect();
        assert_eq!(
            names,
            [
                "GUID_BATTERY_PERCENTAGE_REMAINING",
                "GUID_ACDC_POWER_SOURCE",
                "GUID_POWER_SAVING_STATUS",
            ]
        );
        assert_eq!(*POWER_SETTINGS[0].0, GUID_BATTERY_PERCENTAGE_REMAINING);
        assert_eq!(*POWER_SETTINGS[1].0, GUID_ACDC_POWER_SOURCE);
        assert_eq!(*POWER_SETTINGS[2].0, GUID_POWER_SAVING_STATUS);
    }

    /// A `POWERBROADCAST_SETTING` for `setting` with `length` in its
    /// `DataLength` and `data` at `Data`, written field by field through the
    /// structure's own offsets into a byte buffer, one byte in (so at no
    /// particular alignment), at least as large as the structure.
    fn power_setting(setting: windows::core::GUID, length: u32, data: &[u8]) -> Vec<u8> {
        let header = core::mem::size_of::<POWERBROADCAST_SETTING>();
        let at = core::mem::offset_of!(POWERBROADCAST_SETTING, Data);
        let mut buffer = vec![0u8; 1 + header.max(at + data.len())];
        let guid_at = 1 + core::mem::offset_of!(POWERBROADCAST_SETTING, PowerSetting);
        let guid = buffer[guid_at..guid_at + core::mem::size_of::<windows::core::GUID>()]
            .as_mut_ptr()
            .cast::<windows::core::GUID>();
        // SAFETY: `guid` points at a GUID's worth of bytes of `buffer`;
        // written unaligned.
        unsafe { guid.write_unaligned(setting) };
        let length_at = 1 + core::mem::offset_of!(POWERBROADCAST_SETTING, DataLength);
        buffer[length_at..length_at + 4].copy_from_slice(&length.to_ne_bytes());
        buffer[1 + at..1 + at + data.len()].copy_from_slice(data);
        buffer
    }

    fn saver_of(buffer: &[u8]) -> Option<bool> {
        // SAFETY: `buffer` holds a whole POWERBROADCAST_SETTING after its
        // first byte, with at least the data its length field names when
        // that is a DWORD's (`power_setting`), alive for the call; a longer
        // length is cut to POWER_SETTING_DATA_MAX, which the buffer holds.
        unsafe { power_setting_saver(LPARAM(buffer[1..].as_ptr() as isize)) }
    }

    #[test]
    fn the_window_procedure_reads_battery_saver_out_of_the_setting() {
        let on = power_setting(GUID_POWER_SAVING_STATUS, 4, &1u32.to_ne_bytes());
        assert_eq!(saver_of(&on), Some(true));
        let off = power_setting(GUID_POWER_SAVING_STATUS, 4, &0u32.to_ne_bytes());
        assert_eq!(saver_of(&off), Some(false));
        // The other registered settings carry no battery-saver state.
        let ac = power_setting(GUID_ACDC_POWER_SOURCE, 4, &1u32.to_ne_bytes());
        assert_eq!(saver_of(&ac), None);
        let percent = power_setting(GUID_BATTERY_PERCENTAGE_REMAINING, 4, &1u32.to_ne_bytes());
        assert_eq!(saver_of(&percent), None);
        // A length that is not a DWORD's says nothing; a huge one is cut to
        // POWER_SETTING_DATA_MAX before anything is read.
        let short = power_setting(GUID_POWER_SAVING_STATUS, 1, &[1]);
        assert_eq!(saver_of(&short), None);
        let huge = power_setting(
            GUID_POWER_SAVING_STATUS,
            u32::MAX,
            &[1; POWER_SETTING_DATA_MAX],
        );
        assert_eq!(saver_of(&huge), None);
        // NULL is none.
        // SAFETY: NULL is allowed and checked first.
        assert_eq!(unsafe { power_setting_saver(LPARAM(0)) }, None);
    }
}
