//! The reads from a window's process through one limited-query handle:
//! its image path and its package AUMID.

use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use windows::Win32::Foundation::{APPMODEL_ERROR_NO_APPLICATION, ERROR_INSUFFICIENT_BUFFER};
use windows::Win32::Storage::Packaging::Appx::{
    APPLICATION_USER_MODEL_ID_MAX_LENGTH, GetApplicationUserModelId,
};
use windows::Win32::System::Threading::{PROCESS_NAME_WIN32, QueryFullProcessImageNameW};
use windows::core::PWSTR;

use crate::PlatformError;
use crate::rules::process_access::DeniedProcesses;
use crate::windows::util;

/// Initial / maximum buffer for `QueryFullProcessImageNameW` (NT path limit).
const PATH_BUF_INITIAL: usize = 1024;
const PATH_BUF_MAX: usize = 32 * 1024;
/// Largest AUMID buffer we grow to on `ERROR_INSUFFICIENT_BUFFER` (the
/// documented maximum is `APPLICATION_USER_MODEL_ID_MAX_LENGTH`, 130).
const AUMID_BUF_MAX: usize = 1024;

/// Processes whose `OpenProcess` refusal has been logged, shared by
/// every thread that queries windows (events, identity, icons) so the line
/// appears once per process whichever of them meets the window first.
static DENIED_PROCESSES: Mutex<DeniedProcesses> = Mutex::new(DeniedProcesses::new());

/// A limited-query handle to a process, or `None` when it cannot be opened
/// (protected or exited process). Never an error: identity degrades to
/// title and class. A refusal is logged at debug once per process
/// ([`DENIED_PROCESSES`]) and at trace afterwards, so a protected process's
/// window, queried again on every event about it, does not fill the log.
pub(super) fn open_process(process_id: u32) -> Option<util::ProcessHandle> {
    match util::ProcessHandle::open_limited(process_id) {
        Ok(h) => {
            denied_processes().clear(process_id);
            Some(h)
        }
        Err(PlatformError::AccessDenied) => {
            if denied_processes().note(process_id) {
                log::debug!(
                    "process {process_id}: access denied, no image path or AUMID (its windows \
                     keep title, class and `class:` identity; logged once per process)"
                );
            } else {
                log::trace!("process {process_id}: access denied (already logged)");
            }
            None
        }
        Err(e) => {
            log::debug!("process {process_id}: OpenProcess failed: {e}");
            None
        }
    }
}

/// The shared refusal memory; a poisoned lock (a panic while held, which
/// nothing here can cause) still yields the set.
fn denied_processes() -> std::sync::MutexGuard<'static, DeniedProcesses> {
    DENIED_PROCESSES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Full image path of a process through an open handle, or `None` when it
/// cannot be read.
pub(super) fn process_image_path(handle: &util::ProcessHandle, process_id: u32) -> Option<PathBuf> {
    let mut buf = vec![0u16; PATH_BUF_INITIAL];
    loop {
        let mut len = buf.len() as u32;
        // SAFETY: `buf` is writable for `len` UTF-16 units and `len` is a
        // live in/out pointer; the handle is open for the guard's lifetime.
        let result = unsafe {
            QueryFullProcessImageNameW(
                handle.raw(),
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
        };
        match result {
            Ok(()) => {
                let path = String::from_utf16_lossy(&buf[..(len as usize).min(buf.len())]);
                return (!path.is_empty()).then(|| PathBuf::from(path));
            }
            Err(e)
                if e.code() == util::hresult_of(ERROR_INSUFFICIENT_BUFFER)
                    && buf.len() < PATH_BUF_MAX =>
            {
                buf.resize(PATH_BUF_MAX, 0);
            }
            Err(e) => {
                log::debug!("process {process_id}: QueryFullProcessImageNameW failed: {e}");
                return None;
            }
        }
    }
}

/// The AppUserModelID of a process that runs with package identity
/// (`GetApplicationUserModelId`), or `None` for a plain desktop process
/// (`APPMODEL_ERROR_NO_APPLICATION`) and on any other failure. A buffer
/// too small is retried once with the length the API asks for, within
/// [`AUMID_BUF_MAX`]. Sends no messages; cannot block on a hung target.
pub(super) fn process_app_user_model_id(
    handle: &util::ProcessHandle,
    process_id: u32,
) -> Option<String> {
    let mut buf = vec![0u16; APPLICATION_USER_MODEL_ID_MAX_LENGTH as usize];
    loop {
        let mut len = buf.len() as u32;
        // SAFETY: `buf` is writable for `len` UTF-16 units and `len` is a
        // live in/out pointer; the handle is open for the guard's lifetime.
        let err = unsafe {
            GetApplicationUserModelId(handle.raw(), &mut len, Some(PWSTR(buf.as_mut_ptr())))
        };
        if err.is_ok() {
            let aumid = util::from_wide(&buf[..(len as usize).min(buf.len())]);
            let aumid = aumid.trim();
            return (!aumid.is_empty()).then(|| aumid.to_owned());
        }
        let needed = len as usize;
        if err == ERROR_INSUFFICIENT_BUFFER && needed > buf.len() && needed <= AUMID_BUF_MAX {
            buf.resize(needed, 0);
            continue;
        }
        if err == APPMODEL_ERROR_NO_APPLICATION {
            log::trace!("process {process_id}: no package identity");
        } else {
            log::trace!("process {process_id}: GetApplicationUserModelId failed: {err:?}");
        }
        return None;
    }
}
