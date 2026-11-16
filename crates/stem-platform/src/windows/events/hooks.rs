//! The WinEvent hooks and their callbacks, which filter, translate and post
//! to the loop.

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Accessibility::{
    HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent, WINEVENTPROC,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CHILDID_SELF, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_CREATE, EVENT_OBJECT_DESTROY,
    EVENT_OBJECT_HIDE, EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_SHOW,
    EVENT_OBJECT_UNCLOAKED, EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND,
    EVENT_SYSTEM_MINIMIZESTART, GA_ROOT, GetAncestor, HSHELL_ACCESSIBILITYSTATE,
    HSHELL_ACTIVATESHELLWINDOW, HSHELL_APPCOMMAND, HSHELL_ENDTASK, HSHELL_GETMINRECT,
    HSHELL_HIGHBIT, HSHELL_LANGUAGE, HSHELL_MONITORCHANGED, HSHELL_REDRAW, HSHELL_SYSMENU,
    HSHELL_TASKMAN, HSHELL_WINDOWACTIVATED, HSHELL_WINDOWCREATED, HSHELL_WINDOWDESTROYED,
    HSHELL_WINDOWREPLACED, HSHELL_WINDOWREPLACING, OBJID_WINDOW, WINEVENT_OUTOFCONTEXT,
    WINEVENT_SKIPOWNPROCESS,
};

use super::dispatch::{MSG_LOCATIONCHANGE, MSG_WINEVENT, post_to_self};
use crate::rules::events::{self as rules, Intent, TopLevel};
use crate::{PlatformError, PlatformResult};

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

/// `SetWinEventHook` guard; unhooked on drop (on the installing thread).
pub(super) struct WinEventHook(HWINEVENTHOOK);

impl WinEventHook {
    /// A hook for the translation table's events, delivered to [`win_event_proc`].
    pub(super) fn install(event_min: u32, event_max: u32) -> PlatformResult<Self> {
        Self::install_with(event_min, event_max, Some(win_event_proc))
    }

    pub(super) fn install_with(
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
pub(super) unsafe extern "system" fn location_event_proc(
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
