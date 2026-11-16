//! The OS interface: the [`Platform`] trait, the types its methods take, and
//! the choice of the implementation for this host.

mod appbar;
mod backdrop;
mod dock_window;
mod error;
mod interface;
#[cfg(test)]
mod tests;

use std::sync::Arc;

pub use appbar::{AppBarGrant, AppBarRequest, AppBarSink};
pub use backdrop::{BackdropMaterial, DockBackdrop};
pub use dock_window::{DockWindowOptions, HitRegion, NativeWindowHandle};
pub use error::{PlatformError, PlatformResult};
pub use interface::{EventSink, Platform};

use crate::mock;
#[cfg(windows)]
use crate::windows;

/// Whether `platform` is the scripted [`mock::MockPlatform`] (by its
/// [`Platform::name`]), whichever way it was chosen: `STEM_PLATFORM=mock`, or
/// a host that has no real implementation. The app layer uses this to keep
/// demo runs away from the real settings file.
pub fn is_mock(platform: &dyn Platform) -> bool {
    platform.name() == MOCK_PLATFORM_NAME
}

/// What [`mock::MockPlatform::name`] returns.
const MOCK_PLATFORM_NAME: &str = "mock";

/// Build the platform for this host. Honors `STEM_PLATFORM=mock` on any host
/// (surrounding blanks ignored: cmd's `set STEM_PLATFORM=mock && …` keeps
/// the space before `&&`).
pub fn create_platform() -> Arc<dyn Platform> {
    if std::env::var("STEM_PLATFORM").is_ok_and(|v| v.trim().eq_ignore_ascii_case("mock")) {
        return Arc::new(mock::MockPlatform::with_demo_scenario());
    }
    #[cfg(windows)]
    {
        Arc::new(windows::WindowsPlatform::new())
    }
    #[cfg(not(windows))]
    {
        Arc::new(mock::MockPlatform::with_demo_scenario())
    }
}
