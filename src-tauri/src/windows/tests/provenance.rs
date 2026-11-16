use super::*;
use crate::windows::panel::panel_on_screen;
use crate::windows::placement::DockContent;
use crate::windows::state::DockWindowState;
use crate::windows::viewport::reanchor_panel;

fn css(width: f64, height: f64) -> CssViewport {
    CssViewport { width, height }
}

#[test]
fn reanchoring_keeps_the_distance_to_the_edge_side_and_the_offset_from_the_centre() {
    use DockPosition::{Bottom, Left, Right, Top};
    // A row 640 css long centred in a 2560 css window, 16 css below the
    // top of a 96 css thick one (8 above its bottom) on `bottom`, 8 css
    // below the top on `top`; a column 640 css long centred in a 1440
    // css tall window, 8 css from the edge side of a 150 css wide one.
    let row = Rect::new(960, 16, 640, 64);
    let top_row = Rect::new(960, 8, 640, 64);
    let column = Rect::new(8, 400, 64, 640);
    let right_column = Rect::new(78, 400, 64, 640);
    let wide = css(2560.0, 96.0);
    let tall = css(150.0, 1440.0);
    let cases = [
        // A thickness change: the window 48 css thicker across the edge.
        (
            Bottom,
            row,
            wide,
            css(2560.0, 144.0),
            Rect::new(960, 64, 640, 64),
        ),
        (Top, top_row, wide, css(2560.0, 144.0), top_row),
        (Left, column, tall, css(198.0, 1440.0), column),
        (
            Right,
            right_column,
            tall,
            css(198.0, 1440.0),
            Rect::new(126, 400, 64, 640),
        ),
        // Thinner again (a drop, a smaller icon size).
        (
            Bottom,
            Rect::new(960, 848, 640, 64),
            css(2560.0, 928.0),
            wide,
            row,
        ),
        (
            Right,
            Rect::new(2488, 400, 64, 640),
            css(2560.0, 1440.0),
            tall,
            right_column,
        ),
        // A length change (another work area): half of it along the edge.
        (
            Bottom,
            row,
            wide,
            css(1920.0, 96.0),
            Rect::new(640, 16, 640, 64),
        ),
        (
            Top,
            top_row,
            wide,
            css(1920.0, 96.0),
            Rect::new(640, 8, 640, 64),
        ),
        (
            Left,
            column,
            tall,
            css(150.0, 1040.0),
            Rect::new(8, 200, 64, 640),
        ),
        (
            Right,
            right_column,
            tall,
            css(150.0, 1040.0),
            Rect::new(78, 200, 64, 640),
        ),
        // Both at once.
        (
            Bottom,
            row,
            wide,
            css(1920.0, 144.0),
            Rect::new(640, 64, 640, 64),
        ),
        (
            Right,
            right_column,
            tall,
            css(198.0, 1040.0),
            Rect::new(126, 200, 64, 640),
        ),
        // Nothing changed.
        (Left, column, tall, tall, column),
    ];
    for (edge, rect, from, to, expected) in cases {
        let moved = reanchor_panel(rect, edge, from, to);
        assert_eq!(
            moved, expected,
            "{edge:?}: {rect:?} from {from:?} to {to:?}"
        );
        assert_eq!((moved.width, moved.height), (rect.width, rect.height));
        let report = PanelReport {
            rect,
            edge,
            viewport: from,
        };
        assert_eq!(report.now(Some(to)), expected);
        assert_eq!(report.now(None), rect, "an unknown viewport: as reported");
    }
}

#[test]
fn reanchoring_at_150_percent_ignores_the_rounding_of_the_viewport() {
    // A bottom dock at 1.5x: a client area of 2560 x 144 px is 1706.67 x
    // 96 css; the page reports whole css px (1707 or 1706, as its
    // `innerWidth` rounds), which is the same viewport.
    let rest = CssViewport::of((2560, 144), 1.5);
    assert!((rest.width - 2560.0 / 1.5).abs() < 1e-9);
    assert_eq!(rest.height, 96.0);
    let rect = Rect::new(534, 16, 640, 64);
    for page in [css(1707.0, 96.0), css(1706.0, 96.0)] {
        assert_eq!(reanchor_panel(rect, DockPosition::Bottom, page, rest), rect);
    }
    // The magnification grows the window to 216 px (144 css): 48 css
    // lower, the same place on screen once the window moved up 72 px.
    let thicker = CssViewport::of((2560, 216), 1.5);
    let moved = reanchor_panel(rect, DockPosition::Bottom, css(1707.0, 96.0), thicker);
    assert_eq!(moved, Rect::new(534, 64, 640, 64));
    assert_eq!(
        panel_on_screen((0, 1176), moved, 1.5),
        panel_on_screen((0, 1248), rect, 1.5)
    );
    assert_eq!(
        panel_on_screen((0, 1248), rect, 1.5),
        Rect::new(801, 1272, 960, 96)
    );
    // Another monitor 2400 px wide (1600 css): half the change along.
    let narrower = CssViewport::of((2400, 144), 1.5);
    assert_eq!(
        reanchor_panel(rect, DockPosition::Bottom, css(1706.0, 96.0), narrower),
        Rect::new(481, 16, 640, 64)
    );

    // A right dock at 1.5x: 225 x 1392 px is 150 x 928 css; 288 px wide
    // is 192 css, and the column keeps its 8 css to the right side.
    let column = Rect::new(78, 144, 64, 640);
    let right = css(150.0, 928.0);
    let wider = CssViewport::of((288, 1392), 1.5);
    let moved = reanchor_panel(column, DockPosition::Right, right, wider);
    assert_eq!(moved, Rect::new(120, 144, 64, 640));
    assert_eq!(
        panel_on_screen((2272, 0), moved, 1.5),
        panel_on_screen((2335, 0), column, 1.5),
        "the same place on screen"
    );
    // A shorter work area (1032 px, 688 css): centred again along it.
    let shorter = CssViewport::of((225, 1032), 1.5);
    assert_eq!(
        reanchor_panel(column, DockPosition::Right, right, shorter),
        Rect::new(78, 24, 64, 640)
    );
}

#[test]
fn a_report_for_another_edge_than_the_windows_is_dropped() {
    use DockPosition::{Bottom, Left};
    // The window was moved to the left edge; the page's reports measured
    // for the bottom edge arrive after that.
    let mut dock = DockWindowState {
        edge: Some(Left),
        ..DockWindowState::default()
    };
    let bottom_size = DockContent {
        width: 1300.0,
        height: 135.0,
        edge: Bottom,
    };
    assert_eq!(dock.take_content(bottom_size, Left), Err(Left));
    assert_eq!(dock.content, None);
    let panel = Rect::new(960, 16, 640, 64);
    let envelope = Rect::new(900, 0, 760, 80);
    assert_eq!(dock.take_hit_region(panel, envelope, Bottom), Err(Left));
    assert_eq!(dock.hit_region_css, None);
    let bottom_panel = PanelReport {
        rect: panel,
        edge: Bottom,
        viewport: css(2560.0, 96.0),
    };
    assert_eq!(dock.take_panel(bottom_panel), Err(Left));
    assert_eq!((dock.panel, dock.reserve_css), (None, None));

    // The reports measured for the left edge are taken.
    let left_size = DockContent {
        width: 135.0,
        height: 800.0,
        edge: Left,
    };
    assert_eq!(dock.take_content(left_size, Left), Ok(None));
    assert_eq!(dock.content, Some(left_size));
    let column = Rect::new(8, 400, 64, 640);
    let column_envelope = Rect::new(8, 340, 110, 760);
    assert_eq!(dock.take_hit_region(column, column_envelope, Left), Ok(()));
    assert_eq!(dock.hit_region_css, Some((column, column_envelope)));
    let left_panel = PanelReport {
        rect: column,
        edge: Left,
        viewport: css(150.0, 1440.0),
    };
    assert_eq!(dock.take_panel(left_panel), Ok(Some(72)));
    assert_eq!((dock.panel, dock.reserve_css), (Some(left_panel), Some(72)));

    // A late one from the bottom edge changes none of them.
    assert_eq!(dock.take_content(bottom_size, Left), Err(Left));
    assert_eq!(dock.take_panel(bottom_panel), Err(Left));
    assert_eq!(dock.take_hit_region(panel, envelope, Bottom), Err(Left));
    assert_eq!(dock.content, Some(left_size));
    assert_eq!((dock.panel, dock.reserve_css), (Some(left_panel), Some(72)));
    assert_eq!(dock.hit_region_css, Some((column, column_envelope)));

    // Before its first placement a window takes any edge.
    let mut fresh = DockWindowState::default();
    assert_eq!(fresh.stale_for(Bottom), None);
    assert_eq!(fresh.take_content(bottom_size, Left), Ok(None));
    assert_eq!(dock.stale_for(Left), None);
    assert_eq!(dock.stale_for(Bottom), Some(Left));
}

#[test]
fn a_content_size_for_the_edge_in_effect_retries_a_failed_move() {
    use DockPosition::{Bottom, Left, Top};
    // The position became `left`, but the window's move there failed: it
    // is still on the bottom edge, with what its page reported there.
    let bottom_size = DockContent {
        width: 1300.0,
        height: 135.0,
        edge: Bottom,
    };
    let mut dock = DockWindowState {
        edge: Some(Bottom),
        content: Some(bottom_size),
        ..DockWindowState::default()
    };
    // The page's hit region and panel rect for the left edge do not
    // belong to a window on the bottom edge: dropped, as before.
    let column = Rect::new(8, 400, 64, 640);
    let column_envelope = Rect::new(8, 340, 110, 760);
    let left_panel = PanelReport {
        rect: column,
        edge: Left,
        viewport: css(150.0, 1440.0),
    };
    assert_eq!(
        dock.take_hit_region(column, column_envelope, Left),
        Err(Bottom)
    );
    assert_eq!(dock.take_panel(left_panel), Err(Bottom));
    // Its content size for the left edge, the edge in effect, is taken,
    // with the edge the window is still on: the layout that follows is
    // the move, retried.
    let left_size = DockContent {
        width: 135.0,
        height: 800.0,
        edge: Left,
    };
    assert_eq!(dock.take_content(left_size, Left), Ok(Some(Bottom)));
    assert_eq!(dock.content, Some(left_size));
    // A late report from an edge that is neither stays dropped.
    let top_size = DockContent {
        edge: Top,
        ..bottom_size
    };
    assert_eq!(dock.take_content(top_size, Left), Err(Bottom));
    assert_eq!(dock.content, Some(left_size));
    // Once that layout has placed the window (`place` remembers the
    // edge), the left edge's other reports are taken and the bottom
    // edge's are the stale ones.
    dock.edge = Some(Left);
    assert_eq!(dock.take_hit_region(column, column_envelope, Left), Ok(()));
    assert_eq!(dock.take_panel(left_panel), Ok(Some(72)));
    assert_eq!(dock.take_content(bottom_size, Left), Err(Left));
    assert_eq!(dock.take_content(left_size, Left), Ok(None));
}

#[test]
fn the_band_is_read_against_the_viewport_the_report_carries() {
    let mut dock = DockWindowState {
        edge: Some(DockPosition::Bottom),
        ..DockWindowState::default()
    };
    // At rest: 80 css from the panel's top to the window's bottom.
    let rest = PanelReport {
        rect: Rect::new(960, 16, 640, 64),
        edge: DockPosition::Bottom,
        viewport: css(2560.0, 96.0),
    };
    assert_eq!(dock.take_panel(rest), Ok(Some(80)));
    // Measured in the drag's grown viewport and handled after the drop
    // shrank the window back: still 80, read against its own viewport
    // (against the shrunk window the rect would lie below its bottom).
    let grown = PanelReport {
        rect: Rect::new(960, 848, 640, 64),
        viewport: css(2560.0, 928.0),
        ..rest
    };
    assert_eq!(dock.take_panel(grown), Ok(Some(80)));
    assert_eq!(dock.reserve_css, Some(80));
    // A rect outside its own viewport (no page sends one) keeps the band.
    let outside = PanelReport {
        rect: Rect::new(960, 100, 640, 64),
        ..rest
    };
    assert_eq!(dock.take_panel(outside), Ok(None));
    assert_eq!(dock.reserve_css, Some(80));
    assert_eq!(dock.panel, Some(outside));

    // A top dock at 1.5x: the page's 1707 x 96 viewport, the panel 8 css
    // from the top: 72 css.
    let mut top = DockWindowState {
        edge: Some(DockPosition::Top),
        ..DockWindowState::default()
    };
    let report = PanelReport {
        rect: Rect::new(534, 8, 640, 64),
        edge: DockPosition::Top,
        viewport: css(1707.0, 96.0),
    };
    assert_eq!(top.take_panel(report), Ok(Some(72)));
}

#[test]
fn a_viewport_must_be_positive_and_finite() {
    assert_eq!(css(1280.0, 240.0).validate(), Ok(()));
    assert_eq!(css(0.5, 0.5).validate(), Ok(()));
    for bad in [
        css(0.0, 240.0),
        css(1280.0, 0.0),
        css(-1.0, 240.0),
        css(f64::NAN, 240.0),
        css(1280.0, f64::INFINITY),
        css(f64::NEG_INFINITY, 240.0),
    ] {
        assert!(bad.validate().is_err(), "{bad:?}");
    }
    // The window's own viewport; an unusable scale counts as 1.
    assert_eq!(CssViewport::of((300, 150), 2.0), css(150.0, 75.0));
    assert_eq!(CssViewport::of((300, 150), f64::NAN), css(300.0, 150.0));
    assert_eq!(CssViewport::of((300, 150), 0.0), css(300.0, 150.0));
    assert_eq!(css(1706.6, 95.5).whole(), (1707, 96));
    // The payload as the page sends it.
    let parsed: CssViewport =
        serde_json::from_str(r#"{ "width": 1280, "height": 240 }"#).expect("parses");
    assert_eq!(parsed, css(1280.0, 240.0));
}
