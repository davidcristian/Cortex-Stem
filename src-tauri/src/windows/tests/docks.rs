use super::*;
use crate::windows::create::show_new_dock_window;
use crate::windows::placement::DockContent;
use crate::windows::plan::{DockPlan, dock_label, dock_page, plan_docks, query_value};
use crate::windows::state::DockWindowState;

#[test]
fn dock_labels_keep_only_safe_characters_and_pages_carry_the_encoded_id() {
    let display3 = MonitorId(r"\\.\DISPLAY3".into());
    assert_eq!(dock_label(&display3), "dock-DISPLAY3");
    assert_eq!(
        dock_label(&MonitorId("mock-primary".into())),
        "dock-mock-primary"
    );
    assert_eq!(dock_label(&MonitorId("a b/c:d_e".into())), "dock-abcd_e");
    assert_eq!(dock_label(&MonitorId(String::new())), "dock-");
    assert_eq!(query_value(r"\\.\DISPLAY3"), "%5C%5C.%5CDISPLAY3");
    assert_eq!(query_value("mock-primary_1.~"), "mock-primary_1.~");
    assert_eq!(query_value("a b&c=d"), "a%20b%26c%3Dd");
    assert_eq!(query_value("é"), "%C3%A9");
    // The primary's page has no query string, whatever its monitor.
    assert_eq!(dock_page(DOCK_LABEL, Some(&display3)), "dock.html");
    assert_eq!(dock_page(DOCK_LABEL, None), "dock.html");
    assert_eq!(
        dock_page("dock-DISPLAY3", Some(&display3)),
        "dock.html?monitor=%5C%5C.%5CDISPLAY3"
    );
    assert_eq!(dock_page("dock-DISPLAY3", None), "dock.html");
}

#[test]
fn plan_docks_gives_the_primary_one_window_and_all_one_per_monitor() {
    let primary = monitor(r"\\.\DISPLAY2", true, 0, 1.0);
    let left = monitor(r"\\.\DISPLAY1", false, -1920, 1.5);
    let right = monitor(r"\\.\DISPLAY3", false, 2560, 1.0);
    let monitors = vec![left.clone(), primary.clone(), right.clone()];

    // The default: exactly today's one window, on the primary.
    assert_eq!(
        plan_docks(MonitorMode::Primary, &monitors),
        vec![DockPlan {
            label: "dock".into(),
            monitor: Some(primary.clone()),
        }]
    );
    // No monitor known yet: still the one window, unplaced.
    assert_eq!(
        plan_docks(MonitorMode::Primary, &[]),
        vec![DockPlan {
            label: "dock".into(),
            monitor: None,
        }]
    );
    assert_eq!(
        plan_docks(MonitorMode::All, &[]),
        vec![DockPlan {
            label: "dock".into(),
            monitor: None,
        }]
    );

    // All: the primary keeps `dock`; the others follow in list order.
    assert_eq!(
        plan_docks(MonitorMode::All, &monitors),
        vec![
            DockPlan {
                label: "dock".into(),
                monitor: Some(primary.clone()),
            },
            DockPlan {
                label: "dock-DISPLAY1".into(),
                monitor: Some(left.clone()),
            },
            DockPlan {
                label: "dock-DISPLAY3".into(),
                monitor: Some(right.clone()),
            },
        ]
    );
    // One monitor only: `All` is the same as `Primary`.
    assert_eq!(
        plan_docks(MonitorMode::All, std::slice::from_ref(&primary)),
        plan_docks(MonitorMode::Primary, std::slice::from_ref(&primary))
    );
}

#[test]
fn plan_docks_falls_back_to_the_first_monitor_and_skips_label_collisions() {
    // Nothing flagged primary: the first monitor is the primary's.
    let a = monitor(r"\\.\DISPLAY1", false, 0, 1.0);
    let b = monitor(r"\\.\DISPLAY2", false, 2560, 1.0);
    let plan = plan_docks(MonitorMode::All, &[a.clone(), b.clone()]);
    assert_eq!(plan[0].label, "dock");
    assert_eq!(plan[0].monitor, Some(a));
    assert_eq!(plan[1].label, "dock-DISPLAY2");
    assert_eq!(plan[1].monitor, Some(b));

    // Two ids that sanitise to the same label: the second is left out.
    let primary = monitor(r"\\.\DISPLAY1", true, 0, 1.0);
    let x = monitor(r"\\.\DISPLAY2", false, 2560, 1.0);
    let twin = monitor("DISPLAY2", false, 5120, 1.0);
    let plan = plan_docks(MonitorMode::All, &[primary, x.clone(), twin]);
    assert_eq!(plan.len(), 2);
    assert_eq!(plan[1].monitor, Some(x));
}

#[test]
fn dock_geometry_lists_the_primary_first() {
    let geometry = DockGeometry::default();
    for label in ["dock-DISPLAY3", "dock", "dock-DISPLAY1"] {
        geometry
            .docks
            .lock()
            .insert(label.to_owned(), DockWindowState::default());
    }
    assert_eq!(
        geometry.labels(),
        vec!["dock", "dock-DISPLAY1", "dock-DISPLAY3"]
    );
    assert!(geometry.dock("dock-DISPLAY3").is_some());
    assert!(geometry.dock("settings").is_none());
    let content = DockContent {
        width: 640.0,
        height: 96.0,
        edge: DockPosition::Bottom,
    };
    assert_eq!(
        geometry.update("dock", |dock| {
            dock.content = Some(content);
            1
        }),
        Some(1)
    );
    assert_eq!(geometry.update("nope", |_| 1), None);
    assert_eq!(geometry.dock("dock").unwrap().content, Some(content));
    assert_eq!(geometry.dock("dock").unwrap().edge, None);
}

#[test]
fn a_closing_dock_window_defers_a_reconcile_until_it_is_gone() {
    let geometry = DockGeometry::default();
    // Not closing: a reconcile goes ahead, and a `Destroyed` (the quit's)
    // asks for nothing.
    assert!(!geometry.defer_if_closing("dock-DISPLAY2"));
    assert_eq!(geometry.finish_closing("dock-DISPLAY2"), None);

    // Destroyed, `Destroyed` still to come, and nobody wanted it back.
    geometry.start_closing("dock-DISPLAY2");
    assert_eq!(geometry.finish_closing("dock-DISPLAY2"), Some(false));
    assert!(!geometry.defer_if_closing("dock-DISPLAY2"), "gone now");

    // A monitor that drops and comes back before the main thread caught
    // up: the second reconcile neither lays the dying window out nor
    // builds its label again; its `Destroyed` runs the reconcile again,
    // once.
    geometry.start_closing("dock-DISPLAY2");
    assert!(geometry.defer_if_closing("dock-DISPLAY2"));
    assert!(geometry.defer_if_closing("dock-DISPLAY2"));
    assert!(!geometry.defer_if_closing("dock-DISPLAY3"));
    assert_eq!(geometry.finish_closing("dock-DISPLAY2"), Some(true));
    assert_eq!(geometry.finish_closing("dock-DISPLAY2"), None);
    assert!(!geometry.defer_if_closing("dock-DISPLAY2"));
}

#[test]
fn a_new_secondary_dock_window_follows_the_primary_and_the_primary_always_shows() {
    // The primary's window is shown whatever is passed.
    for primary_visible in [None, Some(false), Some(true)] {
        assert!(show_new_dock_window(DOCK_LABEL, primary_visible));
    }
    // A secondary one only while the set is on screen.
    assert!(show_new_dock_window("dock-DISPLAY3", Some(true)));
    assert!(!show_new_dock_window("dock-DISPLAY3", Some(false)));
    assert!(!show_new_dock_window("dock-DISPLAY3", None));
}
