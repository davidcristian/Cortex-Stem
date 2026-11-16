mod bars;
mod geometry;
mod pacing;

use stem_core::Rect;
use stem_core::settings::DockPosition;

use super::*;
use crate::AppBarRequest;

/// A 1920×1080 monitor at the origin.
const MONITOR: Rect = Rect::new(0, 0, 1920, 1080);
/// A 2560×1440 monitor left of the primary (a negative origin).
const LEFT_MONITOR: Rect = Rect::new(-2560, 0, 2560, 1440);

fn request(monitor: Rect, edge: DockPosition, thickness: i32) -> AppBarRequest {
    AppBarRequest {
        monitor,
        edge,
        thickness,
    }
}
