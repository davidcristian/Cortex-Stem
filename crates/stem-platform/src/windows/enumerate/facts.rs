//! The cheap facts about a window (styles, owner, cloak flags, class,
//! title, frame), read without a process handle or COM.

use stem_core::Rect;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{
    DWM_CLOAKED_APP, DWM_CLOAKED_INHERITED, DWM_CLOAKED_SHELL, DWMWA_CLOAKED,
    DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GA_ROOT, GW_OWNER, GWL_EXSTYLE, GetAncestor, GetClassNameW, GetWindow, GetWindowLongPtrW,
    GetWindowRect, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindow, IsWindowVisible, WINDOW_EX_STYLE, WS_EX_APPWINDOW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW,
};

use crate::rules::eligibility::{self, Owner, WindowFacts};

// The pure rule table mirrors dwmapi.h; prove it against the SDK.
const _: () = {
    assert!(eligibility::CLOAKED_APP == DWM_CLOAKED_APP);
    assert!(eligibility::CLOAKED_SHELL == DWM_CLOAKED_SHELL);
    assert!(eligibility::CLOAKED_INHERITED == DWM_CLOAKED_INHERITED);
};

/// Longest window title we copy; longer captions are truncated.
const MAX_TITLE_CHARS: usize = 1024;
/// `GetClassNameW` caps class names at 256 characters.
const MAX_CLASS_CHARS: usize = 256;

/// The window's frame in screen coordinates (physical pixels): the DWM
/// extended frame bounds, else `GetWindowRect` when DWM refuses (desktop
/// composition off, some cloaked windows). `None` when both fail, which
/// means a dead handle. Reads cached state only; nothing is sent to the
/// window's thread.
pub(in crate::windows) fn window_frame(hwnd: HWND) -> Option<Rect> {
    let mut rect = RECT::default();
    let out = std::ptr::from_mut(&mut rect).cast::<core::ffi::c_void>();
    // SAFETY: `out` addresses a RECT and the size passed is exactly that of a RECT.
    let dwm = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            out,
            size_of::<RECT>() as u32,
        )
    };
    if dwm.is_err() {
        // SAFETY: `rect` is a live out-pointer for the duration of the call.
        if let Err(e) = unsafe { GetWindowRect(hwnd, &mut rect) } {
            log::trace!("no frame for window {:#x}: {e}", hwnd.0 as isize);
            return None;
        }
    }
    Some(Rect::new(
        rect.left,
        rect.top,
        rect.right - rect.left,
        rect.bottom - rect.top,
    ))
}

/// The cheap facts (no process handle, no COM).
pub(super) struct Probe {
    pub(super) facts: WindowFacts,
    pub(super) is_minimized: bool,
}

/// Gather [`WindowFacts`] for a live window; `None` when the handle is dead.
pub(super) fn probe(hwnd: HWND, own_pid: u32) -> Option<Probe> {
    // SAFETY: IsWindow accepts any handle value.
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return None;
    }
    let mut process_id = 0u32;
    // SAFETY: `process_id` is a live out-pointer for the duration of the call.
    let thread_id = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    if thread_id == 0 {
        // The window vanished between IsWindow and here.
        return None;
    }
    let mut facts = WindowFacts {
        process_id,
        own_process_id: own_pid,
        ..WindowFacts::default()
    };
    if process_id == own_pid {
        // Our own windows never qualify; stop before anything that would
        // send WM_GETTEXT to ourselves.
        return Some(Probe {
            facts,
            is_minimized: false,
        });
    }

    // SAFETY: GetAncestor accepts any handle value and returns NULL for dead ones.
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    facts.is_root = root == hwnd;
    // SAFETY: IsWindowVisible accepts any handle value.
    facts.is_visible = unsafe { IsWindowVisible(hwnd) }.as_bool();
    facts.class_name = class_name(hwnd);
    // SAFETY: GetWindowLongPtrW reads a cached window property; 0 is both a
    // valid style and the failure value, and a zero style is fine either way.
    let ex_style = WINDOW_EX_STYLE(unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32);
    facts.is_tool_window = ex_style.contains(WS_EX_TOOLWINDOW);
    facts.is_app_window = ex_style.contains(WS_EX_APPWINDOW);
    facts.is_no_activate = ex_style.contains(WS_EX_NOACTIVATE);
    facts.owner = owner_of(hwnd);
    facts.cloak_flags = cloak_flags(hwnd);
    facts.title = window_title(hwnd);
    // SAFETY: IsIconic accepts any handle value.
    let is_minimized = unsafe { IsIconic(hwnd) }.as_bool();
    Some(Probe {
        facts,
        is_minimized,
    })
}

fn owner_of(hwnd: HWND) -> Owner {
    // SAFETY: GetWindow reads the owner relation; it fails (Err) when there is none.
    let owner = unsafe { GetWindow(hwnd, GW_OWNER) }
        .ok()
        .filter(|h| !h.is_invalid());
    match owner {
        None => Owner::None,
        // SAFETY: IsWindowVisible accepts any handle value.
        Some(owner) if unsafe { IsWindowVisible(owner) }.as_bool() => Owner::Visible,
        Some(_) => Owner::Hidden,
    }
}

/// `DWMWA_CLOAKED` flags; `0` when DWM refuses (e.g. not composited).
fn cloak_flags(hwnd: HWND) -> u32 {
    let mut flags = 0u32;
    let out = std::ptr::from_mut(&mut flags).cast::<core::ffi::c_void>();
    // SAFETY: `out` addresses a u32 and the size passed is exactly that of a u32.
    match unsafe { DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, out, size_of::<u32>() as u32) } {
        Ok(()) => flags,
        Err(_) => 0,
    }
}

/// `GetClassNameW`; empty for a dead handle. Shared with `fullscreen`.
pub(in crate::windows) fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; MAX_CLASS_CHARS];
    // SAFETY: `buf` is a writable slice; the API writes at most its length
    // (including the NUL) and returns the number of characters copied.
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..(len as usize).min(buf.len())])
}

/// The window caption. For windows of other processes this reads the cached
/// text and cannot block on a hung application (see the `enumerate` notes); do
/// not call it for windows of our own process. Shared with `fullscreen`.
pub(in crate::windows) fn window_title(hwnd: HWND) -> String {
    // SAFETY: GetWindowTextLengthW accepts any handle value.
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    if len <= 0 {
        return String::new();
    }
    let capacity = (len as usize + 1).min(MAX_TITLE_CHARS);
    let mut buf = vec![0u16; capacity];
    // SAFETY: `buf` is a writable slice; the API writes at most its length
    // (including the NUL) and returns the number of characters copied.
    let copied = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if copied <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..(copied as usize).min(buf.len())])
}
