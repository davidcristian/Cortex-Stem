//! Launching applications.
//!
//! Contract:
//! - `Exe` -> `ShellExecuteExW` with `SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI`,
//!   verb `open`, arguments and working directory honoured.
//! - `Shortcut` -> `ShellExecuteExW` on the `.lnk` itself (the shell resolves it).
//! - `StoreApp` -> `IApplicationActivationManager::ActivateApplication`
//!   (fallback: `ShellExecuteExW("shell:AppsFolder\<aumid>")`).
//! - `Uri` -> `ShellExecuteExW` on the string.
//! - Never blocks on the launched process. Errors map to `PlatformError::Os`.
//!
//! `ShellExecuteExW` returns once the shell has started the target (it does
//! not wait for the process); `SEE_MASK_NOASYNC` only makes the DDE/shell
//! work happen before the call returns so the calling thread may go away.
//! `SEE_MASK_FLAG_NO_UI` suppresses the shell's own error dialogs; failures
//! surface as `PlatformError::Os` for the UI to report.

use stem_core::LaunchTarget;
use windows::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, CoAllowSetForegroundWindow, CoCreateInstance,
};
use windows::Win32::UI::Shell::{
    AO_NONE, ApplicationActivationManager, IApplicationActivationManager, SEE_MASK_FLAG_NO_UI,
    SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{PCWSTR, w};

use super::util;
use crate::PlatformResult;
use crate::rules::identity::{apps_folder_parsing_name, join_command_line};

pub fn launch(target: &LaunchTarget) -> PlatformResult<()> {
    let _com = util::ComApartment::sta();
    match target {
        LaunchTarget::Exe {
            path,
            args,
            working_dir,
        } => {
            let parameters = join_command_line(args);
            let directory = working_dir.as_deref().map(|d| d.to_string_lossy());
            shell_execute(
                &path.to_string_lossy(),
                Some(&parameters),
                directory.as_deref(),
            )
        }
        LaunchTarget::Shortcut { path } => shell_execute(&path.to_string_lossy(), None, None),
        LaunchTarget::StoreApp { app_user_model_id } => {
            let aumid = app_user_model_id.trim();
            match activate_application(aumid) {
                Ok(()) => Ok(()),
                Err(e) => {
                    log::debug!(
                        "ActivateApplication({aumid:?}) failed: {e}; falling back to ShellExecuteExW"
                    );
                    shell_execute(&apps_folder_parsing_name(aumid), None, None)
                }
            }
        }
        LaunchTarget::Uri { uri } => shell_execute(uri, None, None),
    }
}

/// `ShellExecuteExW(open)` without UI and without waiting. Empty `parameters`
/// / `directory` are passed as NULL.
pub(crate) fn shell_execute(
    file: &str,
    parameters: Option<&str>,
    directory: Option<&str>,
) -> PlatformResult<()> {
    let file_w = util::to_wide(file);
    let parameters_w = parameters
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(util::to_wide);
    let directory_w = directory
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(util::to_wide);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: w!("open"),
        lpFile: util::pcwstr(&file_w),
        lpParameters: parameters_w.as_deref().map_or(PCWSTR::null(), util::pcwstr),
        lpDirectory: directory_w.as_deref().map_or(PCWSTR::null(), util::pcwstr),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: `cbSize` is set; every pointer in `info` refers to a
    // NUL-terminated buffer that outlives the call; no process handle is
    // requested (no SEE_MASK_NOCLOSEPROCESS), so nothing needs closing.
    unsafe { ShellExecuteExW(&mut info) }?;
    log::debug!("ShellExecuteExW({file:?}) ok");
    Ok(())
}

/// Activate a packaged app by AUMID through the activation manager.
fn activate_application(aumid: &str) -> PlatformResult<()> {
    // SAFETY: standard activation of the (out-of-process) activation manager.
    let manager: IApplicationActivationManager =
        unsafe { CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_LOCAL_SERVER) }?;
    // Let the activated app come to the foreground (best effort).
    // SAFETY: `manager` is a live COM object; no reserved data.
    if let Err(e) = unsafe { CoAllowSetForegroundWindow(&manager, None) } {
        log::trace!("CoAllowSetForegroundWindow failed: {e}");
    }
    let aumid_w = util::to_wide(aumid);
    // SAFETY: `aumid_w` is NUL-terminated and outlives the call; no arguments.
    let pid =
        unsafe { manager.ActivateApplication(util::pcwstr(&aumid_w), PCWSTR::null(), AO_NONE) }?;
    log::debug!("ActivateApplication({aumid:?}) -> pid {pid}");
    Ok(())
}
