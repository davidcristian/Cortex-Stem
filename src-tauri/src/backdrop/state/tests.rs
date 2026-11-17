use stem_core::Rect;
use stem_core::settings::DockPosition;
use stem_platform::BackdropMaterial;

use super::*;
use crate::windows::{CssViewport, PanelReport};

const DOCK: NativeWindowHandle = NativeWindowHandle(0x1234);

fn acrylic_inputs() -> Inputs {
    Inputs {
        backdrop: Backdrop::Acrylic,
        radius_css: 22,
        theme_mode: ThemeMode::Dark,
    }
}

/// The bottom dock's window at rest at 1.5x: 2560 x 144 px (1706.67 x
/// 96 css) with its top-left corner at (0, 1248).
const ORIGIN: (i32, i32) = (0, 1248);
const REST: (u32, u32) = (2560, 144);

/// The page's report at rest: 640 x 64 css, 16 css below the window's
/// top (8 above its bottom), measured in the page's 1707 x 96 viewport
/// (`innerWidth` is whole css px).
fn report() -> PanelReport {
    PanelReport {
        rect: Rect::new(534, 16, 640, 64),
        edge: DockPosition::Bottom,
        viewport: CssViewport {
            width: 1707.0,
            height: 96.0,
        },
    }
}

/// What `windows::panel::push_dock_panel` hands over for `report` in a window
/// whose top-left corner is `origin` and whose client area is
/// `physical` px at `scale`: the rect re-anchored to that viewport.
fn converted(
    report: PanelReport,
    origin: (i32, i32),
    physical: (u32, u32),
    scale: f64,
) -> Option<PanelConversion> {
    let viewport = CssViewport::of(physical, scale);
    Some(PanelConversion {
        origin,
        css: report.now(Some(viewport)),
        scale,
        viewport: Some(viewport),
    })
}

/// The panel at rest (960 x 96 px at 1.5x).
fn at_rest() -> Option<PanelConversion> {
    converted(report(), ORIGIN, REST, 1.5)
}

fn backdrop_at_rest() -> DockBackdrop {
    DockBackdrop {
        rect: Rect::new(801, 1272, 960, 96),
        corner_radius: 33,
        material: BackdropMaterial::Acrylic,
        dark: true,
    }
}

/// Facts with acrylic, the window's handle and its panel shown at rest,
/// the first request already sent.
fn shown() -> Facts {
    let mut facts = Facts {
        inputs: Some(acrylic_inputs()),
        handle: Some(DOCK),
        ..Facts::default()
    };
    assert!(!facts.panel_converted(at_rest()));
    assert_eq!(
        facts.step().send,
        Some((DOCK, Some(backdrop_at_rest()))),
        "set-up"
    );
    facts
}

#[test]
fn nothing_is_logged_or_sent_before_the_settings_and_the_handle_arrive() {
    let mut facts = Facts::default();
    facts.panel_converted(at_rest());
    assert_eq!(facts.step(), Step::default(), "no settings yet");

    facts.inputs = Some(acrylic_inputs());
    assert_eq!(
        facts.step(),
        Step {
            log: Some(Effective::Native(BackdropMaterial::Acrylic)),
            send: None,
        },
        "the look is logged, but there is no window to ask for"
    );
    assert_eq!(facts.step(), Step::default(), "logged once");

    facts.handle = Some(DOCK);
    assert_eq!(
        facts.step().send,
        Some((DOCK, Some(backdrop_at_rest()))),
        "the handle arrives after the inputs: the request goes now"
    );
    assert_eq!(
        facts.step(),
        Step::default(),
        "an unchanged request is not sent again"
    );
}

#[test]
fn the_panel_moving_or_the_window_hiding_sends_the_new_request_once() {
    let mut facts = shown();
    facts.panel_converted(converted(report(), (0, 1200), REST, 1.5));
    let moved = DockBackdrop {
        rect: Rect::new(801, 1224, 960, 96),
        ..backdrop_at_rest()
    };
    assert_eq!(facts.step().send, Some((DOCK, Some(moved))));
    facts.panel_converted(converted(report(), (0, 1200), REST, 1.5));
    assert_eq!(facts.step().send, None, "the same place again");

    facts.panel_converted(None);
    assert_eq!(facts.step().send, Some((DOCK, None)), "window hidden");
    assert_eq!(facts.step().send, None);
    facts.panel_converted(at_rest());
    assert_eq!(facts.step().send, Some((DOCK, Some(backdrop_at_rest()))));
}

#[test]
fn a_panel_fits_when_it_lies_inside_the_viewport() {
    let rest = CssViewport::of(REST, 1.5);
    assert!(panel_fits(Rect::new(534, 16, 640, 64), rest));
    // Rounded outwards past the bottom edge by less than a px: fits.
    assert!(panel_fits(Rect::new(534, 32, 640, 65), rest));
    // Below the viewport's bottom edge, or above its top.
    assert!(!panel_fits(Rect::new(534, 848, 640, 64), rest));
    assert!(!panel_fits(Rect::new(534, -8, 640, 64), rest));
    // A row longer than its window: material would stick out of it.
    assert!(!panel_fits(Rect::new(-40, 16, 2600, 64), rest));

    // A right dock: a column 8 css from the window's right side.
    let right = CssViewport {
        width: 150.0,
        height: 928.0,
    };
    assert!(panel_fits(Rect::new(78, 144, 64, 640), right));
    assert!(!panel_fits(Rect::new(1572, 144, 64, 640), right));
    assert!(!panel_fits(Rect::new(-8, 144, 64, 640), right));
}

/// A magnification change makes the window
/// thicker (144 px to 216 px across the edge at 1.5x) while the panel
/// keeps its size. On `top` and `left` the page's rect stays the same,
/// so the page does not report it again; read against the resized
/// window without re-anchoring, that rect counted as stale and the
/// material was gone until the rect changed. Re-anchored, the panel is
/// where it was on screen on every edge, and the material stays.
#[test]
fn a_thickness_change_keeps_the_material_on_every_edge() {
    struct Case {
        edge: DockPosition,
        /// The page's rect and viewport at rest.
        rect: Rect,
        viewport: (f64, f64),
        /// The window before and after (origin, client area px).
        before: ((i32, i32), (u32, u32)),
        after: ((i32, i32), (u32, u32)),
        /// The material, the same before and after.
        material: Rect,
    }
    let cases = [
        Case {
            edge: DockPosition::Top,
            rect: Rect::new(534, 8, 640, 64),
            viewport: (1707.0, 96.0),
            before: ((0, 0), (2560, 144)),
            after: ((0, 0), (2560, 216)),
            material: Rect::new(801, 12, 960, 96),
        },
        Case {
            edge: DockPosition::Left,
            rect: Rect::new(8, 144, 64, 640),
            viewport: (150.0, 928.0),
            before: ((0, 0), (225, 1392)),
            after: ((0, 0), (288, 1392)),
            material: Rect::new(12, 216, 96, 960),
        },
        Case {
            edge: DockPosition::Bottom,
            rect: Rect::new(534, 16, 640, 64),
            viewport: (1707.0, 96.0),
            before: ((0, 1248), (2560, 144)),
            after: ((0, 1176), (2560, 216)),
            material: Rect::new(801, 1272, 960, 96),
        },
        Case {
            edge: DockPosition::Right,
            rect: Rect::new(78, 144, 64, 640),
            viewport: (150.0, 928.0),
            before: ((2335, 0), (225, 1392)),
            after: ((2272, 0), (288, 1392)),
            material: Rect::new(2452, 216, 96, 960),
        },
    ];
    for case in cases {
        let edge = case.edge;
        let report = PanelReport {
            rect: case.rect,
            edge,
            viewport: CssViewport {
                width: case.viewport.0,
                height: case.viewport.1,
            },
        };
        let mut facts = Facts {
            inputs: Some(acrylic_inputs()),
            handle: Some(DOCK),
            ..Facts::default()
        };
        let (origin, size) = case.before;
        assert!(!facts.panel_converted(converted(report, origin, size, 1.5)));
        let material = DockBackdrop {
            rect: case.material,
            ..backdrop_at_rest()
        };
        assert_eq!(facts.step().send, Some((DOCK, Some(material))), "{edge:?}");

        // The resize (`windows::layout::place`) converts the same report again.
        let (origin, size) = case.after;
        assert!(
            !facts.panel_converted(converted(report, origin, size, 1.5)),
            "{edge:?}: the material stays"
        );
        assert_eq!(facts.step().send, None, "{edge:?}: in the same place");
        assert_eq!(facts.panel.map(|p| p.rect), Some(case.material), "{edge:?}");
    }
}

#[test]
fn a_drag_and_the_drop_keep_the_material_under_the_panel() {
    let mut facts = shown();
    // The drag grows the window to the work area (1392 px) with its
    // bottom edge fixed: the report at rest, re-anchored, is 832 css
    // lower in the grown viewport, the same place on screen.
    let grown = ((0, 0), (2560, 1392));
    assert!(!facts.panel_converted(converted(report(), grown.0, grown.1, 1.5)));
    assert_eq!(facts.step().send, None, "the material stays put");

    // The page reports the panel in the grown viewport.
    let in_grown = PanelReport {
        rect: Rect::new(534, 848, 640, 64),
        viewport: CssViewport {
            width: 1707.0,
            height: 928.0,
        },
        ..report()
    };
    assert!(!facts.panel_converted(converted(in_grown, grown.0, grown.1, 1.5)));
    assert_eq!(facts.step().send, None);

    // The drop shrinks the window back before that report is replaced:
    // re-anchored to the short viewport, it is the panel at rest.
    assert!(!facts.panel_converted(converted(in_grown, ORIGIN, REST, 1.5)));
    assert_eq!(facts.step().send, None);
    assert_eq!(
        facts.panel.map(|p| p.rect),
        Some(backdrop_at_rest().rect),
        "where the panel rests"
    );

    // A right dock at 1x: grown to 1920 px with its right side fixed.
    let right = PanelReport {
        rect: Rect::new(78, 144, 64, 640),
        edge: DockPosition::Right,
        viewport: CssViewport {
            width: 150.0,
            height: 1040.0,
        },
    };
    let mut facts = Facts {
        inputs: Some(acrylic_inputs()),
        handle: Some(DOCK),
        ..Facts::default()
    };
    assert!(!facts.panel_converted(converted(right, (1770, 0), (150, 1040), 1.0)));
    let resting = DockBackdrop {
        rect: Rect::new(1848, 144, 64, 640),
        corner_radius: 22,
        ..backdrop_at_rest()
    };
    assert_eq!(facts.step().send, Some((DOCK, Some(resting))));
    assert!(!facts.panel_converted(converted(right, (0, 0), (1920, 1040), 1.0)));
    assert_eq!(facts.step().send, None, "grown: the same place on screen");
    let in_grown = PanelReport {
        rect: Rect::new(1848, 144, 64, 640),
        viewport: CssViewport {
            width: 1920.0,
            height: 1040.0,
        },
        ..right
    };
    assert!(!facts.panel_converted(converted(in_grown, (1770, 0), (150, 1040), 1.0)));
    assert_eq!(facts.step().send, None, "dropped: still the same place");
}

#[test]
fn no_material_while_the_viewport_is_unknown() {
    let mut facts = shown();
    let blind = Some(PanelConversion {
        origin: ORIGIN,
        css: report().now(None),
        scale: 1.5,
        viewport: None,
    });
    assert!(facts.panel_converted(blind));
    assert_eq!(facts.step().send, Some((DOCK, None)));
    assert!(!facts.panel_converted(at_rest()));
    assert_eq!(facts.step().send, Some((DOCK, Some(backdrop_at_rest()))));
}

#[test]
fn the_material_goes_with_the_slide_out_and_comes_back_when_the_reveal_is_due() {
    let mut facts = shown();
    assert_eq!(facts.auto_hide_reported(true), None);
    assert_eq!(facts.step().send, Some((DOCK, None)), "out: at once");

    let reveal = facts.auto_hide_reported(false);
    assert!(reveal.is_some());
    assert_eq!(facts.step().send, None, "back: not before the delay");
    facts.reveal_due(reveal.unwrap_or_default());
    assert_eq!(facts.step().send, Some((DOCK, Some(backdrop_at_rest()))));
}

#[test]
fn a_reveal_overtaken_by_a_newer_report_leaves_the_panel_slid_out() {
    let mut facts = shown();
    facts.auto_hide_reported(true);
    assert_eq!(facts.step().send, Some((DOCK, None)));

    // Out, back and out again within the delay: the first reveal is stale.
    let stale = facts.auto_hide_reported(false).unwrap_or_default();
    assert_eq!(facts.auto_hide_reported(true), None);
    facts.reveal_due(stale);
    assert!(facts.slid_out);
    assert_eq!(facts.step().send, None, "still slid out: nothing to send");

    // Back twice within the delay: only the second reveal acts.
    let first = facts.auto_hide_reported(false).unwrap_or_default();
    let second = facts.auto_hide_reported(false).unwrap_or_default();
    facts.reveal_due(first);
    assert_eq!(facts.step().send, None, "overtaken by the second report");
    facts.reveal_due(second);
    assert_eq!(facts.step().send, Some((DOCK, Some(backdrop_at_rest()))));
}

#[test]
fn a_settings_or_theme_change_while_slid_out_shows_nothing_until_the_reveal() {
    let mut facts = shown();
    facts.auto_hide_reported(true);
    assert_eq!(facts.step().send, Some((DOCK, None)));

    facts.inputs = Some(Inputs {
        backdrop: Backdrop::Mica,
        radius_css: 8,
        ..acrylic_inputs()
    });
    assert_eq!(
        facts.step(),
        Step {
            log: Some(Effective::Native(BackdropMaterial::Mica)),
            send: None,
        },
        "the new look is logged; the panel is still out"
    );
    facts.theme.dark = false;
    assert_eq!(facts.step(), Step::default());

    let reveal = facts.auto_hide_reported(false).unwrap_or_default();
    facts.reveal_due(reveal);
    let mica = DockBackdrop {
        corner_radius: 12,
        material: BackdropMaterial::Mica,
        ..backdrop_at_rest()
    };
    assert_eq!(facts.step().send, Some((DOCK, Some(mica))));
}

#[test]
fn a_contrast_theme_or_the_css_look_takes_the_material_away_and_logs_why() {
    let mut facts = shown();
    facts.theme.high_contrast = true;
    assert_eq!(
        facts.step(),
        Step {
            log: Some(Effective::ContrastTheme(BackdropMaterial::Acrylic)),
            send: Some((DOCK, None)),
        }
    );
    facts.theme.high_contrast = false;
    assert_eq!(
        facts.step(),
        Step {
            log: Some(Effective::Native(BackdropMaterial::Acrylic)),
            send: Some((DOCK, Some(backdrop_at_rest()))),
        }
    );
    facts.inputs = Some(Inputs {
        backdrop: Backdrop::Css,
        ..acrylic_inputs()
    });
    assert_eq!(
        facts.step(),
        Step {
            log: Some(Effective::Css),
            send: Some((DOCK, None)),
        }
    );
}
