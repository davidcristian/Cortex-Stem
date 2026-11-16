//! Finding the windows the probe cannot create, and reading whether a
//! process is elevated.

use stem_core::RawWindow;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use super::{CALCULATOR_AUMID_PREFIX, STORE_FRAME_CLASS, TASK_MANAGER_CLASS, emit};
use crate::windows::{enumerate, util, virtual_desktop};

/// The windows found for the `Find` slots.
#[derive(Debug, Default)]
pub(super) struct External {
    pub(super) elevated: Option<RawWindow>,
    pub(super) store_app: Option<RawWindow>,
    pub(super) other_desktop: Option<RawWindow>,
    /// Why nothing could be looked for, or why the desktop is unknown.
    pub(super) note: Option<String>,
}

fn is_notepad(window: &RawWindow) -> bool {
    let exe = window
        .exe_path
        .as_ref()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("notepad.exe"));
    let aumid = window
        .app_user_model_id
        .as_deref()
        .is_some_and(|id| id.contains("Notepad"));
    exe || aumid
}

fn is_calculator(window: &RawWindow) -> bool {
    window
        .app_user_model_id
        .as_deref()
        .is_some_and(|id| id.starts_with(CALCULATOR_AUMID_PREFIX))
}

/// Picks the sources the probe cannot create from the taskbar windows:
/// Task Manager by class, Calculator by AUMID (else another minimised or
/// cloaked Store frame), and a window the shell places on another desktop
/// (Notepad first). Windows on the current desktop come first for the
/// first two; no window fills two slots.
pub(super) fn pick_external(windows: &[RawWindow]) -> External {
    let here = |window: &RawWindow| window.on_current_desktop != Some(false);
    let elevated = windows
        .iter()
        .filter(|window| window.class_name == TASK_MANAGER_CLASS)
        .min_by_key(|window| !here(window))
        .cloned();
    let store_app = windows
        .iter()
        .filter(|window| Some(window.id) != elevated.as_ref().map(|w| w.id))
        .filter_map(|window| {
            let rank = if is_calculator(window) && here(window) {
                0
            } else if window.class_name == STORE_FRAME_CLASS
                && (window.is_minimized || window.is_cloaked)
                && here(window)
            {
                1
            } else if is_calculator(window) {
                2
            } else {
                return None;
            };
            Some((rank, window))
        })
        .min_by_key(|(rank, _)| *rank)
        .map(|(_, window)| window.clone());
    let taken = [
        elevated.as_ref().map(|w| w.id),
        store_app.as_ref().map(|w| w.id),
    ];
    let other_desktop = windows
        .iter()
        .filter(|window| window.on_current_desktop == Some(false))
        .filter(|window| !taken.contains(&Some(window.id)))
        .min_by_key(|window| !is_notepad(window))
        .cloned();
    let note = windows
        .iter()
        .all(|window| window.on_current_desktop.is_none())
        .then(|| {
            "the shell gave no virtual desktop facts, so no window counts as on another desktop"
                .to_owned()
        });
    External {
        elevated,
        store_app,
        other_desktop,
        note,
    }
}

pub(super) fn find_external_windows() -> External {
    // The calling thread holds a COM apartment (`run`); the manager slot is
    // released before it.
    let desktops = virtual_desktop::ThreadManager::enable();
    let windows = enumerate::enumerate_taskbar_windows();
    drop(desktops);
    match windows {
        Ok(list) => {
            say!("found {} taskbar windows of other processes", list.len());
            pick_external(&list)
        }
        Err(e) => External {
            note: Some(format!("the window enumeration failed: {e}")),
            ..External::default()
        },
    }
}

/// Owned token handle, closed on drop.
struct Token(HANDLE);

impl Drop for Token {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful OpenProcessToken and is
        // closed exactly once, here.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn token_elevation(process: HANDLE) -> Result<bool, String> {
    let mut raw = HANDLE::default();
    // SAFETY: `process` is a live handle with query access (or the current
    // process pseudo-handle); `raw` is a live out-pointer.
    let opened = unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut raw) };
    opened.map_err(|e| format!("OpenProcessToken: {e}"))?;
    let token = Token(raw);
    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0u32;
    // SAFETY: the buffer is a live TOKEN_ELEVATION and its size is passed;
    // `returned` is a live out-pointer.
    let read = unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            Some((&raw mut elevation).cast()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    };
    read.map_err(|e| format!("GetTokenInformation: {e}"))?;
    Ok(elevation.TokenIsElevated != 0)
}

pub(super) fn own_elevation() -> String {
    // SAFETY: GetCurrentProcess returns a pseudo-handle; it needs no close.
    let process = unsafe { GetCurrentProcess() };
    match token_elevation(process) {
        Ok(true) => "ELEVATED".to_owned(),
        Ok(false) => "not elevated".to_owned(),
        Err(e) => format!("unknown ({e})"),
    }
}

fn process_elevation(process_id: u32) -> String {
    match util::ProcessHandle::open_limited(process_id) {
        Err(e) => format!("elevation unknown (OpenProcess: {e})"),
        Ok(process) => match token_elevation(process.raw()) {
            Ok(true) => "elevated".to_owned(),
            Ok(false) => "NOT elevated".to_owned(),
            Err(e) => format!("elevation unknown ({e})"),
        },
    }
}

pub(super) fn describe_found(window: &RawWindow) -> String {
    let exe = window
        .exe_path
        .as_ref()
        .and_then(|path| path.file_name())
        .map_or_else(
            || "?".to_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
    let desktop = match window.on_current_desktop {
        Some(true) => "this desktop",
        Some(false) => "another desktop",
        None => "desktop unknown",
    };
    format!(
        "found '{}' ({}, pid {}, {exe}, {}, {}, {desktop})",
        window.title,
        window.class_name,
        window.process_id,
        process_elevation(window.process_id),
        if window.is_minimized {
            "minimised"
        } else {
            "not minimised"
        },
    )
}
