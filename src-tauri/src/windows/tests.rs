use super::*;

use stem_core::settings::{DockPosition, MonitorMode, ThemeMode};
use stem_core::{DockState, MonitorId, MonitorInfo, Rect, WindowAction, WindowId};
use stem_platform::PlatformError;

mod docks;
mod focus;
mod placement;
mod provenance;

fn monitor(id: &str, primary: bool, x: i32, scale: f64) -> MonitorInfo {
    MonitorInfo {
        id: MonitorId(id.into()),
        name: id.trim_start_matches(r"\\.\").to_owned(),
        bounds: Rect::new(x, 0, 2560, 1440),
        work_area: Rect::new(x, 0, 2560, 1392),
        scale_factor: scale,
        is_primary: primary,
    }
}
