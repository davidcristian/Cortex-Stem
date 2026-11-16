//! The thread messages the callbacks post, the message loop, and the
//! handler each message goes to.

use stem_core::PlatformEvent;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_DESTROY, EVENT_OBJECT_UNCLOAKED,
    GetMessageW, MSG, PostThreadMessageW, SPI_SETWORKAREA, TranslateMessage, WM_APP, WM_TIMER,
};

use super::state::ThreadState;
use crate::rules::events::{self as rules, Intent};
use crate::rules::power as power_rules;
use crate::windows::util;

/// Thread messages (`MSG::hwnd == NULL`) posted by the hook callback and the
/// window procedure to the loop running on the same thread.
/// `wParam` = event id, `lParam` = `HWND`.
pub(super) const MSG_WINEVENT: u32 = WM_APP + 1;
/// `wParam` = `HSHELL_*` code, `lParam` = `HWND`.
pub(super) const MSG_SHELLHOOK: u32 = WM_APP + 2;
/// `wParam` = the `WM_SETTINGCHANGE` action (`SPI_*`, or 0); `lParam` =
/// [`SETTING_AREA_INTL`] when the message named the `intl` area,
/// else 0.
pub(super) const MSG_SETTINGCHANGE: u32 = WM_APP + 3;
pub(super) const MSG_THEMECHANGE: u32 = WM_APP + 4;
pub(super) const MSG_DISPLAYCHANGE: u32 = WM_APP + 5;
/// A top-level window moved or resized; `lParam` = its `HWND`.
pub(super) const MSG_LOCATIONCHANGE: u32 = WM_APP + 6;
/// A `WM_POWERBROADCAST` that may change the power reading;
/// `wParam` = the `PBT_*` code, `lParam` = the battery-saver state a
/// `GUID_POWER_SAVING_STATUS` notification carried
/// (`rules::power::saver_to_lparam`).
pub(super) const MSG_POWERCHANGE: u32 = WM_APP + 7;

/// `MSG_SETTINGCHANGE`'s `lParam` for a `WM_SETTINGCHANGE` that named the
/// `intl` area.
pub(super) const SETTING_AREA_INTL: isize = 1;

impl ThreadState {
    pub(super) fn run(&mut self) {
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
}

/// Post a thread message to the current thread's own queue. Used from the
/// hook callback and the window procedure, which both run on the events thread.
pub(super) fn post_to_self(message: u32, wparam: WPARAM, lparam: LPARAM) {
    // SAFETY: GetCurrentThreadId has no preconditions.
    let thread_id = unsafe { GetCurrentThreadId() };
    // SAFETY: posting plain integers to our own queue; a full queue is
    // reported through the Result and the event is dropped.
    if let Err(e) = unsafe { PostThreadMessageW(thread_id, message, wparam, lparam) } {
        log::warn!("PostThreadMessageW({message:#x}) failed: {e}");
    }
}
