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

use stem_core::LaunchTarget;

use crate::{PlatformError, PlatformResult};

pub fn launch(_target: &LaunchTarget) -> PlatformResult<()> {
    Err(PlatformError::Unsupported("app launching"))
}
