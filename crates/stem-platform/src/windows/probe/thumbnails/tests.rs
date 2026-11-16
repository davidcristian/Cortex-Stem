use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use stem_core::{RawWindow, WindowId};
use windows::Win32::Foundation::{RECT, SIZE};
use windows::Win32::Graphics::Dwm::{DWM_TNP_RECTSOURCE, DWM_TNP_VISIBLE};
use windows::core::HRESULT;

use super::external::pick_external;
use super::facts::cloak_text;
use super::geometry::{crop_to_frame, fit, height, layout, thumbnail_props, width};
use super::input::{Drained, drain_early};
use super::plans::PLANS;
use super::quit::Quit;
use super::summary::table;
use super::timing::{Call, timing_lines};
use super::worker::{WorkerEnd, worker_result};
use super::{STORE_FRAME_CLASS, TASK_MANAGER_CLASS};

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
    RECT {
        left,
        top,
        right,
        bottom,
    }
}

#[test]
fn fit_keeps_the_aspect_ratio_and_centres() {
    let slot = rect(10, 20, 330, 220);
    assert_eq!(fit(1600, 1000, slot), rect(10, 20, 330, 220));
    assert_eq!(fit(1000, 1000, slot), rect(70, 20, 270, 220));
    assert_eq!(fit(1920, 600, slot), rect(10, 70, 330, 170));
}

#[test]
fn fit_never_scales_up_and_survives_bad_input() {
    let slot = rect(0, 0, 320, 200);
    assert_eq!(fit(100, 50, slot), rect(110, 75, 210, 125));
    assert_eq!(fit(0, 50, slot), slot);
    assert_eq!(fit(100, 50, rect(5, 5, 5, 5)), rect(5, 5, 5, 5));
}

#[test]
fn crop_removes_the_invisible_borders() {
    let window = rect(100, 100, 1114, 708);
    let frame = rect(107, 100, 1107, 701);
    let source = SIZE { cx: 1014, cy: 608 };
    assert_eq!(
        crop_to_frame(window, frame, source, false),
        Ok(rect(7, 0, 1007, 601))
    );
}

#[test]
fn crop_refuses_iconic_empty_and_outside_frames() {
    let source = SIZE { cx: 1014, cy: 608 };
    let iconic = rect(-32000, -32000, -31840, -31972);
    assert!(crop_to_frame(iconic, iconic, source, false).is_err());
    let window = rect(100, 100, 1114, 708);
    assert!(crop_to_frame(window, window, source, true).is_err());
    assert!(crop_to_frame(window, rect(107, 100, 107, 701), source, false).is_err());
    let small = SIZE { cx: 676, cy: 405 };
    assert!(crop_to_frame(window, rect(107, 100, 1107, 701), small, false).is_err());
}

#[test]
fn props_set_the_source_rectangle_only_when_cropping() {
    let destination = rect(1, 2, 3, 4);
    let plain = thumbnail_props(destination, None);
    let flags = plain.dwFlags;
    assert_eq!(flags & DWM_TNP_RECTSOURCE, 0);
    assert_ne!(flags & DWM_TNP_VISIBLE, 0);
    let cropped = thumbnail_props(destination, Some(rect(7, 0, 100, 50)));
    let flags = cropped.dwFlags;
    let source = cropped.rcSource;
    assert_ne!(flags & DWM_TNP_RECTSOURCE, 0);
    assert_eq!(source, rect(7, 0, 100, 50));
}

fn inside(inner: RECT, outer: RECT) -> bool {
    inner.left >= outer.left
        && inner.top >= outer.top
        && inner.right <= outer.right
        && inner.bottom <= outer.bottom
}

fn overlap(a: RECT, b: RECT) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

fn check_layout(scale: f64, work: SIZE) {
    let grid = layout(scale, work, PLANS.len());
    assert_eq!(grid.slots.len(), PLANS.len());
    assert!(grid.client.cx <= work.cx && grid.client.cy <= work.cy);
    let client = rect(0, 0, grid.client.cx, grid.client.cy);
    for (index, slot) in grid.slots.iter().enumerate() {
        assert!(inside(slot.label, client) && inside(slot.image, client));
        assert!(slot.label.bottom <= slot.image.top);
        assert_eq!(width(slot.image) * 10 / 16, height(slot.image));
        for other in &grid.slots[index + 1..] {
            assert!(!overlap(slot.image, other.image));
            assert!(!overlap(slot.label, other.image));
        }
    }
}

#[test]
fn layout_fits_common_work_areas() {
    check_layout(1.0, SIZE { cx: 1920, cy: 1040 });
    check_layout(1.25, SIZE { cx: 1920, cy: 1032 });
    check_layout(1.5, SIZE { cx: 1920, cy: 1032 });
    check_layout(2.0, SIZE { cx: 3840, cy: 2064 });
    check_layout(1.0, SIZE { cx: 1366, cy: 728 });
    let grid = layout(1.0, SIZE { cx: 1920, cy: 1040 }, PLANS.len());
    assert_eq!(grid.client, SIZE { cx: 1512, cy: 512 });
}

#[test]
fn table_aligns_columns() {
    let rows = vec![
        vec!["1".to_owned(), "normal".to_owned()],
        vec!["10".to_owned(), "x".to_owned()],
    ];
    assert_eq!(
        table(&["#", "source"], &rows),
        vec![
            "#  | source".to_owned(),
            "---+-------".to_owned(),
            "1  | normal".to_owned(),
            "10 | x".to_owned(),
        ]
    );
}

#[test]
fn cloak_values_are_named() {
    assert_eq!(cloak_text(None), "cloak ?");
    assert_eq!(cloak_text(Some(0)), "not cloaked");
    assert_eq!(cloak_text(Some(1)), "cloaked 0x1 (app)");
    assert_eq!(cloak_text(Some(6)), "cloaked 0x6 (shell+inherited)");
}

fn window(id: u64, class: &str, title: &str) -> RawWindow {
    RawWindow {
        id: WindowId(id),
        title: title.to_owned(),
        class_name: class.to_owned(),
        process_id: 100 + id as u32,
        exe_path: None,
        app_user_model_id: None,
        is_visible: true,
        is_minimized: false,
        is_cloaked: false,
        on_current_desktop: Some(true),
        monitor: None,
        rect: None,
        icon_key: None,
    }
}

#[test]
fn picks_task_manager_on_this_desktop_first() {
    let mut elsewhere = window(1, TASK_MANAGER_CLASS, "Task Manager");
    elsewhere.on_current_desktop = Some(false);
    let here = window(2, TASK_MANAGER_CLASS, "Task Manager");
    let found = pick_external(&[elsewhere, here]);
    assert_eq!(found.elevated.map(|w| w.id), Some(WindowId(2)));
    assert_eq!(found.other_desktop.map(|w| w.id), Some(WindowId(1)));
    assert!(found.note.is_none());
}

#[test]
fn picks_calculator_before_other_store_frames() {
    let mut frame = window(1, STORE_FRAME_CLASS, "Settings");
    frame.is_cloaked = true;
    let mut calculator = window(2, STORE_FRAME_CLASS, "Rechner");
    calculator.app_user_model_id = Some("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App".into());
    calculator.is_minimized = true;
    let found = pick_external(&[frame.clone(), calculator]);
    assert_eq!(found.store_app.map(|w| w.id), Some(WindowId(2)));
    let found = pick_external(&[frame, window(3, STORE_FRAME_CLASS, "Photos")]);
    assert_eq!(found.store_app.map(|w| w.id), Some(WindowId(1)));
}

#[test]
fn picks_notepad_on_another_desktop_first() {
    let mut terminal = window(1, "CASCADIA_HOSTING_WINDOW_CLASS", "Terminal");
    terminal.on_current_desktop = Some(false);
    let mut notepad = window(2, "Notepad", "Untitled - Notepad");
    notepad.on_current_desktop = Some(false);
    notepad.exe_path = Some(PathBuf::from(r"C:\Windows\System32\notepad.exe"));
    let found = pick_external(&[terminal, notepad, window(3, "Notepad", "here")]);
    assert_eq!(found.other_desktop.map(|w| w.id), Some(WindowId(2)));
    assert!(found.elevated.is_none() && found.store_app.is_none());
}

#[test]
fn says_when_the_shell_gave_no_desktop_facts() {
    let mut unknown = window(1, "Notepad", "a");
    unknown.on_current_desktop = None;
    let found = pick_external(&[unknown]);
    assert!(found.other_desktop.is_none());
    assert!(found.note.is_some());
}

#[test]
fn timing_lines_flag_slow_calls() {
    let call = |api, micros| Call {
        api,
        hr: HRESULT(0),
        micros,
    };
    let lines = timing_lines(&[
        call("DwmRegisterThumbnail", 100.0),
        call("DwmRegisterThumbnail", 20_000.0),
        call("DwmUnregisterThumbnail", 10.0),
    ]);
    assert_eq!(lines.len(), 3);
    assert!(lines[0].contains("2 calls") && lines[0].contains("slowest 20000 us"));
    assert_eq!(lines[2], "A6: 1 of 3 DWM calls took longer than 16 ms");
}

#[test]
fn worker_result_names_the_step_that_did_not_return() {
    let returned = vec!["worker: DwmUpdateThumbnailProperties(slot 1) ok in 30 us".to_owned()];
    let waited = Duration::from_millis(5_004);
    assert_eq!(
        worker_result(WorkerEnd::Finished, &returned, None, waited),
        returned[0]
    );
    let stuck = worker_result(
        WorkerEnd::TimedOut,
        &returned,
        Some("DwmUnregisterThumbnail(slot 2)"),
        waited,
    );
    assert!(
        stuck.starts_with("did not finish within 5004 ms: still in DwmUnregisterThumbnail(slot 2)")
    );
    assert!(
        stuck
            .ends_with("returned before: worker: DwmUpdateThumbnailProperties(slot 1) ok in 30 us")
    );
    let stopped = worker_result(
        WorkerEnd::Stopped,
        &[],
        Some("slot 2's registration"),
        waited,
    );
    assert_eq!(
        stopped,
        "stopped waiting after 5004 ms: still in slot 2's registration; returned before: nothing"
    );
    let vanished = worker_result(WorkerEnd::Vanished, &[], None, waited);
    assert!(vanished.contains("panicked) in its last message"));
}

#[test]
fn early_input_is_dropped_but_q_and_the_end_still_count() {
    let (sender, input) = mpsc::channel();
    assert_eq!(drain_early(&input), Drained::default());

    for line in ["", "a note"] {
        let _ = sender.send(Some(line.to_owned()));
    }
    assert_eq!(
        drain_early(&input),
        Drained {
            ignored: vec![String::new(), "a note".to_owned()],
            stop: None,
        }
    );

    for line in ["", "Q", "after q"] {
        let _ = sender.send(Some(line.to_owned()));
    }
    let drained = drain_early(&input);
    assert_eq!(drained.ignored, vec![String::new()]);
    assert_eq!(drained.stop, Some(Quit::Typed));

    let _ = sender.send(Some("after q".to_owned()));
    let _ = sender.send(None);
    drop(sender);
    let drained = drain_early(&input);
    assert_eq!(drained.stop, Some(Quit::StdinClosed));
    assert_eq!(drain_early(&input).stop, Some(Quit::StdinClosed));
}
