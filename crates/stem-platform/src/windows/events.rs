//! The event thread.
//!
//! Contract:
//! - [`EventHookThread::start`] spawns a thread that: creates a message-only
//!   window, installs `SetWinEventHook` out-of-context hooks for
//!   `EVENT_OBJECT_CREATE/DESTROY/SHOW/HIDE/CLOAKED/UNCLOAKED/NAMECHANGE`,
//!   `EVENT_SYSTEM_FOREGROUND`, `EVENT_SYSTEM_MINIMIZESTART/END`, registers a
//!   shell hook window (`RegisterShellHookWindow`) for `HSHELL_FLASH`, and then
//!   runs a `GetMessageW` loop.
//! - Hook callbacks only translate the event and push a [`stem_core::PlatformEvent`]
//!   into `sink`; all filtering goes through `enumerate::query_window`.
//! - The thread emits an initial `WindowsSnapshot`, `MonitorsChanged`,
//!   `ThemeChanged`, `RecycleBinChanged`, `ForegroundChanged` after setup.
//! - `WM_DISPLAYCHANGE`/`WM_DPICHANGED` -> `MonitorsChanged` + `WindowsSnapshot`.
//! - `WM_SETTINGCHANGE` ("ImmersiveColorSet") and `WM_DWMCOLORIZATIONCOLORCHANGED`
//!   -> `ThemeChanged` (via `theme::current_theme`).
//! - A timer polls `recycle_bin::query` every few seconds and emits
//!   `RecycleBinChanged` on change.
//! - [`EventHookThread::stop`] posts `WM_QUIT`, unhooks, and joins (bounded).
//!
//! # Design
//!
//! One OS thread (`stem-platform-events`) owns everything: a hidden window,
//! the WinEvent hooks, the timers and the message loop. All of it is released
//! on that thread, in reverse order, when the loop exits (RAII guards:
//! [`WinEventHook`], [`EventWindow`], [`ThreadTimer`]).
//!
//! **Hidden top-level window instead of a message-only window.**
//! `WM_SETTINGCHANGE`, `WM_DISPLAYCHANGE`, `WM_THEMECHANGED` and
//! `WM_DWMCOLORIZATIONCOLORCHANGED` are broadcasts to *top-level* windows, and
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
//! only emitted for windows this thread reported earlier (`known`), which
//! keeps child-control `DESTROY` storms off the engine's channel. A window
//! that stops qualifying (hidden, cloaked by its app, title emptied) is
//! reported as removed; one that starts qualifying on any update is reported
//! as added, so a missed `SHOW` is healed by the next event about it.
//!
//! **Thread safety.** [`EventHookThread`] stores only the thread id and the
//! `JoinHandle`; raw handles (`HWND`, `HWINEVENTHOOK`, timer ids) are `!Send`
//! and never leave the thread.

use std::collections::HashSet;
use std::sync::OnceLock;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded};
use stem_core::{PlatformEvent, RecycleBinState, ThemeInfo, WindowId};
use windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CHILDID_SELF, CreateWindowExW, DefWindowProcW, DeregisterShellHookWindow, DestroyWindow,
    DispatchMessageW, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_CREATE, EVENT_OBJECT_DESTROY,
    EVENT_OBJECT_HIDE, EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_SHOW, EVENT_OBJECT_UNCLOAKED,
    EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZESTART, GA_ROOT,
    GetAncestor, GetMessageW, HSHELL_HIGHBIT, HSHELL_REDRAW, HSHELL_WINDOWACTIVATED, KillTimer,
    MSG, OBJID_WINDOW, PostThreadMessageW, RegisterClassExW, RegisterShellHookWindow,
    RegisterWindowMessageW, SPI_SETWORKAREA, SetTimer, TranslateMessage, UnregisterClassW,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_APP, WM_DISPLAYCHANGE,
    WM_DWMCOLORIZATIONCOLORCHANGED, WM_QUIT, WM_SETTINGCHANGE, WM_THEMECHANGED, WM_TIMER,
    WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_OVERLAPPED,
};

use super::{actions, enumerate, monitors, recycle_bin, theme, util};
use crate::rules::events::{self as rules, Intent, TitleDebouncer, TopLevel};
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
    assert!(rules::OBJID_WINDOW == OBJID_WINDOW.0);
    assert!(rules::CHILDID_SELF == CHILDID_SELF as i32);
    assert!(rules::HSHELL_WINDOWACTIVATED == HSHELL_WINDOWACTIVATED);
    assert!(rules::HSHELL_REDRAW == HSHELL_REDRAW);
    assert!(rules::HSHELL_HIGHBIT == HSHELL_HIGHBIT);
};

/// Thread messages (`MSG::hwnd == NULL`) posted by the hook callback and the
/// window procedure to the loop running on the same thread.
/// `wParam` = event id, `lParam` = `HWND`.
const MSG_WINEVENT: u32 = WM_APP + 1;
/// `wParam` = `HSHELL_*` code, `lParam` = `HWND`.
const MSG_SHELLHOOK: u32 = WM_APP + 2;
/// `wParam` = the `WM_SETTINGCHANGE` action (`SPI_*`, or 0).
const MSG_SETTINGCHANGE: u32 = WM_APP + 3;
const MSG_THEMECHANGE: u32 = WM_APP + 4;
const MSG_DISPLAYCHANGE: u32 = WM_APP + 5;

const CLASS_NAME: &str = "DockPlatformEvents";
const WINDOW_TITLE: &str = "Stem platform events";
const SHELL_HOOK_MESSAGE_NAME: &str = "SHELLHOOK";

const RECYCLE_BIN_POLL: Duration = Duration::from_secs(5);
const START_TIMEOUT: Duration = Duration::from_secs(5);
const STOP_TIMEOUT: Duration = Duration::from_secs(2);

/// `RegisterWindowMessageW("SHELLHOOK")`: a per-process constant.
static SHELL_HOOK_MESSAGE: OnceLock<u32> = OnceLock::new();

/// Handle to the running event thread. Dropping it without `stop` asks the
/// thread to quit but does not wait for it; prefer `stop`.
#[derive(Debug)]
pub struct EventHookThread {
    thread_id: u32,
    join: Option<JoinHandle<()>>,
    done: Receiver<()>,
}

impl EventHookThread {
    pub fn start(sink: EventSink) -> PlatformResult<Self> {
        monitors::ensure_dpi_awareness();
        let (ready_tx, ready_rx) = bounded::<PlatformResult<u32>>(1);
        let (done_tx, done_rx) = bounded::<()>(1);
        let join = std::thread::Builder::new()
            .name("stem-platform-events".into())
            .spawn(move || thread_main(sink, &ready_tx, &done_tx))
            .map_err(|e| PlatformError::Other(format!("spawn events thread: {e}")))?;
        match ready_rx.recv_timeout(START_TIMEOUT) {
            Ok(Ok(thread_id)) => {
                log::info!("events thread started (id {thread_id})");
                Ok(Self {
                    thread_id,
                    join: Some(join),
                    done: done_rx,
                })
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
        if self.join.is_some() {
            self.post_quit();
        }
    }
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
    fn install(event_min: u32, event_max: u32) -> PlatformResult<Self> {
        // SAFETY: the callback is a static `extern "system"` function with the
        // WINEVENTPROC signature; out-of-context hooks need no module handle.
        let hook = unsafe {
            SetWinEventHook(
                event_min,
                event_max,
                None,
                Some(win_event_proc),
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

/// Everything the loop owns. Field order is drop order: timers, then hooks,
/// then the window, then the COM apartment last.
struct ThreadState {
    sink: EventSink,
    thread_id: u32,
    started: Instant,
    /// Windows reported to the engine (snapshot or `WindowAdded`) and not
    /// yet removed.
    known: HashSet<WindowId>,
    titles: TitleDebouncer,
    title_timer: Option<ThreadTimer>,
    recycle_timer: Option<ThreadTimer>,
    last_recycle_bin: Option<RecycleBinState>,
    recycle_bin_error_logged: bool,
    last_theme: Option<ThemeInfo>,
    _hooks: Vec<WinEventHook>,
    _window: EventWindow,
    _com: util::ComApartment,
}

impl ThreadState {
    fn setup(sink: EventSink) -> PlatformResult<Self> {
        let com = util::ComApartment::sta();
        let window = EventWindow::create()?;
        let mut hooks = Vec::with_capacity(rules::HOOK_RANGES.len());
        for &(min, max) in rules::HOOK_RANGES {
            hooks.push(WinEventHook::install(min, max)?);
        }
        // SAFETY: GetCurrentThreadId has no preconditions.
        let thread_id = unsafe { GetCurrentThreadId() };
        let recycle_timer = ThreadTimer::start(RECYCLE_BIN_POLL);
        Ok(Self {
            sink,
            thread_id,
            started: Instant::now(),
            known: HashSet::new(),
            titles: TitleDebouncer::default(),
            title_timer: None,
            recycle_timer,
            last_recycle_bin: None,
            recycle_bin_error_logged: false,
            last_theme: None,
            _hooks: hooks,
            _window: window,
            _com: com,
        })
    }

    fn emit(&self, event: PlatformEvent) {
        (self.sink)(event);
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// Same order as the mock platform: monitors, theme, recycle bin,
    /// windows, foreground.
    fn emit_initial(&mut self) {
        self.emit_monitors();
        self.refresh_theme(true);
        self.poll_recycle_bin(true);
        self.emit_snapshot();
        self.emit(PlatformEvent::ForegroundChanged(
            actions::foreground_window(),
        ));
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
            }
            MSG_THEMECHANGE => self.refresh_theme(false),
            MSG_DISPLAYCHANGE => {
                self.emit_monitors();
                self.emit_snapshot();
            }
            WM_TIMER => self.on_timer(msg.wParam.0),
            _ => {}
        }
    }

    fn on_win_event(&mut self, event: u32, hwnd: HWND) {
        match rules::event_intent(event) {
            Intent::Added | Intent::Updated => self.refresh_window(hwnd),
            Intent::Removed => self.remove_window(util::window_id(hwnd)),
            Intent::TitleChanged => self.on_title_changed(util::window_id(hwnd)),
            Intent::Foreground => self.emit_foreground(hwnd),
            Intent::Attention | Intent::Ignore => {}
        }
    }

    fn on_shell_hook(&mut self, code: u32, hwnd: HWND) {
        match rules::shell_hook_intent(code) {
            Intent::Foreground => self.emit_foreground(hwnd),
            Intent::Attention => {
                let id = util::window_id(hwnd);
                if !self.known.contains(&id) {
                    self.refresh_window(hwnd);
                }
                if self.known.contains(&id) {
                    self.emit(PlatformEvent::WindowAttention { id, flashing: true });
                }
            }
            _ => {}
        }
    }

    fn on_timer(&mut self, timer_id: usize) {
        if self
            .title_timer
            .as_ref()
            .is_some_and(|t| t.id() == timer_id)
        {
            self.flush_titles();
        } else if self
            .recycle_timer
            .as_ref()
            .is_some_and(|t| t.id() == timer_id)
        {
            self.poll_recycle_bin(false);
        }
    }

    /// Re-query a window and report it as added, updated or removed.
    fn refresh_window(&mut self, hwnd: HWND) {
        let id = util::window_id(hwnd);
        match enumerate::query_window(hwnd) {
            Ok(Some(raw)) => {
                if self.known.insert(id) {
                    self.emit(PlatformEvent::WindowAdded(raw));
                } else {
                    self.emit(PlatformEvent::WindowUpdated(raw));
                }
            }
            Ok(None) => self.remove_window(id),
            Err(e) => log::debug!("query_window({:?}) failed: {e}", hwnd.0),
        }
    }

    fn remove_window(&mut self, id: WindowId) {
        self.titles.forget(id.0);
        if self.known.remove(&id) {
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
            self.refresh_window(util::hwnd(WindowId(id)));
        }
        // Re-arm for the next pending flush, or stop the timer.
        self.title_timer = self.titles.next_due().and_then(|due| {
            ThreadTimer::start(Duration::from_millis(due.saturating_sub(now).max(1)))
        });
    }

    fn emit_foreground(&mut self, hwnd: HWND) {
        let id = (!hwnd.is_invalid() && enumerate::is_taskbar_candidate(hwnd))
            .then(|| util::window_id(hwnd));
        if let Some(id) = id
            && !self.known.contains(&id)
        {
            // A candidate we never saw shown (missed event): report it first
            // so the engine can mark it active.
            self.refresh_window(hwnd);
        }
        self.emit(PlatformEvent::ForegroundChanged(id));
    }

    fn emit_snapshot(&mut self) {
        match enumerate::enumerate_taskbar_windows() {
            Ok(windows) => {
                self.known = windows.iter().map(|w| w.id).collect();
                self.titles = TitleDebouncer::default();
                self.title_timer = None;
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

    /// Emit `RecycleBinChanged` when the state differs from the last one
    /// emitted (or always, when `force`). Errors are skipped and logged once.
    fn poll_recycle_bin(&mut self, force: bool) {
        match recycle_bin::query() {
            Ok(state) => {
                self.recycle_bin_error_logged = false;
                if force || self.last_recycle_bin != Some(state) {
                    self.last_recycle_bin = Some(state);
                    self.emit(PlatformEvent::RecycleBinChanged(state));
                }
            }
            Err(e) => {
                if !self.recycle_bin_error_logged {
                    log::debug!("recycle bin unavailable: {e}");
                    self.recycle_bin_error_logged = true;
                }
            }
        }
    }
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
            // call; only the action code travels on.
            post_to_self(MSG_SETTINGCHANGE, wparam, LPARAM(0));
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
        m if SHELL_HOOK_MESSAGE.get() == Some(&m) => {
            post_to_self(MSG_SHELLHOOK, wparam, lparam);
            LRESULT(0)
        }
        // SAFETY: forwarding the arguments we were given to the default procedure.
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}
