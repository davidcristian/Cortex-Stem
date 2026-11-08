//! Does a window cover the dock?
//!
//! `autoHide.mode = whenOverlapped` hides the dock while some window on its
//! monitor overlaps the panel where it rests, and shows it again when none
//! does. The fact is computed here, from what the platform already reports
//! about every window ([`RawWindow`]) and the panel rect the UI reports
//! through `set_dock_panel_rect`, converted by the app layer to the same
//! coordinate space (physical pixels of the virtual desktop).
//!
//! The rule ([`overlapped`]): a window counts when it
//! - is visible (`is_visible`), not minimised and not cloaked, whatever
//!   `behavior.showOnlyCurrentDesktop` says: a cloaked window is not on
//!   screen, so it cannot cover anything;
//! - has a frame (`rect: Some`); a platform that could not read it says
//!   nothing about it;
//! - belongs to another process (`process_id != own_process_id`): our own
//!   windows (the settings window over the dock) never hide it;
//! - and its frame intersects the panel with a **positive area**: frames
//!   that merely touch an edge do not count.
//!
//! The monitor is implicit in the geometry: a window on another monitor has
//! a frame that cannot intersect a panel on this one. `fullscreen`
//! is a separate fact; the UI combines the two.

use crate::model::{RawWindow, Rect};

/// Whether `a` and `b` share a region of positive area. Rects with a zero or
/// negative size intersect nothing; touching edges do not count.
pub fn intersects(a: &Rect, b: &Rect) -> bool {
    if a.width <= 0 || a.height <= 0 || b.width <= 0 || b.height <= 0 {
        return false;
    }
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

/// Whether `window` covers part of `panel`: on screen (visible, not
/// minimised, not cloaked), with a known frame, of another process, and
/// intersecting the panel with a positive area.
pub fn covers(panel: &Rect, window: &RawWindow, own_process_id: Option<u32>) -> bool {
    if !window.is_visible || window.is_minimized || window.is_cloaked {
        return false;
    }
    if own_process_id == Some(window.process_id) {
        return false;
    }
    window
        .rect
        .as_ref()
        .is_some_and(|frame| intersects(panel, frame))
}

/// `DockState::overlapped` for a panel resting at `panel` (physical screen
/// pixels) among `windows`: whether any of them [`covers`] it. O(windows);
/// a panel with no area is never overlapped.
pub fn overlapped<'a>(
    panel: &Rect,
    windows: impl IntoIterator<Item = &'a RawWindow>,
    own_process_id: Option<u32>,
) -> bool {
    if panel.width <= 0 || panel.height <= 0 {
        return false;
    }
    windows
        .into_iter()
        .any(|window| covers(panel, window, own_process_id))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::model::WindowId;

    /// A 560 × 72 panel resting at the bottom of a 2560 × 1392 work area.
    const PANEL: Rect = Rect::new(1000, 1320, 560, 72);

    fn window(id: u64, rect: Option<Rect>) -> RawWindow {
        RawWindow {
            id: WindowId(id),
            title: format!("W{id}"),
            class_name: "Window".into(),
            process_id: 1000 + id as u32,
            exe_path: Some(PathBuf::from(r"C:\a.exe")),
            app_user_model_id: None,
            is_visible: true,
            is_minimized: false,
            is_cloaked: false,
            monitor: None,
            rect,
            icon_key: None,
        }
    }

    #[test]
    fn intersects_needs_a_positive_shared_area() {
        let a = Rect::new(0, 0, 10, 10);
        assert!(intersects(&a, &Rect::new(5, 5, 10, 10)));
        assert!(intersects(&a, &Rect::new(-5, -5, 10, 10)));
        assert!(intersects(&a, &Rect::new(2, 2, 2, 2)), "contained");
        assert!(intersects(&Rect::new(2, 2, 2, 2), &a), "containing");
        assert!(intersects(&a, &a));
        // Touching edges and corners share no area.
        assert!(!intersects(&a, &Rect::new(10, 0, 10, 10)));
        assert!(!intersects(&a, &Rect::new(0, 10, 10, 10)));
        assert!(!intersects(&a, &Rect::new(-10, -10, 10, 10)));
        assert!(!intersects(&a, &Rect::new(10, 10, 10, 10)));
        // Disjoint.
        assert!(!intersects(&a, &Rect::new(11, 0, 10, 10)));
        assert!(!intersects(&a, &Rect::new(0, -11, 10, 10)));
        // Empty or inverted rects intersect nothing, wherever they are.
        assert!(!intersects(&a, &Rect::new(5, 5, 0, 10)));
        assert!(!intersects(&a, &Rect::new(5, 5, 10, 0)));
        assert!(!intersects(&a, &Rect::new(5, 5, -3, 10)));
        assert!(!intersects(&Rect::new(5, 5, 0, 0), &a));
    }

    #[test]
    fn a_window_over_the_panel_overlaps_and_one_beside_it_does_not() {
        let over = window(1, Some(Rect::new(800, 900, 1200, 500)));
        let beside = window(2, Some(Rect::new(0, 0, 900, 1392)));
        let above = window(3, Some(Rect::new(0, 0, 2560, 1320)));
        assert!(covers(&PANEL, &over, None));
        assert!(!covers(&PANEL, &beside, None));
        assert!(
            !covers(&PANEL, &above, None),
            "its bottom edge touches the panel's top"
        );
        assert!(overlapped(&PANEL, [&beside, &above, &over], None));
        assert!(!overlapped(&PANEL, [&beside, &above], None));
        assert!(!overlapped(&PANEL, [] as [&RawWindow; 0], None));
    }

    #[test]
    fn one_pixel_of_overlap_is_enough() {
        let corner = window(
            1,
            Some(Rect::new(PANEL.right() - 1, PANEL.bottom() - 1, 100, 100)),
        );
        assert!(covers(&PANEL, &corner, None));
        let edge = window(2, Some(Rect::new(PANEL.right(), PANEL.y, 100, 100)));
        assert!(!covers(&PANEL, &edge, None));
    }

    #[test]
    fn windows_that_are_not_on_screen_never_overlap() {
        let frame = Some(Rect::new(0, 0, 2560, 1392));
        let mut minimized = window(1, frame);
        minimized.is_minimized = true;
        let mut cloaked = window(2, frame);
        cloaked.is_cloaked = true;
        let mut invisible = window(3, frame);
        invisible.is_visible = false;
        let unknown = window(4, None);
        for w in [&minimized, &cloaked, &invisible, &unknown] {
            assert!(!covers(&PANEL, w, None), "{:?}", w.id);
        }
        assert!(!overlapped(
            &PANEL,
            [&minimized, &cloaked, &invisible, &unknown],
            None
        ));
        // The same frame on a shown window does.
        assert!(overlapped(&PANEL, [&window(5, frame)], None));
    }

    #[test]
    fn our_own_windows_never_overlap() {
        let ours = window(1, Some(Rect::new(0, 0, 2560, 1392)));
        assert!(!covers(&PANEL, &ours, Some(ours.process_id)));
        assert!(covers(&PANEL, &ours, Some(ours.process_id + 1)));
        assert!(covers(&PANEL, &ours, None));
        assert!(!overlapped(&PANEL, [&ours], Some(1001)));
    }

    #[test]
    fn a_monitor_to_the_left_has_negative_coordinates_and_still_works() {
        let panel = Rect::new(-1500, 1000, 560, 72);
        let over = window(1, Some(Rect::new(-1920, 0, 1920, 1080)));
        let right_monitor = window(2, Some(Rect::new(0, 0, 2560, 1440)));
        assert!(overlapped(&panel, [&right_monitor, &over], None));
        assert!(!overlapped(&panel, [&right_monitor], None));
    }

    #[test]
    fn an_empty_panel_is_never_overlapped() {
        let over = window(1, Some(Rect::new(0, 0, 2560, 1392)));
        assert!(!overlapped(&Rect::new(1000, 1320, 0, 72), [&over], None));
        assert!(!overlapped(&Rect::new(1000, 1320, 560, 0), [&over], None));
        assert!(overlapped(&PANEL, [&over], None));
    }
}
