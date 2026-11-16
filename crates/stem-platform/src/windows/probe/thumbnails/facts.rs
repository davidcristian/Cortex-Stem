//! Facts about a window (its rectangles, cloak state and display affinity)
//! and the Windows build.

use windows::Win32::Foundation::{ERROR_SUCCESS, HWND, RECT};
use windows::Win32::Graphics::Dwm::{
    DWM_CLOAKED_APP, DWM_CLOAKED_INHERITED, DWM_CLOAKED_SHELL, DWMWA_CLOAKED,
    DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::System::Registry::{
    HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowDisplayAffinity, GetWindowRect, IsIconic};
use windows::core::w;

use crate::windows::util;

pub(super) fn window_rect(hwnd: HWND) -> Option<RECT> {
    let mut rect = RECT::default();
    // SAFETY: `rect` is a live out-pointer; a dead handle fails the call.
    unsafe { GetWindowRect(hwnd, &mut rect) }.ok()?;
    Some(rect)
}

pub(super) fn frame_bounds(hwnd: HWND) -> Option<RECT> {
    let mut rect = RECT::default();
    // SAFETY: the buffer is a live RECT and its size is passed.
    let result = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut rect).cast(),
            size_of::<RECT>() as u32,
        )
    };
    result.ok()?;
    Some(rect)
}

fn cloak_value(hwnd: HWND) -> Option<u32> {
    let mut value = 0u32;
    // SAFETY: the buffer is a live u32 and its size is passed.
    let result = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            (&raw mut value).cast(),
            size_of::<u32>() as u32,
        )
    };
    result.ok()?;
    Some(value)
}

pub(super) fn is_iconic(hwnd: HWND) -> bool {
    // SAFETY: IsIconic accepts any handle value.
    unsafe { IsIconic(hwnd) }.as_bool()
}

fn affinity(hwnd: HWND) -> Option<u32> {
    let mut value = 0u32;
    // SAFETY: `value` is a live out-pointer; a dead handle fails the call.
    unsafe { GetWindowDisplayAffinity(hwnd, &mut value) }.ok()?;
    Some(value)
}

pub(super) fn facts(hwnd: HWND) -> String {
    let affinity = affinity(hwnd).map_or_else(|| "?".to_owned(), |v| format!("0x{v:X}"));
    format!(
        "{}, {}, display affinity {affinity}",
        if is_iconic(hwnd) {
            "minimised"
        } else {
            "not minimised"
        },
        cloak_text(cloak_value(hwnd)),
    )
}

pub(super) fn short_state(hwnd: HWND) -> String {
    let mut parts = Vec::new();
    if is_iconic(hwnd) {
        parts.push("iconic".to_owned());
    }
    match cloak_value(hwnd) {
        Some(0) => {}
        Some(value) => parts.push(format!("cloak 0x{value:X}")),
        None => parts.push("cloak ?".to_owned()),
    }
    match affinity(hwnd) {
        Some(0) => {}
        Some(value) => parts.push(format!("wda 0x{value:X}")),
        None => parts.push("wda ?".to_owned()),
    }
    if parts.is_empty() {
        "-".to_owned()
    } else {
        parts.join(", ")
    }
}

pub(super) fn windows_build() -> String {
    let key = w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
    let mut buffer = [0u16; 32];
    let mut size = (buffer.len() * size_of::<u16>()) as u32;
    // SAFETY: `buffer` is writable for `size` bytes; RRF_RT_REG_SZ makes the
    // call fail for a value of another type; the names are static literals.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key,
            w!("CurrentBuild"),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    let build = if status == ERROR_SUCCESS {
        util::from_wide(&buffer)
    } else {
        "?".to_owned()
    };
    let mut revision = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: `revision` is a 4-byte buffer and `size` says so;
    // RRF_RT_REG_DWORD refuses other types; the names are static literals.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key,
            w!("UBR"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut revision).cast()),
            Some(&mut size),
        )
    };
    if status == ERROR_SUCCESS {
        format!("{build}.{revision}")
    } else {
        build
    }
}

pub(super) fn cloak_text(value: Option<u32>) -> String {
    let Some(value) = value else {
        return "cloak ?".to_owned();
    };
    if value == 0 {
        return "not cloaked".to_owned();
    }
    let mut parts = Vec::new();
    if value & DWM_CLOAKED_APP != 0 {
        parts.push("app");
    }
    if value & DWM_CLOAKED_SHELL != 0 {
        parts.push("shell");
    }
    if value & DWM_CLOAKED_INHERITED != 0 {
        parts.push("inherited");
    }
    format!("cloaked 0x{value:X} ({})", parts.join("+"))
}
