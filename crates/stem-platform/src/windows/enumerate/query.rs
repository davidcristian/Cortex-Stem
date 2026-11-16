//! The context of one query (our own pid, a COM apartment, where the
//! desktop answer comes from) and the full facts of an eligible window.

use stem_core::RawWindow;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::GetCurrentProcessId;

use super::facts::{probe, window_frame};
use super::process::{open_process, process_app_user_model_id, process_image_path};
use super::read_app_user_model_id;
use crate::rules::identity::hwnd_icon_key;
use crate::rules::recheck::{self, Check};
use crate::windows::{monitors, util, virtual_desktop};

/// Where a query takes `RawWindow::on_current_desktop` from.
pub(super) enum DesktopSource {
    /// The shell, through the thread's manager.
    Ask(virtual_desktop::DesktopQuery),
    /// An answer the caller already has (the move path).
    Known(Option<bool>),
}

/// Per-call context: our own pid, a COM apartment for the property-store
/// read, and the thread's virtual desktop manager, initialised once
/// per enumeration instead of once per window. Field order is drop order:
/// the manager reference goes before the apartment.
pub(super) struct QueryContext {
    own_pid: u32,
    desktop: DesktopSource,
    _com: util::ComApartment,
}

impl QueryContext {
    /// For one window: the thread's manager as it is.
    pub(super) fn new() -> Self {
        Self::with(|| DesktopSource::Ask(virtual_desktop::DesktopQuery::for_this_thread()))
    }

    /// For a full enumeration: a fresh manager on a thread that has one.
    pub(super) fn renewed() -> Self {
        Self::with(|| DesktopSource::Ask(virtual_desktop::DesktopQuery::renewed()))
    }

    pub(super) fn with(desktop: impl FnOnce() -> DesktopSource) -> Self {
        // The apartment first: the manager is created inside it.
        let com = util::ComApartment::sta();
        Self {
            // SAFETY: GetCurrentProcessId has no preconditions.
            own_pid: unsafe { GetCurrentProcessId() },
            desktop: desktop(),
            _com: com,
        }
    }

    /// The desktop answer for `hwnd` (see [`DesktopSource`]).
    fn on_current_desktop(&self, hwnd: HWND) -> Option<bool> {
        match &self.desktop {
            DesktopSource::Ask(query) => query.is_window_on_current_desktop(hwnd),
            DesktopSource::Known(answer) => *answer,
        }
    }

    /// How this context gets its desktop facts, for the enumeration's log line.
    pub(super) fn desktop_facts(&self) -> &'static str {
        match &self.desktop {
            DesktopSource::Ask(query) if !query.is_available() => "unavailable on this thread",
            DesktopSource::Ask(_) if virtual_desktop::is_paused() => "paused after a slow call",
            DesktopSource::Ask(_) => "asked",
            DesktopSource::Known(_) => "reused",
        }
    }

    /// Full facts for an eligible window, `None` otherwise.
    pub(super) fn query(&self, hwnd: HWND) -> Option<RawWindow> {
        self.query_checked(hwnd).ok()
    }

    /// Full facts for an eligible window, else how it stands
    /// (`rules::recheck::classify`; `Gone` for a dead handle).
    pub(super) fn query_checked(&self, hwnd: HWND) -> Result<RawWindow, Check> {
        let probe = probe(hwnd, self.own_pid).ok_or(Check::Gone)?;
        let check = recheck::classify(&probe.facts);
        if check != Check::Eligible {
            log::trace!(
                "window {:?} ({:?}) rejected: {check:?}",
                hwnd.0,
                probe.facts.class_name
            );
            return Err(check);
        }
        let id = util::window_id(hwnd);
        let is_cloaked = probe.facts.is_cloaked();
        let process_id = probe.facts.process_id;
        // One handle for everything the process can tell us.
        let process = open_process(process_id);
        let exe_path = process
            .as_ref()
            .and_then(|h| process_image_path(h, process_id));
        let app_user_model_id = read_app_user_model_id(hwnd).or_else(|| {
            process
                .as_ref()
                .and_then(|h| process_app_user_model_id(h, process_id))
        });
        Ok(RawWindow {
            id,
            title: probe.facts.title,
            class_name: probe.facts.class_name,
            process_id,
            exe_path,
            app_user_model_id,
            is_visible: probe.facts.is_visible,
            is_minimized: probe.is_minimized,
            is_cloaked,
            on_current_desktop: self.on_current_desktop(hwnd),
            monitor: monitors::monitor_for_window(hwnd),
            rect: window_frame(hwnd),
            icon_key: Some(hwnd_icon_key(id.0, probe.facts.process_id)),
        })
    }
}
