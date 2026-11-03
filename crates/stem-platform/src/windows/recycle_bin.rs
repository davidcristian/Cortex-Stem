//! Recycle Bin.
//!
//! Contract:
//! - [`query`] -> `SHQueryRecycleBinW(None)` (all drives) into [`RecycleBinState`].
//! - [`open`]  -> `ShellExecuteExW("shell:RecycleBinFolder")`.
//! - [`empty`] -> `SHEmptyRecycleBinW(None, SHERB_NOCONFIRMATION? no — keep
//!   the confirmation dialog; flags 0)`.

use stem_core::RecycleBinState;

use crate::{PlatformError, PlatformResult};

pub fn query() -> PlatformResult<RecycleBinState> {
    Err(PlatformError::Unsupported("recycle bin access"))
}

pub fn open() -> PlatformResult<()> {
    Err(PlatformError::Unsupported("recycle bin access"))
}

pub fn empty() -> PlatformResult<()> {
    Err(PlatformError::Unsupported("recycle bin access"))
}
