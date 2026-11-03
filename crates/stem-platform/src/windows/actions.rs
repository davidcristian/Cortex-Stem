//! Window actions.
//!
//! Contract:
//! - [`perform`] implements `Activate` (restore if minimised, then bring to
//!   foreground honouring the `SetForegroundWindow` rules with the documented
//!   fallbacks: `AllowSetForegroundWindow`, the ALT key-press trick,
//!   `AttachThreadInput`; log which path was used), `Minimize`
//!   (`ShowWindowAsync(SW_MINIMIZE)`), `Restore` (`SW_RESTORE`), and `Close`
//!   (`PostMessageW(WM_CLOSE)` — never `DestroyWindow`).
//! - [`foreground_window`] returns the current foreground window if it is a
//!   taskbar candidate (see `enumerate::is_taskbar_candidate`).
//! - Returns `WindowNotFound` for dead handles, never panics.

use stem_core::{WindowAction, WindowId};

use crate::{PlatformError, PlatformResult};

pub fn perform(_id: WindowId, _action: WindowAction) -> PlatformResult<()> {
    Err(PlatformError::Unsupported("window action handling"))
}

pub fn foreground_window() -> Option<WindowId> {
    None
}
