use pretty_assertions::assert_eq;

use super::*;

#[test]
fn the_query_rect_lies_on_the_monitor_bounds_edge_on_all_four_edges() {
    assert_eq!(
        query_rect(&request(MONITOR, DockPosition::Bottom, 72)),
        Rect::new(0, 1008, 1920, 72)
    );
    assert_eq!(
        query_rect(&request(MONITOR, DockPosition::Top, 72)),
        Rect::new(0, 0, 1920, 72)
    );
    assert_eq!(
        query_rect(&request(MONITOR, DockPosition::Left, 72)),
        Rect::new(0, 0, 72, 1080)
    );
    assert_eq!(
        query_rect(&request(MONITOR, DockPosition::Right, 72)),
        Rect::new(1848, 0, 72, 1080)
    );
    // A negative origin: the right edge is the boundary with the primary.
    assert_eq!(
        query_rect(&request(LEFT_MONITOR, DockPosition::Right, 108)),
        Rect::new(-108, 0, 108, 1440)
    );
    assert_eq!(
        query_rect(&request(LEFT_MONITOR, DockPosition::Left, 108)),
        Rect::new(-2560, 0, 108, 1440)
    );
}

#[test]
fn the_band_is_at_least_one_pixel_and_at_most_half_the_monitor() {
    assert_eq!(
        band_thickness(&request(MONITOR, DockPosition::Bottom, 0)),
        1
    );
    assert_eq!(
        band_thickness(&request(MONITOR, DockPosition::Bottom, -5)),
        1
    );
    assert_eq!(
        band_thickness(&request(MONITOR, DockPosition::Bottom, 900)),
        540
    );
    assert_eq!(
        band_thickness(&request(MONITOR, DockPosition::Left, 1500)),
        960
    );
    assert_eq!(
        band_thickness(&request(MONITOR, DockPosition::Left, 72)),
        72
    );
    assert_eq!(
        query_rect(&request(MONITOR, DockPosition::Top, 5000)),
        Rect::new(0, 0, 1920, 540)
    );
    // A degenerate monitor still gives a 1 px band.
    assert_eq!(
        query_rect(&request(Rect::new(0, 0, 0, 0), DockPosition::Bottom, 72)),
        Rect::new(0, -1, 1, 1)
    );
}

#[test]
fn the_set_rect_keeps_the_side_the_shell_moved_and_puts_the_thickness_back() {
    // A visible 48 px taskbar at the bottom: the shell moves the
    // proposal's bottom up to the taskbar's top and leaves its top where
    // it was, which would make the band 24 px thick.
    let bottom = request(MONITOR, DockPosition::Bottom, 72);
    assert_eq!(
        set_rect(Rect::new(0, 1008, 1920, 24), &bottom),
        Rect::new(0, 960, 1920, 72)
    );
    // Another bar 30 px thick at the top.
    let top = request(MONITOR, DockPosition::Top, 72);
    assert_eq!(
        set_rect(Rect::new(0, 30, 1920, 42), &top),
        Rect::new(0, 30, 1920, 72)
    );
    // On the left, trimmed at its lower end by the bottom taskbar.
    let left = request(MONITOR, DockPosition::Left, 72);
    assert_eq!(
        set_rect(Rect::new(0, 0, 72, 1032), &left),
        Rect::new(0, 0, 72, 1032)
    );
    // On the right, past a 20 px bar on the same edge and trimmed.
    let right = request(MONITOR, DockPosition::Right, 72);
    assert_eq!(
        set_rect(Rect::new(1848, 0, 52, 1032), &right),
        Rect::new(1828, 0, 72, 1032)
    );
    // Unchanged by a shell with nothing on that edge.
    for edge in [
        DockPosition::Bottom,
        DockPosition::Top,
        DockPosition::Left,
        DockPosition::Right,
    ] {
        let r = request(MONITOR, edge, 72);
        assert_eq!(set_rect(query_rect(&r), &r), query_rect(&r), "{edge:?}");
    }
}

#[test]
fn the_window_sits_with_its_edge_side_on_the_band_on_all_four_edges() {
    // The band above a 48 px taskbar; the window is 135 px thick.
    assert_eq!(
        window_rect(Rect::new(0, 960, 1920, 72), DockPosition::Bottom, 135),
        Rect::new(0, 897, 1920, 135)
    );
    assert_eq!(
        window_rect(Rect::new(0, 30, 1920, 72), DockPosition::Top, 135),
        Rect::new(0, 30, 1920, 135)
    );
    assert_eq!(
        window_rect(Rect::new(0, 0, 72, 1032), DockPosition::Left, 135),
        Rect::new(0, 0, 135, 1032)
    );
    assert_eq!(
        window_rect(Rect::new(1828, 0, 72, 1032), DockPosition::Right, 135),
        Rect::new(1765, 0, 135, 1032)
    );
    // A negative origin, on the boundary with the primary.
    assert_eq!(
        window_rect(Rect::new(-108, 0, 108, 1392), DockPosition::Right, 203),
        Rect::new(-203, 0, 203, 1392)
    );
    // Never empty.
    assert_eq!(
        window_rect(Rect::new(0, 960, 1920, 72), DockPosition::Bottom, 0),
        Rect::new(0, 1031, 1920, 1)
    );
}

#[test]
fn the_reserved_thickness_runs_from_the_panels_far_side_to_the_windows_edge_side() {
    // A horizontal window 1920×135 with a 640×64 panel 8 px above its
    // bottom: 64 + 8.
    let panel = Rect::new(640, 63, 640, 64);
    assert_eq!(
        reserved_thickness((1920, 135), panel, DockPosition::Bottom),
        Some(72)
    );
    // On the top edge, 8 px below the window's top.
    let panel = Rect::new(640, 8, 640, 64);
    assert_eq!(
        reserved_thickness((1920, 135), panel, DockPosition::Top),
        Some(72)
    );
    // Vertical: a 64×640 panel 8 px from the window's left side.
    let panel = Rect::new(8, 200, 64, 640);
    assert_eq!(
        reserved_thickness((135, 1032), panel, DockPosition::Left),
        Some(72)
    );
    // And 8 px from its right side.
    let panel = Rect::new(63, 200, 64, 640);
    assert_eq!(
        reserved_thickness((135, 1032), panel, DockPosition::Right),
        Some(72)
    );
    // The window grown for a drag: the panel's place follows, the band
    // does not.
    let panel = Rect::new(640, 935, 640, 64);
    assert_eq!(
        reserved_thickness((1920, 1007), panel, DockPosition::Bottom),
        Some(72)
    );
    // A panel longer than its window along the edge still has its
    // thickness across it.
    let panel = Rect::new(-40, 63, 2000, 64);
    assert_eq!(
        reserved_thickness((1920, 135), panel, DockPosition::Bottom),
        Some(72)
    );
}

#[test]
fn the_reserved_thickness_is_clamped_to_the_window_and_zero_without_a_panel() {
    assert_eq!(
        reserved_thickness((1920, 135), Rect::new(0, 0, 0, 0), DockPosition::Bottom),
        Some(0)
    );
    assert_eq!(
        reserved_thickness((1920, 135), Rect::new(10, 10, 0, 64), DockPosition::Top),
        Some(0)
    );
    // Rounded outwards past the window's sides by a px: inside, and
    // never more than the window.
    assert_eq!(
        reserved_thickness(
            (1920, 135),
            Rect::new(640, 72, 640, 64),
            DockPosition::Bottom
        ),
        Some(63)
    );
    assert_eq!(
        reserved_thickness((1920, 135), Rect::new(640, -1, 640, 137), DockPosition::Top),
        Some(135)
    );
    assert_eq!(
        reserved_thickness(
            (135, 1032),
            Rect::new(-1, 200, 137, 640),
            DockPosition::Right
        ),
        Some(135)
    );
}

#[test]
fn a_panel_outside_its_window_across_the_edge_has_no_thickness() {
    // The icon size going from 96 to 48 on `bottom`: the page measures
    // the smaller panel (64 css high) in the 226 css window, 8 above its
    // bottom, but the report is read after the window shrank to 149.
    // 149 - 154 would be 0: a band given back for nothing.
    assert_eq!(
        reserved_thickness(
            (1920, 149),
            Rect::new(640, 154, 640, 64),
            DockPosition::Bottom
        ),
        None
    );
    // The same after a drop on `bottom`: measured in the 1007 css high
    // window the drag grew, read in the 135 css one.
    assert_eq!(
        reserved_thickness(
            (1920, 135),
            Rect::new(640, 935, 640, 64),
            DockPosition::Bottom
        ),
        None
    );
    // And on `right`: measured in the grown window, the column
    // lies past the side of the 150 css wide one.
    assert_eq!(
        reserved_thickness(
            (150, 928),
            Rect::new(1572, 144, 64, 640),
            DockPosition::Right
        ),
        None
    );
    // Once the page measured in the window's own viewport again.
    assert_eq!(
        reserved_thickness((150, 928), Rect::new(78, 144, 64, 640), DockPosition::Right),
        Some(72)
    );
    // A panel rect still laid out for the bottom read on the left edge.
    assert_eq!(
        reserved_thickness((135, 1032), Rect::new(640, 63, 640, 64), DockPosition::Left),
        None
    );
    // Above the window, and past its top side.
    assert_eq!(
        reserved_thickness((1920, 135), Rect::new(0, -70, 640, 64), DockPosition::Top),
        None
    );
    assert_eq!(
        reserved_thickness((1920, 135), Rect::new(0, -2, 640, 64), DockPosition::Bottom),
        None
    );
    // Every edge: a panel more than a px past the far side.
    for (window, panel, edge) in [
        ((1920, 135), Rect::new(0, 72, 640, 65), DockPosition::Bottom),
        ((1920, 135), Rect::new(0, 72, 640, 65), DockPosition::Top),
        ((135, 1032), Rect::new(72, 0, 65, 640), DockPosition::Left),
        ((135, 1032), Rect::new(72, 0, 65, 640), DockPosition::Right),
    ] {
        assert_eq!(reserved_thickness(window, panel, edge), None, "{edge:?}");
    }
}

#[test]
fn a_band_is_usable_only_when_it_is_not_empty_and_inside_its_monitor() {
    let r = request(MONITOR, DockPosition::Bottom, 72);
    assert!(is_usable(Rect::new(0, 960, 1920, 72), &r));
    assert!(is_usable(MONITOR, &r));
    assert!(!is_usable(Rect::new(0, 960, 1920, 0), &r));
    assert!(!is_usable(Rect::new(0, 960, 0, 72), &r));
    assert!(
        !is_usable(Rect::new(0, 1050, 1920, 72), &r),
        "past the bottom"
    );
    assert!(
        !is_usable(Rect::new(-10, 960, 1920, 72), &r),
        "past the left"
    );
    let left = request(LEFT_MONITOR, DockPosition::Right, 108);
    assert!(is_usable(Rect::new(-108, 0, 108, 1440), &left));
    assert!(
        !is_usable(Rect::new(1812, 0, 108, 1080), &left),
        "the primary"
    );
}

#[test]
fn a_work_area_excludes_a_band_until_it_grows_over_it_on_all_four_edges() {
    // Bottom: the band above a 48 px taskbar.
    let band = Rect::new(0, 960, 1920, 72);
    assert!(work_area_excludes(
        Rect::new(0, 0, 1920, 960),
        band,
        DockPosition::Bottom
    ));
    assert!(!work_area_excludes(
        Rect::new(0, 0, 1920, 1032),
        band,
        DockPosition::Bottom
    ));
    // Top.
    let band = Rect::new(0, 0, 1920, 72);
    assert!(work_area_excludes(
        Rect::new(0, 72, 1920, 960),
        band,
        DockPosition::Top
    ));
    assert!(!work_area_excludes(
        Rect::new(0, 0, 1920, 1032),
        band,
        DockPosition::Top
    ));
    // Left.
    let band = Rect::new(0, 0, 72, 1032);
    assert!(work_area_excludes(
        Rect::new(72, 0, 1848, 1032),
        band,
        DockPosition::Left
    ));
    assert!(!work_area_excludes(
        Rect::new(0, 0, 1920, 1032),
        band,
        DockPosition::Left
    ));
    // Right, on a monitor with a negative origin.
    let band = Rect::new(-108, 0, 108, 1440);
    assert!(work_area_excludes(
        Rect::new(-2560, 0, 2452, 1392),
        band,
        DockPosition::Right
    ));
    assert!(!work_area_excludes(
        Rect::new(-2560, 0, 2560, 1392),
        band,
        DockPosition::Right
    ));
}
