//! Application identity.
//!
//! Contract:
//! - [`identify_window`] takes the `RawWindow` facts already collected by
//!   `enumerate` and produces an [`AppIdentity`]: `id` via
//!   `stem_core::app_id_for`, `display_name` from the exe's `FileDescription`
//!   version resource (fallback: `ProductName`, then file stem, then window
//!   title), `icon_key` (`aumid:` for packaged apps, otherwise `exe:`), and a
//!   `launch` target (`StoreApp` for packaged apps, otherwise `Exe`).
//! - [`identify_target`] resolves what the user wants to pin: `.lnk` files are
//!   resolved through `IShellLinkW` (target path, arguments, working dir, icon
//!   location); `shell:AppsFolder\<aumid>` and `StoreApp` map to `aumid:`;
//!   plain executables map to `exe:`. The returned `id` **must** equal the id
//!   the engine would derive for that app's windows, or pins and running
//!   windows will not merge.
//! - Elevated/protected processes: `exe_path` may be `None`; degrade to the
//!   window title and class without failing.

use stem_core::{AppIdentity, LaunchTarget, RawWindow};

use crate::{PlatformError, PlatformResult};

pub fn identify_window(_raw: &RawWindow) -> PlatformResult<AppIdentity> {
    Err(PlatformError::Unsupported("app identification"))
}

pub fn identify_target(_target: &LaunchTarget) -> PlatformResult<AppIdentity> {
    Err(PlatformError::Unsupported("app identification"))
}
