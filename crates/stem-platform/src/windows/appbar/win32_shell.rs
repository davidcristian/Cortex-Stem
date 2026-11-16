//! The real shell behind `rules::appbar::AppBarShell`: one
//! `SHAppBarMessage` per call.

use stem_core::Rect;
use stem_core::settings::DockPosition;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::UI::Shell::{
    ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_ACTIVATE, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE,
    ABM_SETPOS, ABM_WINDOWPOSCHANGED, APPBARDATA, SHAppBarMessage,
};
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
use windows::core::{PCWSTR, w};

use super::CALLBACK_MESSAGE;
use crate::rules::appbar::AppBarShell;

/// The real shell: one `SHAppBarMessage` per call. The AppBar thread only.
#[derive(Debug, Clone, Copy)]
pub(super) struct Win32Shell;

impl AppBarShell for Win32Shell {
    /// Whether the tray window exists. Reads the window list; sends nothing.
    fn present(&mut self) -> bool {
        // SAFETY: both arguments are a valid NUL-terminated string or null.
        unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }.is_ok()
    }

    fn register(&mut self, handle: isize) -> bool {
        send(ABM_NEW, &mut app_bar_data(handle)) != 0
    }

    /// `ABM_REMOVE` always "succeeds".
    fn remove(&mut self, handle: isize) {
        send(ABM_REMOVE, &mut app_bar_data(handle));
    }

    fn query(&mut self, handle: isize, edge: DockPosition, rect: Rect) -> Rect {
        let mut data = app_bar_data(handle);
        data.uEdge = abe(edge);
        data.rc = to_rect(rect);
        send(ABM_QUERYPOS, &mut data);
        from_rect(data.rc)
    }

    fn set_pos(&mut self, handle: isize, edge: DockPosition, rect: Rect) -> Rect {
        let mut data = app_bar_data(handle);
        data.uEdge = abe(edge);
        data.rc = to_rect(rect);
        send(ABM_SETPOS, &mut data);
        from_rect(data.rc)
    }

    fn activate(&mut self, handle: isize, active: bool) {
        let mut data = app_bar_data(handle);
        data.lParam = LPARAM(isize::from(active));
        send(ABM_ACTIVATE, &mut data);
    }

    fn window_pos_changed(&mut self, handle: isize) {
        send(ABM_WINDOWPOSCHANGED, &mut app_bar_data(handle));
    }
}

/// One `SHAppBarMessage`. The AppBar thread only.
fn send(message: u32, data: &mut APPBARDATA) -> usize {
    // SAFETY: `data` is a complete `APPBARDATA` with `cbSize` set, alive and
    // exclusively borrowed for the call; the shell copies what it reads,
    // writes `rc` back for ABM_QUERYPOS / ABM_SETPOS, and keeps no pointer.
    unsafe { SHAppBarMessage(message, data) }
}

/// An `APPBARDATA` for the bar of the window `handle`.
fn app_bar_data(handle: isize) -> APPBARDATA {
    APPBARDATA {
        cbSize: size_of::<APPBARDATA>() as u32,
        hWnd: HWND(handle as *mut core::ffi::c_void),
        uCallbackMessage: CALLBACK_MESSAGE,
        ..Default::default()
    }
}

fn abe(edge: DockPosition) -> u32 {
    match edge {
        DockPosition::Bottom => ABE_BOTTOM,
        DockPosition::Top => ABE_TOP,
        DockPosition::Left => ABE_LEFT,
        DockPosition::Right => ABE_RIGHT,
    }
}

fn to_rect(rect: Rect) -> RECT {
    RECT {
        left: rect.x,
        top: rect.y,
        right: rect.right(),
        bottom: rect.bottom(),
    }
}

fn from_rect(rect: RECT) -> Rect {
    Rect::new(
        rect.left,
        rect.top,
        rect.right.saturating_sub(rect.left),
        rect.bottom.saturating_sub(rect.top),
    )
}
