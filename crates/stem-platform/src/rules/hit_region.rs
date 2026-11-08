//! Hit-region hysteresis.
//!
//! Decides whether a dock window should accept mouse input from the cursor
//! position and the [`HitRegion`] the UI reported. Pure: the Windows
//! tracker thread (`windows::hit_test`) gathers the cursor position in
//! window-local physical pixels and applies the answer.
//!
//! The rule, from `docs/ipc/hit-region.md`:
//! - *outside → inside* only when the point is in `panel` (the visible dock
//!   panel at rest);
//! - *inside → outside* only when the point has left `envelope` (the panel
//!   plus everything magnification can lift above it), so an icon that grew
//!   past the panel's edge stays clickable until the cursor leaves it;
//! - an envelope that does not contain the panel is treated as
//!   `envelope ∪ panel`, so a bad envelope can never make the panel itself
//!   release the cursor;
//! - rectangles are half-open (`[x, x + width)`); a zero-sized rectangle
//!   contains nothing.

use crate::HitRegion;

/// The next hysteresis state for a cursor at `point` (window-local physical
/// pixels), given the current state `inside` (`true` = the window accepts
/// mouse input).
pub fn next_inside(inside: bool, point: (i32, i32), region: &HitRegion) -> bool {
    let (x, y) = point;
    let in_panel = region.panel.contains(x, y);
    if inside {
        in_panel || region.envelope.contains(x, y)
    } else {
        in_panel
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use stem_core::Rect;

    use super::*;

    /// A 400×60 panel at (100, 40) inside a window 600×100; the envelope
    /// adds 40 px of headroom above the panel and 20 px on each side.
    fn region() -> HitRegion {
        HitRegion {
            panel: Rect::new(100, 40, 400, 60),
            envelope: Rect::new(80, 0, 440, 100),
        }
    }

    #[test]
    fn entering_through_the_headroom_does_not_activate() {
        let region = region();
        // In the envelope, above the panel.
        assert!(!next_inside(false, (300, 10), &region));
        // Beside the panel, inside the envelope's side slack.
        assert!(!next_inside(false, (90, 70), &region));
        // Far away.
        assert!(!next_inside(false, (10, 10), &region));
    }

    #[test]
    fn entering_through_the_panel_activates() {
        let region = region();
        assert!(next_inside(false, (300, 70), &region));
        // Corners of the half-open panel.
        assert!(next_inside(false, (100, 40), &region));
        assert!(next_inside(false, (499, 99), &region));
        assert!(!next_inside(false, (500, 99), &region));
        assert!(!next_inside(false, (499, 100), &region));
    }

    #[test]
    fn staying_in_the_envelope_above_the_panel_keeps_the_window() {
        let region = region();
        assert!(next_inside(true, (300, 10), &region));
        assert!(next_inside(true, (300, 0), &region));
        assert!(next_inside(true, (80, 50), &region));
        assert!(next_inside(true, (519, 50), &region));
        // Still inside the panel itself, of course.
        assert!(next_inside(true, (300, 70), &region));
    }

    #[test]
    fn leaving_the_envelope_releases_the_window() {
        let region = region();
        assert!(!next_inside(true, (79, 50), &region));
        assert!(!next_inside(true, (520, 50), &region));
        assert!(!next_inside(true, (300, -1), &region));
        assert!(!next_inside(true, (300, 100), &region));
    }

    #[test]
    fn a_full_round_trip_follows_the_hysteresis() {
        let region = region();
        let path = [
            ((300, 10), false), // enters through the headroom: stays outside
            ((300, 70), true),  // reaches the panel: inside
            ((300, 10), true),  // back up into the headroom: still inside
            ((300, -5), false), // leaves the envelope: outside
            ((300, 10), false), // the headroom alone never re-activates
        ];
        let mut inside = false;
        for (point, expected) in path {
            inside = next_inside(inside, point, &region);
            assert_eq!(inside, expected, "at {point:?}");
        }
    }

    #[test]
    fn an_envelope_smaller_than_the_panel_is_widened_to_the_panel() {
        let region = HitRegion {
            panel: Rect::new(100, 40, 400, 60),
            envelope: Rect::new(200, 50, 10, 10),
        };
        // Inside the panel but outside the envelope: the panel wins.
        assert!(next_inside(true, (120, 45), &region));
        assert!(next_inside(false, (120, 45), &region));
        // Outside both.
        assert!(!next_inside(true, (50, 45), &region));
        assert!(!next_inside(false, (50, 45), &region));
    }

    #[test]
    fn degenerate_rectangles_contain_nothing() {
        let empty = HitRegion {
            panel: Rect::new(10, 10, 0, 0),
            envelope: Rect::new(0, 0, 0, 0),
        };
        assert!(!next_inside(false, (10, 10), &empty));
        assert!(!next_inside(true, (10, 10), &empty));
        assert!(!next_inside(true, (0, 0), &empty));

        let negative = HitRegion {
            panel: Rect::new(10, 10, -5, -5),
            envelope: Rect::new(0, 0, -1, 100),
        };
        assert!(!next_inside(false, (10, 10), &negative));
        assert!(!next_inside(true, (7, 7), &negative));
        assert!(!next_inside(true, (0, 50), &negative));
    }

    /// The UI's "no clickable area" (`NO_HIT_REGION`): 0 × 0 at the origin
    /// as both rectangles, reported while the dock is hidden in fullscreen
    /// above a visible taskbar, where there is no reveal strip and
    /// the window must be click-through everywhere.
    fn no_hit_region() -> HitRegion {
        HitRegion {
            panel: Rect::new(0, 0, 0, 0),
            envelope: Rect::new(0, 0, 0, 0),
        }
    }

    #[test]
    fn the_empty_region_at_the_origin_never_takes_the_cursor() {
        let region = no_hit_region();
        let points = [
            (0, 0),
            (-1, -1),
            (1, 1),
            (0, 1),
            (1, 0),
            (-1, 0),
            (0, -1),
            (300, 70),
            (599, 99),
            (i32::MIN, i32::MIN),
            (i32::MAX, i32::MAX),
        ];
        for point in points {
            assert!(!next_inside(false, point, &region), "outside, at {point:?}");
            assert!(!next_inside(true, point, &region), "inside, at {point:?}");
        }
    }

    #[test]
    fn the_empty_region_releases_a_window_that_was_inside() {
        // The dock hid under the cursor while it was over the panel: the
        // next sample, wherever it is, makes the window click-through, and
        // it stays so.
        let shown = region();
        let hidden = no_hit_region();
        let mut inside = next_inside(false, (300, 70), &shown);
        assert!(inside);
        for point in [(300, 70), (0, 0), (300, 10), (0, 0)] {
            inside = next_inside(inside, point, &hidden);
            assert!(!inside, "at {point:?}");
        }
    }
}
