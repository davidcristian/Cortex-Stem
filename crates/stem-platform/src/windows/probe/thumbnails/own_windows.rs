//! The probe's own windows: their procedures and classes, creating and
//! showing them, the work area and the message pump.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DispatchMessageW, IDC_ARROW,
    LoadCursorW, MSG, PM_REMOVE, PeekMessageW, RegisterClassExW, SHOW_WINDOW_CMD, SPI_GETWORKAREA,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, ShowWindow, SystemParametersInfoW, TranslateMessage,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_ERASEBKGND, WM_PAINT, WM_TIMER, WNDCLASSEXW,
};

use super::WM_PROBE_WAKE;
use super::geometry::{height, width};
use super::painting::{paint_board, paint_source};
use super::quit::{Quit, quitting, request_quit};
use crate::windows::util;

pub(super) unsafe extern "system" fn destination_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_PAINT => {
            paint_board(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND | WM_PROBE_WAKE => LRESULT(1),
        WM_CLOSE => {
            // The probe destroys the window itself, after unregistering.
            request_quit(Quit::CloseButton);
            LRESULT(0)
        }
        _ => {
            // SAFETY: forwarding the arguments we were given to the default
            // procedure.
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
    }
}

pub(super) unsafe extern "system" fn source_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_PAINT => {
            paint_source(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_TIMER => {
            // SAFETY: invalidates our own window; no rectangle pointer.
            let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
            LRESULT(0)
        }
        // A source closes with the probe, never on its own.
        WM_CLOSE => LRESULT(0),
        _ => {
            // SAFETY: forwarding the arguments we were given to the default
            // procedure.
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
    }
}

pub(super) type WindowProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

pub(super) fn register_class(
    instance: HINSTANCE,
    name: &[u16],
    procedure: WindowProc,
) -> Result<(), String> {
    // SAFETY: IDC_ARROW is a predefined system cursor; no module is needed.
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default();
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(procedure),
        hInstance: instance,
        hCursor: cursor,
        lpszClassName: util::pcwstr(name),
        ..Default::default()
    };
    // SAFETY: `class` is fully initialised and the name buffer outlives the
    // call (the probe keeps it until UnregisterClassW).
    if unsafe { RegisterClassExW(&class) } == 0 {
        let err = windows::core::Error::from_thread();
        if err.code() != util::hresult_of(ERROR_CLASS_ALREADY_EXISTS) {
            return Err(format!("RegisterClassExW failed: {err}"));
        }
    }
    Ok(())
}

pub(super) fn create_window(
    instance: HINSTANCE,
    class: &[u16],
    title: &str,
    style: WINDOW_STYLE,
    rect: RECT,
) -> Result<HWND, String> {
    let wide_title = util::to_wide(title);
    // SAFETY: the class is registered by this process; both string buffers
    // outlive the call; no parent, menu or creation data. Extended style 0:
    // a normal window, not topmost, not a tool window.
    let created = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            util::pcwstr(class),
            util::pcwstr(&wide_title),
            style,
            rect.left,
            rect.top,
            width(rect),
            height(rect),
            None,
            None,
            Some(instance),
            None,
        )
    };
    created.map_err(|e| format!("CreateWindowExW('{title}') failed: {e}"))
}

pub(super) fn show(hwnd: HWND, command: SHOW_WINDOW_CMD) {
    // SAFETY: our own window; the return value is the previous visibility.
    let _ = unsafe { ShowWindow(hwnd, command) };
}

pub(super) fn work_area() -> RECT {
    let mut rect = RECT::default();
    // SAFETY: `rect` is a live RECT, as SPI_GETWORKAREA expects; nothing is
    // written to the profile (flags 0).
    let result = unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some((&raw mut rect).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    if result.is_err() || width(rect) <= 0 || height(rect) <= 0 {
        RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        }
    } else {
        rect
    }
}

/// Pumps this thread's messages for `duration` (so new windows paint and
/// DWM composes a frame before the next step).
pub(super) fn pump_for(duration: Duration) {
    let end = Instant::now() + duration;
    while Instant::now() < end && !quitting() {
        pump_pending();
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Dispatches every message waiting for this thread, including messages
/// other threads send to its windows, and returns.
pub(super) fn pump_pending() {
    loop {
        let mut message = MSG::default();
        // SAFETY: `message` is a live out-struct; a NULL window takes any
        // message of this thread.
        let more = unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) };
        if !more.as_bool() {
            break;
        }
        // SAFETY: `message` was filled by PeekMessageW.
        let _ = unsafe { TranslateMessage(&message) };
        // SAFETY: as above; dispatches to this thread's procedures.
        unsafe { DispatchMessageW(&message) };
    }
}
