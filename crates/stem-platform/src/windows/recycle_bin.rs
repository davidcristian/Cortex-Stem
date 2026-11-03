//! Recycle Bin.
//!
//! Contract:
//! - [`query`] -> `SHQueryRecycleBinW(NULL)` (all drives) into [`RecycleBinState`].
//! - [`open`]  -> `ShellExecuteExW("shell:RecycleBinFolder")`.
//! - [`empty`] -> `SHEmptyRecycleBinW(NULL, NULL, 0)`: flags 0 keep the
//!   shell's confirmation dialog, progress UI and sound.
//!
//! `SHEmptyRecycleBinW` is modal: it blocks its thread until the user has
//! answered the confirmation and the files are gone. It therefore runs on a
//! short-lived thread owned by this crate
//! (`docs/development/windows-rules.md`) and [`empty`] returns immediately;
//! the event thread's recycle-bin poll reports the new state once the
//! operation completes.

use stem_core::RecycleBinState;
use windows::Win32::UI::Shell::{SHEmptyRecycleBinW, SHQUERYRBINFO, SHQueryRecycleBinW};
use windows::core::PCWSTR;

use super::util;
use crate::{PlatformError, PlatformResult};

pub fn query() -> PlatformResult<RecycleBinState> {
    let mut info = SHQUERYRBINFO {
        cbSize: size_of::<SHQUERYRBINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a valid out-structure with `cbSize` set; a NULL root
    // path queries every drive's bin.
    unsafe { SHQueryRecycleBinW(PCWSTR::null(), &mut info) }?;
    Ok(RecycleBinState {
        item_count: u64::try_from(info.i64NumItems).unwrap_or(0),
        size_bytes: u64::try_from(info.i64Size).unwrap_or(0),
    })
}

pub fn open() -> PlatformResult<()> {
    let _com = util::ComApartment::sta();
    super::launch::shell_execute("shell:RecycleBinFolder", None, None)
}

pub fn empty() -> PlatformResult<()> {
    std::thread::Builder::new()
        .name("dock-empty-recycle-bin".into())
        .spawn(|| {
            let _com = util::ComApartment::sta();
            // SAFETY: no owner window, NULL root path (all drives); flags 0
            // keep the confirmation UI, so nothing is deleted without consent.
            match unsafe { SHEmptyRecycleBinW(None, PCWSTR::null(), 0) } {
                Ok(()) => log::info!("recycle bin emptied"),
                // Cancelled by the user, or already empty: not a problem.
                Err(e) => log::info!("SHEmptyRecycleBinW did not complete: {e}"),
            }
        })
        .map(drop)
        .map_err(|e| PlatformError::Other(format!("could not start the recycle-bin thread: {e}")))
}
