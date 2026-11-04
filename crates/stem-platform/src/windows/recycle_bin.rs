//! Recycle Bin.
//!
//! Contract:
//! - [`query`] -> `SHQueryRecycleBinW(NULL)` (all drives) into [`RecycleBinState`].
//! - [`open`]  -> `ShellExecuteExW("shell:RecycleBinFolder")`.
//! - [`empty`] -> `SHEmptyRecycleBinW(NULL, NULL, 0)`: flags 0 keep the
//!   shell's confirmation dialog, progress UI and sound.
//! - [`recycle`] -> `IFileOperation::DeleteItem` per path with
//!   `FOF_ALLOWUNDO | FOFX_RECYCLEONDELETE | FOF_WANTNUKEWARNING`, then one
//!   `PerformOperations` (a file drop on the bin's item).
//!
//! `SHEmptyRecycleBinW` is modal: it blocks its thread until the user has
//! answered the confirmation and the files are gone. It therefore runs on a
//! short-lived thread owned by this crate
//! (`docs/development/windows-rules.md`) and [`empty`] returns immediately;
//! the event thread's recycle-bin poll reports the new state once the
//! operation completes.
//!
//! [`recycle`] uses the shell's copy engine through `IFileOperation` rather
//! than `SHFileOperationW`: its errors are `HRESULT`s instead of the legacy
//! `DE_*` codes, it takes paths longer than `MAX_PATH`, and each path is a
//! shell item, so no double-NUL-terminated buffer is built by hand. No flag
//! suppresses UI, so the shell shows what Explorer's Delete shows: the
//! confirmation (when the bin's "Display delete confirmation dialog" is on),
//! the progress window for long operations, and its "File In Use" / "Access
//! denied" dialogs, where the user can retry, skip or cancel.
//! `FOFX_RECYCLEONDELETE | FOF_ALLOWUNDO` recycle instead of deleting;
//! `FOF_WANTNUKEWARNING` makes the shell warn before an item that cannot be
//! recycled (its drive has no bin, it is larger than the bin) would be
//! deleted for good, so a drop never destroys a file without the user's
//! consent. There is no owner window (the dialogs are top-level): the dock
//! window lives on the Tauri main thread, and owning a dialog across threads
//! would attach the two threads' input queues.
//!
//! The copy engine runs its dialogs' message loops on the calling thread, so
//! [`recycle`] gives every operation its own short-lived STA thread
//! (`stem-recycle`) and waits for its answer: the caller blocks until the
//! shell is done (see `Platform::delete_to_recycle_bin`), never the thread
//! that pumps the dock window. The bin's new state is the caller's to ask
//! for (the service queries it right after; the poll would see it within
//! `events::RECYCLE_BIN_POLL`).

use stem_core::RecycleBinState;
use windows::Win32::Foundation::ERROR_CANCELLED;
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::Win32::UI::Shell::{
    COPYENGINE_E_USER_CANCELLED, FOF_ALLOWUNDO, FOF_WANTNUKEWARNING, FOFX_RECYCLEONDELETE,
    FileOperation, IFileOperation, IShellItem, SHCreateItemFromParsingName, SHEmptyRecycleBinW,
    SHQUERYRBINFO, SHQueryRecycleBinW,
};
use windows::core::PCWSTR;

use super::util;
use crate::rules::recycle::{nothing_to_recycle, plan_recycle};
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

/// Move `paths` to the Recycle Bin on a thread of its own and wait until the
/// shell is done (see the module docs and `Platform::delete_to_recycle_bin`).
pub fn recycle(paths: &[String]) -> PlatformResult<()> {
    let paths = paths.to_vec();
    let (answer_tx, answer_rx) = crossbeam_channel::bounded::<PlatformResult<()>>(1);
    std::thread::Builder::new()
        .name("stem-recycle".into())
        .spawn(move || {
            let _com = util::ComApartment::sta();
            let _ = answer_tx.send(recycle_here(&paths));
        })
        .map_err(|e| PlatformError::Other(format!("could not start the recycle thread: {e}")))?;
    // No timeout: the shell may be waiting for the user. A thread that died
    // without answering (a panic) drops the sender, which ends the wait.
    answer_rx.recv().unwrap_or_else(|_| {
        Err(PlatformError::Other(
            "the recycle thread ended without an answer".into(),
        ))
    })
}

/// The whole operation, on the calling thread, which holds an STA.
fn recycle_here(paths: &[String]) -> PlatformResult<()> {
    let plan = plan_recycle(paths, may_exist);
    for skipped in &plan.skipped {
        if skipped.reason.is_warning() {
            log::warn!(
                "not moved to the Recycle Bin: {} {}",
                skipped.path,
                skipped.reason
            );
        } else {
            log::debug!("not asked again: {} {}", skipped.path, skipped.reason);
        }
    }
    if plan.recycle.is_empty() {
        return Err(PlatformError::Other(nothing_to_recycle(&plan)));
    }

    // SAFETY: COM is initialised on this thread (an STA, see `recycle`); the
    // copy engine is an in-process shell object.
    let operation: IFileOperation = unsafe { CoCreateInstance(&FileOperation, None, CLSCTX_ALL) }?;
    // SAFETY: `operation` is a live object; the flags are plain values.
    unsafe {
        operation.SetOperationFlags(FOF_ALLOWUNDO | FOFX_RECYCLEONDELETE | FOF_WANTNUKEWARNING)
    }?;
    let mut queued = 0usize;
    let mut unqueued: Option<(String, PlatformError)> = None;
    for path in &plan.recycle {
        match queue_delete(&operation, path) {
            Ok(()) => queued += 1,
            Err(e) => {
                log::warn!("{path} cannot be moved to the Recycle Bin: {e}");
                unqueued.get_or_insert_with(|| (path.clone(), e));
            }
        }
    }
    if queued == 0 {
        return Err(match unqueued {
            Some((path, e)) => PlatformError::Other(format!("{path}: {e}")),
            None => PlatformError::Other(nothing_to_recycle(&plan)),
        });
    }

    // SAFETY: `operation` is live with `queued` deletions queued. The copy
    // engine runs its dialogs' message loops on this thread, which is ours
    // alone, and returns once every item is done, skipped or cancelled.
    let performed = unsafe { operation.PerformOperations() };
    // SAFETY: `operation` is live; the call only reads its outcome.
    let aborted = unsafe { operation.GetAnyOperationsAborted() }.is_ok_and(|b| b.as_bool());
    if let Err(e) = performed {
        log::warn!("moving {queued} item(s) to the Recycle Bin failed: {e}");
        let code = e.code();
        if code == util::hresult_of(ERROR_CANCELLED) || code == COPYENGINE_E_USER_CANCELLED {
            return Err(PlatformError::Other(
                "cancelled in the shell's dialog".into(),
            ));
        }
        return Err(e.into());
    }
    if aborted {
        log::warn!("the shell stopped before all {queued} item(s) were in the Recycle Bin");
        return Err(PlatformError::Other(
            "not everything went to the Recycle Bin: an item was refused or skipped, or the \
             operation was cancelled"
                .into(),
        ));
    }
    if let Some((path, e)) = unqueued {
        return Err(PlatformError::Other(format!("{path}: {e}")));
    }
    log::info!("moved {queued} item(s) to the Recycle Bin");
    Ok(())
}

/// Queue one deletion. Fails when the shell cannot make an item of the path.
fn queue_delete(operation: &IFileOperation, path: &str) -> PlatformResult<()> {
    let wide = util::to_wide(path);
    // SAFETY: `wide` is NUL-terminated and outlives the call; no bind context.
    let item: IShellItem = unsafe { SHCreateItemFromParsingName(util::pcwstr(&wide), None) }?;
    // SAFETY: both objects are live; no per-item progress sink.
    unsafe { operation.DeleteItem(&item, None) }?;
    Ok(())
}

/// Only a definite "not found" counts as missing: a file another program
/// holds open, or one the user may not read, is the shell's to refuse (in
/// its own dialog).
fn may_exist(path: &str) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(_) => true,
        Err(e) => e.kind() != std::io::ErrorKind::NotFound,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A path under the temp directory that is checked to be absent.
    fn absent(name: &str) -> String {
        let path = std::env::temp_dir().join(format!("stem-recycle-{}-{name}", std::process::id()));
        assert!(!path.exists(), "{} exists", path.display());
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn missing_or_odd_paths_never_reach_the_shell() {
        // Nothing usable is left, so no file operation starts and no UI can
        // appear: these only look at the file system.
        let missing = absent("missing.txt");
        let err = recycle(std::slice::from_ref(&missing)).unwrap_err();
        assert_eq!(err.to_string(), format!("{missing} does not exist"));

        let err = recycle(&[absent("a.txt"), absent("b.txt"), "notes.txt".to_owned()]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "none of the 3 paths can be moved to the Recycle Bin"
        );
        let err = recycle(&[]).unwrap_err();
        assert_eq!(err.to_string(), "no paths to move to the Recycle Bin");
    }

    /// Moves a file this test creates to the **real** Recycle Bin, with the
    /// shell's UI on, so it does not run by default. Run it by hand in a
    /// desktop session (`cargo test -p stem-platform -- --ignored
    /// recycles_a_file_it_created`), answer the confirmation if the bin
    /// asks, and delete `stem-recycle-<pid>-scratch.txt` from the bin after.
    #[test]
    #[ignore = "puts a test file into the real Recycle Bin and may show the shell's dialogs"]
    fn recycles_a_file_it_created() {
        let dir = std::env::temp_dir().join(format!("stem-recycle-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(format!("stem-recycle-{}-scratch.txt", std::process::id()));
        std::fs::write(&file, b"a scratch file of the Recycle Bin test").unwrap();
        recycle(&[file.to_string_lossy().into_owned()]).unwrap();
        assert!(!file.exists(), "the file is still in its folder");
        std::fs::remove_dir(&dir).unwrap();
    }
}
