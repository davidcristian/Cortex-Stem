//! AppBar registration (milestone M4).
//!
//! Contract: an [`AppBar`] registers a dock window with `SHAppBarMessage`
//! (`ABM_NEW`, `ABM_QUERYPOS`/`ABM_SETPOS` on the chosen edge with the dock's
//! height) so maximised windows stop short of the dock, answers
//! `ABN_POSCHANGED` on the event thread, and unregisters (`ABM_REMOVE`) on
//! drop. Not used until the user enables "reserve screen space".

use stem_core::Rect;

use crate::{NativeWindowHandle, PlatformError, PlatformResult};

#[derive(Debug)]
pub struct AppBar {
    _handle: NativeWindowHandle,
}

impl AppBar {
    pub fn register(_handle: NativeWindowHandle, _bounds: Rect) -> PlatformResult<Self> {
        Err(PlatformError::Unsupported("appbar registration"))
    }

    pub fn set_bounds(&self, _bounds: Rect) -> PlatformResult<()> {
        Err(PlatformError::Unsupported("appbar registration"))
    }
}
