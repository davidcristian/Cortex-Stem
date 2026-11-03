//! Shell actions behind the fixed dock items.
//!
//! Contract for [`perform`]:
//! - `OpenStartMenu`: inject a `VK_LWIN` press/release with `SendInput`
//!   (there is no public API).
//! - `OpenTaskView`: `SendInput` Win+Tab.
//! - `ToggleShowDesktop`: `IShellDispatch::ToggleDesktop` via COM
//!   (`Shell.Application`), fallback Win+D.
//! - `OpenRecycleBin` / `EmptyRecycleBin`: `recycle_bin::{open, empty}`.

use stem_core::ShellAction;

use crate::{PlatformError, PlatformResult};

pub fn perform(action: ShellAction) -> PlatformResult<()> {
    match action {
        ShellAction::OpenRecycleBin => super::recycle_bin::open(),
        ShellAction::EmptyRecycleBin => super::recycle_bin::empty(),
        ShellAction::OpenStartMenu | ShellAction::OpenTaskView | ShellAction::ToggleShowDesktop => {
            Err(PlatformError::Unsupported("shell action handling"))
        }
    }
}
