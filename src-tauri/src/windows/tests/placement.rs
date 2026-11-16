use super::*;
use crate::windows::panel::panel_on_screen;
use crate::windows::placement::{
    DockContent, LENGTH_RANGE, PlaceStep, THICKNESS_RANGE, clamp_content, edge_rect, is_vertical,
    physical_px, placement_steps, plan_dock_rect, relayout_rect, scale_rect, thickness_of,
    with_min_length,
};

const EDGES: [DockPosition; 4] = [
    DockPosition::Bottom,
    DockPosition::Top,
    DockPosition::Left,
    DockPosition::Right,
];

/// The invariants of the placement rule on any edge: inside the work
/// area, flush with the edge, as long as the work area along it.
fn assert_on_edge(work_area: Rect, edge: DockPosition, rect: Rect) {
    assert!(rect.width >= 1 && rect.height >= 1, "{rect:?} is empty");
    assert!(
        rect.x >= work_area.x
            && rect.y >= work_area.y
            && rect.right() <= work_area.right()
            && rect.bottom() <= work_area.bottom(),
        "{rect:?} leaves {work_area:?}"
    );
    if is_vertical(edge) {
        assert_eq!((rect.y, rect.height), (work_area.y, work_area.height));
    } else {
        assert_eq!((rect.x, rect.width), (work_area.x, work_area.width));
    }
    let flush = match edge {
        DockPosition::Bottom => rect.bottom() == work_area.bottom(),
        DockPosition::Top => rect.y == work_area.y,
        DockPosition::Left => rect.x == work_area.x,
        DockPosition::Right => rect.right() == work_area.right(),
    };
    assert!(flush, "{rect:?} is not flush with the {edge:?} edge");
}

#[test]
fn edge_rect_spans_the_work_area_along_each_edge_and_is_flush_with_it() {
    // A work area that starts away from the origin (a taskbar on the
    // left and on top, say).
    let wa = Rect::new(100, 50, 1000, 900);
    assert_eq!(
        edge_rect(wa, DockPosition::Bottom, 80),
        Rect::new(100, 870, 1000, 80)
    );
    assert_eq!(
        edge_rect(wa, DockPosition::Top, 80),
        Rect::new(100, 50, 1000, 80)
    );
    assert_eq!(
        edge_rect(wa, DockPosition::Left, 80),
        Rect::new(100, 50, 80, 900)
    );
    assert_eq!(
        edge_rect(wa, DockPosition::Right, 80),
        Rect::new(1020, 50, 80, 900)
    );
    for edge in EDGES {
        for thickness in [-5, 0, 1, 80, 899, 900, 901, 1000, 5000, i32::MAX] {
            assert_on_edge(wa, edge, edge_rect(wa, edge, thickness));
        }
    }
}

#[test]
fn edge_rect_clamps_the_thickness_to_the_work_area_and_never_empties_the_window() {
    let wa = Rect::new(100, 50, 1000, 900);
    // Thicker than the work area: the whole work area, whichever edge.
    for edge in EDGES {
        assert_eq!(edge_rect(wa, edge, 5000), wa, "{edge:?}");
    }
    // The thickness clamps against the work area's height on a
    // horizontal dock and its width on a vertical one.
    assert_eq!(edge_rect(wa, DockPosition::Top, 950).height, 900);
    assert_eq!(edge_rect(wa, DockPosition::Left, 950).width, 950);
    assert_eq!(edge_rect(wa, DockPosition::Right, 1200).width, 1000);
    // Degenerate thicknesses give one pixel, flush with the edge.
    assert_eq!(
        edge_rect(wa, DockPosition::Bottom, 0),
        Rect::new(100, 949, 1000, 1)
    );
    assert_eq!(
        edge_rect(wa, DockPosition::Right, -5),
        Rect::new(1099, 50, 1, 900)
    );
    // A degenerate work area still gives a 1×1 window on its edge.
    let empty = Rect::new(0, 0, 0, 0);
    assert_eq!(
        edge_rect(empty, DockPosition::Bottom, 10),
        Rect::new(0, -1, 1, 1)
    );
    assert_eq!(
        edge_rect(empty, DockPosition::Top, 10),
        Rect::new(0, 0, 1, 1)
    );
    assert_eq!(
        edge_rect(empty, DockPosition::Left, 10),
        Rect::new(0, 0, 1, 1)
    );
    assert_eq!(
        edge_rect(empty, DockPosition::Right, 10),
        Rect::new(-1, 0, 1, 1)
    );
}

#[test]
fn edge_rect_keeps_a_secondary_monitors_negative_origin() {
    // A monitor left of and above the primary, its taskbar at the
    // bottom: work area (-1920, -1080) .. (0, -48).
    let wa = Rect::new(-1920, -1080, 1920, 1032);
    assert_eq!(
        edge_rect(wa, DockPosition::Bottom, 96),
        Rect::new(-1920, -144, 1920, 96)
    );
    assert_eq!(
        edge_rect(wa, DockPosition::Top, 96),
        Rect::new(-1920, -1080, 1920, 96)
    );
    assert_eq!(
        edge_rect(wa, DockPosition::Left, 96),
        Rect::new(-1920, -1080, 96, 1032)
    );
    assert_eq!(
        edge_rect(wa, DockPosition::Right, 96),
        Rect::new(-96, -1080, 96, 1032)
    );
    for edge in EDGES {
        assert_on_edge(wa, edge, edge_rect(wa, edge, 96));
        assert_on_edge(wa, edge, edge_rect(wa, edge, 4000));
    }
}

#[test]
fn plan_dock_rect_takes_the_thickness_across_the_edge_and_scales_it() {
    let wa = Rect::new(0, 0, 2560, 1400);
    // 150 %: 96 css -> 144 physical across the edge; the value along
    // the edge (the panel's length, 700) does not count.
    assert_eq!(
        plan_dock_rect(wa, 1.5, 700.0, 96.0, DockPosition::Bottom),
        Rect::new(0, 1256, 2560, 144)
    );
    assert_eq!(
        plan_dock_rect(wa, 1.5, 700.0, 96.0, DockPosition::Top),
        Rect::new(0, 0, 2560, 144)
    );
    assert_eq!(
        plan_dock_rect(wa, 1.5, 96.0, 700.0, DockPosition::Left),
        Rect::new(0, 0, 144, 1400)
    );
    assert_eq!(
        plan_dock_rect(wa, 1.5, 96.0, 700.0, DockPosition::Right),
        Rect::new(2416, 0, 144, 1400)
    );
    // Fractional results round to the nearest pixel.
    assert_eq!(
        plan_dock_rect(wa, 1.25, 700.0, 97.0, DockPosition::Bottom).height,
        121
    );
    assert_eq!(
        plan_dock_rect(wa, 1.25, 97.0, 700.0, DockPosition::Right).width,
        121
    );
    assert_eq!(physical_px(641.0, 1.25), 801);
    assert_eq!(physical_px(-3.0, 1.0), 0);
    // A secondary monitor to the right of the primary, at 150 %: its own
    // work area on each edge.
    let right = Rect::new(2560, 0, 1920, 1032);
    assert_eq!(
        plan_dock_rect(right, 1.5, 700.0, 96.0, DockPosition::Bottom),
        Rect::new(2560, 888, 1920, 144)
    );
    assert_eq!(
        plan_dock_rect(right, 1.5, 96.0, 700.0, DockPosition::Left),
        Rect::new(2560, 0, 144, 1032)
    );
    assert_eq!(
        plan_dock_rect(right, 1.5, 96.0, 700.0, DockPosition::Right),
        Rect::new(4336, 0, 144, 1032)
    );
}

#[test]
fn content_keeps_its_thickness_across_an_edge_change() {
    // Reported on the bottom: 700 long, 135 thick.
    let bottom = DockContent {
        width: 700.0,
        height: 135.0,
        edge: DockPosition::Bottom,
    };
    assert_eq!(bottom.on_edge(DockPosition::Bottom), (700.0, 135.0));
    assert_eq!(bottom.on_edge(DockPosition::Top), (700.0, 135.0));
    // On a vertical edge the 135 is the width, the window's thickness.
    assert_eq!(bottom.on_edge(DockPosition::Left), (135.0, 700.0));
    assert_eq!(bottom.on_edge(DockPosition::Right), (135.0, 700.0));
    let wa = Rect::new(0, 0, 2560, 1392);
    for edge in EDGES {
        let (width, height) = bottom.on_edge(edge);
        let rect = plan_dock_rect(wa, 1.0, width, height, edge);
        let thickness = if is_vertical(edge) {
            rect.width
        } else {
            rect.height
        };
        assert_eq!(thickness, 135, "{edge:?}");
    }
    // Reported on the right: the width is the thickness.
    let right = DockContent {
        width: 135.0,
        height: 700.0,
        edge: DockPosition::Right,
    };
    assert_eq!(right.on_edge(DockPosition::Left), (135.0, 700.0));
    assert_eq!(right.on_edge(DockPosition::Top), (700.0, 135.0));
}

#[test]
fn a_window_keeps_the_thickness_it_was_placed_with() {
    // Before its first placement the initial height is the thickness.
    assert_eq!(thickness_of((640, 96), None), 96);
    assert_eq!(thickness_of((2560, 135), Some(DockPosition::Bottom)), 135);
    assert_eq!(thickness_of((2560, 135), Some(DockPosition::Top)), 135);
    assert_eq!(thickness_of((135, 1392), Some(DockPosition::Left)), 135);
    assert_eq!(thickness_of((135, 1392), Some(DockPosition::Right)), 135);
    assert_eq!(
        thickness_of((u32::MAX, 1), Some(DockPosition::Left)),
        i32::MAX
    );
    // Moved from the bottom to the left at that thickness.
    let wa = Rect::new(0, 0, 2560, 1392);
    let placed = edge_rect(wa, DockPosition::Bottom, 135);
    let size = (placed.width.unsigned_abs(), placed.height.unsigned_abs());
    let moved = edge_rect(
        wa,
        DockPosition::Left,
        thickness_of(size, Some(DockPosition::Bottom)),
    );
    assert_eq!(moved, Rect::new(0, 0, 135, 1392));
}

#[test]
fn clamp_content_bounds_the_thickness_and_the_length_by_their_roles() {
    // Horizontal: the height is the thickness.
    assert_eq!(
        clamp_content(DockPosition::Bottom, 10.0, 10.0),
        (LENGTH_RANGE.0, THICKNESS_RANGE.0)
    );
    assert_eq!(
        clamp_content(DockPosition::Top, 1e9, 1e9),
        (LENGTH_RANGE.1, THICKNESS_RANGE.1)
    );
    // Vertical: the width is the thickness, so a drag's very thick
    // window is still allowed across a left or right edge.
    assert_eq!(
        clamp_content(DockPosition::Left, 4096.0, 700.0),
        (4096.0, 700.0)
    );
    assert_eq!(
        clamp_content(DockPosition::Right, 10.0, 10.0),
        (THICKNESS_RANGE.0, LENGTH_RANGE.0)
    );
    assert_eq!(
        clamp_content(DockPosition::Right, 1e9, 1e9),
        (THICKNESS_RANGE.1, LENGTH_RANGE.1)
    );
    assert_eq!(
        clamp_content(DockPosition::Bottom, 700.0, 135.0),
        (700.0, 135.0)
    );
}

#[test]
fn with_min_length_grows_the_work_area_along_the_edge_only() {
    let wa = Rect::new(-1920, 0, 1920, 1040);
    assert_eq!(
        with_min_length(wa, DockPosition::Bottom, 2400),
        Rect::new(-1920, 0, 2400, 1040)
    );
    assert_eq!(
        with_min_length(wa, DockPosition::Top, 100),
        wa,
        "never shrinks"
    );
    assert_eq!(
        with_min_length(wa, DockPosition::Left, 1500),
        Rect::new(-1920, 0, 1920, 1500)
    );
    assert_eq!(
        with_min_length(wa, DockPosition::Right, 100),
        wa,
        "never shrinks"
    );
}

#[test]
fn scale_rect_rounds_outwards() {
    // 150 %: 10..=20 css → 15..=30 physical exactly.
    assert_eq!(
        scale_rect(Rect::new(10, 10, 10, 10), 1.5),
        Rect::new(15, 15, 15, 15)
    );
    // 125 %: left 11*1.25=13.75 → 13; right 21*1.25=26.25 → 27.
    assert_eq!(
        scale_rect(Rect::new(11, 0, 10, 8), 1.25),
        Rect::new(13, 0, 14, 10)
    );
    // A bad scale factor means "as is".
    assert_eq!(
        scale_rect(Rect::new(1, 2, 3, 4), f64::NAN),
        Rect::new(1, 2, 3, 4)
    );
    assert_eq!(
        scale_rect(Rect::new(1, 2, 3, 4), 0.0),
        Rect::new(1, 2, 3, 4)
    );
}

#[test]
fn validate_hit_region_requires_containment_and_sizes() {
    let panel = Rect::new(100, 40, 400, 60);
    assert_eq!(
        validate_hit_region(panel, Rect::new(80, 0, 440, 100)),
        Ok(())
    );
    assert_eq!(validate_hit_region(panel, panel), Ok(()));
    assert!(validate_hit_region(panel, Rect::new(120, 0, 440, 100)).is_err());
    assert!(validate_hit_region(panel, Rect::new(80, 0, 440, 90)).is_err());
    assert!(validate_hit_region(Rect::new(0, 0, -1, 10), Rect::new(0, 0, 10, 10)).is_err());
    assert!(validate_hit_region(Rect::new(0, 0, 1, 1), Rect::new(0, 0, 10, -10)).is_err());
}

#[test]
fn panel_on_screen_scales_like_a_hit_region_and_offsets_by_the_window_origin() {
    // A 561 × 64 panel at (360, 168) in a window placed at (0, 1248) at 1×.
    assert_eq!(
        panel_on_screen((0, 1248), Rect::new(360, 168, 561, 64), 1.0),
        Rect::new(360, 1416, 561, 64)
    );
    // 150 %: the CSS rect rounds outwards before the offset (11 css → 16.5 → 16).
    assert_eq!(
        panel_on_screen((100, 2000), Rect::new(11, 0, 10, 8), 1.5),
        Rect::new(116, 2000, 16, 12)
    );
    // A monitor left of the primary: negative origin, kept.
    assert_eq!(
        panel_on_screen((-1920, 944), Rect::new(600, 20, 500, 70), 1.0),
        Rect::new(-1320, 964, 500, 70)
    );
    assert_eq!(validate_panel_rect(Rect::new(0, 0, 0, 0)), Ok(()));
    assert!(validate_panel_rect(Rect::new(0, 0, -1, 10)).is_err());
    assert!(validate_panel_rect(Rect::new(0, 0, 10, -1)).is_err());
}

#[test]
fn plan_dock_rect_clamps_to_the_work_area() {
    let wa = Rect::new(-1920, 0, 1920, 1040);
    // A drag's very thick window at 200 %: the whole work area.
    assert_eq!(
        plan_dock_rect(wa, 2.0, 700.0, 1024.0, DockPosition::Bottom),
        wa
    );
    assert_eq!(
        plan_dock_rect(wa, 2.0, 1024.0, 700.0, DockPosition::Right),
        wa
    );
    // Degenerate sizes never produce a zero-sized window.
    for edge in EDGES {
        let rect = plan_dock_rect(wa, 1.0, 0.0, 0.0, edge);
        assert_on_edge(wa, edge, rect);
        let rect = plan_dock_rect(Rect::new(0, 0, 0, 0), 1.0, 10.0, 10.0, edge);
        assert_eq!((rect.width, rect.height), (1, 1), "{edge:?}");
    }
}

fn inside(inner: Rect, outer: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.right() <= outer.right()
        && inner.bottom() <= outer.bottom()
}

/// The window's rect after the call `step`, from its rect before.
fn apply(step: PlaceStep, rect: Rect) -> Rect {
    match step {
        PlaceStep::Size { width, height } => Rect::new(rect.x, rect.y, width, height),
        PlaceStep::Move { x, y } => Rect::new(x, y, rect.width, rect.height),
    }
}

/// Run `steps` from `current` and check that every rect the window
/// passes through lies inside `current` or `target`, and that it ends
/// at `target`.
fn assert_steps_stay_inside(current: Rect, target: Rect) {
    let steps = placement_steps(Some(current), target);
    assert!(steps.len() <= 3, "{steps:?}");
    let mut rect = current;
    for step in &steps {
        rect = apply(*step, rect);
        assert!(
            inside(rect, current) || inside(rect, target),
            "{current:?} -> {target:?}: {rect:?} after {step:?} of {steps:?} leaves both"
        );
        assert!(rect.width >= 1 && rect.height >= 1, "{rect:?} is empty");
    }
    assert_eq!(rect, target, "{current:?} -> {target:?} by {steps:?}");
}

#[test]
fn placement_steps_never_pass_through_a_rect_outside_the_old_and_the_new_place() {
    // A 150 % secondary left of the primary, work area x -1920..0: its
    // dock goes from the right edge to the top. Resize-then-move would
    // pass through 1920x144 at x = -144, mostly on the primary.
    let secondary = Rect::new(-1920, 0, 1920, 1032);
    let right = edge_rect(secondary, DockPosition::Right, 144);
    let top = edge_rect(secondary, DockPosition::Top, 144);
    assert_eq!(right, Rect::new(-144, 0, 144, 1032));
    assert_eq!(
        placement_steps(Some(right), top),
        vec![
            PlaceStep::Size {
                width: 144,
                height: 144
            },
            PlaceStep::Move { x: -1920, y: 0 },
            PlaceStep::Size {
                width: 1920,
                height: 144
            },
        ]
    );
    // The bottom dock growing for a drag moves up first and
    // grows down into its new place, never below the work area (onto a
    // monitor under this one); it shrinks back where it stands, then
    // moves down.
    let wa = Rect::new(0, 0, 2560, 1392);
    let rest = edge_rect(wa, DockPosition::Bottom, 135);
    let grown = edge_rect(wa, DockPosition::Bottom, 1000);
    assert_eq!(
        placement_steps(Some(rest), grown),
        vec![
            PlaceStep::Move { x: 0, y: 392 },
            PlaceStep::Size {
                width: 2560,
                height: 1000
            },
        ]
    );
    assert_eq!(
        placement_steps(Some(grown), rest),
        vec![
            PlaceStep::Size {
                width: 2560,
                height: 135
            },
            PlaceStep::Move { x: 0, y: 1257 },
        ]
    );
    // A top or left dock keeps its corner: one resize; nothing to do
    // when it is already in place; a pure move is one move.
    let top_rest = edge_rect(wa, DockPosition::Top, 135);
    assert_eq!(
        placement_steps(Some(top_rest), edge_rect(wa, DockPosition::Top, 1000)),
        vec![PlaceStep::Size {
            width: 2560,
            height: 1000
        }]
    );
    assert_eq!(placement_steps(Some(rest), rest), Vec::new());
    let shifted = Rect::new(rest.x - 2560, rest.y, rest.width, rest.height);
    assert_eq!(
        placement_steps(Some(shifted), rest),
        vec![PlaceStep::Move {
            x: rest.x,
            y: rest.y
        }]
    );
    // The window's rect unknown: resize, then move.
    assert_eq!(
        placement_steps(None, rest),
        vec![
            PlaceStep::Size {
                width: 2560,
                height: 135
            },
            PlaceStep::Move { x: 0, y: 1257 },
        ]
    );
    // Every edge to every edge, at several thicknesses, on a monitor
    // with a negative origin and on one right of the primary, and from
    // one monitor to another (a reconcile after a new primary).
    let areas = [
        wa,
        secondary,
        Rect::new(2560, -200, 1920, 1032),
        Rect::new(0, 48, 1280, 672),
    ];
    for from_area in areas {
        for to_area in areas {
            for from in EDGES {
                for to in EDGES {
                    for (a, b) in [(135, 135), (96, 144), (135, 1000), (1000, 135), (1, 5000)] {
                        assert_steps_stay_inside(
                            edge_rect(from_area, from, a),
                            edge_rect(to_area, to, b),
                        );
                    }
                }
            }
        }
    }
    // A freshly created window at the initial size somewhere on screen.
    assert_steps_stay_inside(
        Rect::new(300, 200, 640, 96),
        edge_rect(wa, DockPosition::Right, 96),
    );
}

#[test]
fn relayout_rect_lays_a_window_out_from_its_content_or_its_current_thickness() {
    let wa = Rect::new(0, 0, 2560, 1392);
    let bottom = DockContent {
        width: 700.0,
        height: 135.0,
        edge: DockPosition::Bottom,
    };
    let placed = edge_rect(wa, DockPosition::Bottom, 135);
    let size = (placed.width.unsigned_abs(), placed.height.unsigned_abs());
    // Bottom -> left with a content size reported on the bottom: the
    // 135 css thickness is now the width.
    assert_eq!(
        relayout_rect(
            wa,
            true,
            DockPosition::Left,
            Some(bottom),
            1.0,
            size,
            Some(DockPosition::Bottom)
        ),
        Rect::new(0, 0, 135, 1392)
    );
    // The same switch before any report: the window's height on the
    // bottom is its thickness, now its width.
    assert_eq!(
        relayout_rect(
            wa,
            true,
            DockPosition::Left,
            None,
            1.0,
            size,
            Some(DockPosition::Bottom)
        ),
        Rect::new(0, 0, 135, 1392)
    );
    // A first placement (never placed): the initial height, on any edge.
    assert_eq!(
        relayout_rect(wa, true, DockPosition::Right, None, 1.5, (960, 144), None),
        Rect::new(2416, 0, 144, 1392)
    );
    // Reported on the right, laid out again on the right at 150 %.
    let right = DockContent {
        width: 96.0,
        height: 700.0,
        edge: DockPosition::Right,
    };
    assert_eq!(
        relayout_rect(
            wa,
            true,
            DockPosition::Right,
            Some(right),
            1.5,
            (135, 1392),
            Some(DockPosition::Right)
        ),
        Rect::new(2416, 0, 144, 1392)
    );
    // No monitor list yet (Tauri's own view of the monitor): the content's
    // length is the lower bound along a vertical edge too, the origin kept.
    let small = Rect::new(0, 0, 1920, 1040);
    let tall = DockContent {
        width: 135.0,
        height: 1500.0,
        edge: DockPosition::Left,
    };
    assert_eq!(
        relayout_rect(
            small,
            false,
            DockPosition::Left,
            Some(tall),
            1.0,
            (135, 1040),
            Some(DockPosition::Left)
        ),
        Rect::new(0, 0, 135, 1500)
    );
    assert_eq!(
        relayout_rect(
            small,
            false,
            DockPosition::Right,
            Some(tall),
            1.25,
            (135, 1040),
            Some(DockPosition::Left)
        ),
        Rect::new(1751, 0, 169, 1875)
    );
    // With a monitor list the work area is the length, whatever the
    // content says.
    assert_eq!(
        relayout_rect(
            small,
            true,
            DockPosition::Left,
            Some(tall),
            1.0,
            (135, 1040),
            Some(DockPosition::Left)
        ),
        Rect::new(0, 0, 135, 1040)
    );
    // A DPI change (100 % -> 150 %) before any report: tao kept the
    // window's logical size (2560x96 -> 3840x144), so the 144 px it has
    // across its bottom edge is kept; with a report, the content's 96 css
    // gives the same 144 px.
    assert_eq!(
        relayout_rect(
            wa,
            true,
            DockPosition::Bottom,
            None,
            1.5,
            (3840, 144),
            Some(DockPosition::Bottom)
        ),
        Rect::new(0, 1248, 2560, 144)
    );
    let bottom_96 = DockContent {
        width: 700.0,
        height: 96.0,
        edge: DockPosition::Bottom,
    };
    assert_eq!(
        relayout_rect(
            wa,
            true,
            DockPosition::Bottom,
            Some(bottom_96),
            1.5,
            (3840, 144),
            Some(DockPosition::Bottom)
        ),
        Rect::new(0, 1248, 2560, 144)
    );
    // Every edge stays flush and full length, with or without content.
    for edge in EDGES {
        for content in [None, Some(bottom), Some(right), Some(tall)] {
            let rect = relayout_rect(
                wa,
                true,
                edge,
                content,
                1.5,
                size,
                Some(DockPosition::Bottom),
            );
            assert_on_edge(wa, edge, rect);
        }
    }
}

#[test]
fn the_panel_of_a_right_or_bottom_dock_lands_on_its_edge_on_screen() {
    // The conversions of the panel rect and the hit region are generic
    // rects: they only scale and offset by the window's own origin, which
    // on a right dock is not the work area's left. A secondary monitor
    // right of the primary at 150 %, work area 1920×1032 at (2560, 0).
    let wa = Rect::new(2560, 0, 1920, 1032);
    let window = plan_dock_rect(wa, 1.5, 96.0, 600.0, DockPosition::Right);
    assert_eq!(window, Rect::new(4336, 0, 144, 1032));
    // The panel rests against the window's right side: 64 css wide with
    // its right edge at the viewport's right (96 css), centred along.
    let css = Rect::new(32, 210, 64, 268);
    let panel = panel_on_screen((window.x, window.y), css, 1.5);
    assert_eq!(panel, Rect::new(4384, 315, 96, 402));
    assert_eq!(panel.right(), wa.right(), "flush with the screen edge");
    // The hit region stays window-local: the same scale, no offset.
    assert_eq!(scale_rect(css, 1.5), Rect::new(48, 315, 96, 402));
    // A bottom dock on a monitor left of the primary: negative origin.
    let wa = Rect::new(-1920, 0, 1920, 1040);
    let window = plan_dock_rect(wa, 1.0, 700.0, 96.0, DockPosition::Bottom);
    let panel = panel_on_screen((window.x, window.y), Rect::new(610, 26, 700, 70), 1.0);
    assert_eq!(panel, Rect::new(-1310, 970, 700, 70));
    assert_eq!(panel.bottom(), wa.bottom());
    // A top dock: the window's origin is the work area's.
    let wa = Rect::new(0, 48, 2560, 1392);
    let window = plan_dock_rect(wa, 1.0, 700.0, 96.0, DockPosition::Top);
    let panel = panel_on_screen((window.x, window.y), Rect::new(930, 0, 700, 70), 1.0);
    assert_eq!(panel, Rect::new(930, 48, 700, 70));
}
