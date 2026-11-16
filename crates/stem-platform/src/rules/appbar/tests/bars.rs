use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;
use pretty_assertions::assert_eq;

use super::*;
use crate::{AppBarGrant, AppBarSink, NativeWindowHandle};

#[test]
fn steps_register_reposition_or_move_a_bar() {
    let bottom = request(MONITOR, DockPosition::Bottom, 72);
    let thicker = request(MONITOR, DockPosition::Bottom, 96);
    let left = request(MONITOR, DockPosition::Left, 72);
    let other_monitor = request(LEFT_MONITOR, DockPosition::Bottom, 72);
    assert_eq!(steps(None, None), &[] as &[Step]);
    assert_eq!(steps(None, Some(bottom)), &[Step::New, Step::Position]);
    assert_eq!(steps(Some(bottom), None), &[Step::Remove]);
    assert_eq!(steps(Some(bottom), Some(bottom)), &[] as &[Step]);
    assert_eq!(steps(Some(bottom), Some(thicker)), &[Step::Position]);
    assert_eq!(
        steps(Some(bottom), Some(left)),
        &[Step::Remove, Step::New, Step::Position]
    );
    assert_eq!(
        steps(Some(bottom), Some(other_monitor)),
        &[Step::Remove, Step::New, Step::Position]
    );
}

const H: isize = 0xA;
const H2: isize = 0xB;
const H3: isize = 0xC;
/// The bottom band [`query_rect`] proposes for 72 px on [`MONITOR`]; also
/// what the shell grants with the taskbar hidden.
const BOTTOM_ASK: Rect = Rect::new(0, 1008, 1920, 72);
/// The bottom band above the fake shell's 48 px taskbar.
const ABOVE_TASKBAR: Rect = Rect::new(0, 960, 1920, 72);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sent {
    New(isize),
    Remove(isize),
    Query(isize, Rect),
    SetPos(isize, Rect),
    Activate(isize, bool),
    PosChanged(isize),
}

/// A scripted shell on [`MONITOR`]: a taskbar `taskbar` px thick on the
/// bottom edge (0: hidden) that `ABM_QUERYPOS` moves a bottom band past,
/// as Explorer does; `ABM_NEW` refused `refusals` more times;
/// `ABM_SETPOS` answers from `set_answers` first, then grants what it is
/// asked for. Records every message.
#[derive(Debug)]
struct FakeShell {
    present: bool,
    refusals: u32,
    taskbar: i32,
    set_answers: VecDeque<Rect>,
    sent: Vec<Sent>,
}

impl FakeShell {
    fn new() -> Self {
        Self {
            present: true,
            refusals: 0,
            taskbar: 48,
            set_answers: VecDeque::new(),
            sent: Vec::new(),
        }
    }

    /// The messages sent since the last call.
    fn take(&mut self) -> Vec<Sent> {
        std::mem::take(&mut self.sent)
    }
}

impl AppBarShell for FakeShell {
    fn present(&mut self) -> bool {
        self.present
    }

    fn register(&mut self, handle: isize) -> bool {
        self.sent.push(Sent::New(handle));
        if self.refusals > 0 {
            self.refusals -= 1;
            return false;
        }
        true
    }

    fn remove(&mut self, handle: isize) {
        self.sent.push(Sent::Remove(handle));
    }

    fn query(&mut self, handle: isize, edge: DockPosition, rect: Rect) -> Rect {
        self.sent.push(Sent::Query(handle, rect));
        let taskbar_top = MONITOR.bottom() - self.taskbar;
        if edge == DockPosition::Bottom && self.taskbar > 0 && rect.bottom() > taskbar_top {
            let top = rect.y.min(taskbar_top);
            return Rect::new(rect.x, top, rect.width, taskbar_top - top);
        }
        rect
    }

    fn set_pos(&mut self, handle: isize, _edge: DockPosition, rect: Rect) -> Rect {
        self.sent.push(Sent::SetPos(handle, rect));
        self.set_answers.pop_front().unwrap_or(rect)
    }

    fn activate(&mut self, handle: isize, active: bool) {
        self.sent.push(Sent::Activate(handle, active));
    }

    fn window_pos_changed(&mut self, handle: isize) {
        self.sent.push(Sent::PosChanged(handle));
    }
}

type Reports = Arc<Mutex<Vec<(NativeWindowHandle, Option<AppBarGrant>)>>>;

fn recorder() -> (AppBarSink, Reports) {
    let reports: Reports = Arc::default();
    let seen = Arc::clone(&reports);
    let sink: AppBarSink = Arc::new(move |handle, grant| seen.lock().push((handle, grant)));
    (sink, reports)
}

/// The reports since the last call.
fn drain(reports: &Reports) -> Vec<(NativeWindowHandle, Option<AppBarGrant>)> {
    std::mem::take(&mut *reports.lock())
}

fn bottom() -> AppBarRequest {
    request(MONITOR, DockPosition::Bottom, 72)
}

fn reported(handle: isize, request: AppBarRequest, rect: Rect) -> Vec<ReportOf> {
    vec![(
        NativeWindowHandle(handle),
        Some(AppBarGrant { request, rect }),
    )]
}

fn reported_none(handle: isize) -> Vec<ReportOf> {
    vec![(NativeWindowHandle(handle), None)]
}

type ReportOf = (NativeWindowHandle, Option<AppBarGrant>);

/// [`H`]'s bar on the bottom edge above the fake shell's taskbar,
/// registered at 250 ms, with nothing sent or reported left over.
fn registered() -> (FakeShell, Bars, AppBarSink, Reports) {
    let mut shell = FakeShell::new();
    let (sink, reports) = recorder();
    let mut bars = Bars::default();
    bars.set(&mut shell, H, Some(bottom()), Arc::clone(&sink), 0);
    bars.tick(&mut shell, 250);
    assert_eq!(shell.take().len(), 3);
    assert_eq!(drain(&reports), reported(H, bottom(), ABOVE_TASKBAR));
    (shell, bars, sink, reports)
}

#[test]
fn a_request_settles_then_registers_on_the_bounds_and_reports_the_band() {
    let mut shell = FakeShell::new();
    let (sink, reports) = recorder();
    let mut bars = Bars::default();
    bars.set(&mut shell, H, Some(bottom()), sink, 0);
    assert_eq!(shell.take(), vec![]);
    assert_eq!(bars.next_due(), Some(250));
    bars.tick(&mut shell, 249);
    assert_eq!(shell.take(), vec![]);
    bars.tick(&mut shell, 250);
    assert_eq!(
        shell.take(),
        vec![
            Sent::New(H),
            Sent::Query(H, BOTTOM_ASK),
            Sent::SetPos(H, ABOVE_TASKBAR)
        ]
    );
    assert_eq!(drain(&reports), reported(H, bottom(), ABOVE_TASKBAR));
    assert_eq!(bars.next_due(), None);
}

#[test]
fn a_request_that_changes_nothing_reports_the_band_again() {
    let (mut shell, mut bars, sink, reports) = registered();
    // The window asks for the top edge and comes back before the settle
    // ends; its caller dropped the band meanwhile and must learn it again.
    let top = request(MONITOR, DockPosition::Top, 72);
    bars.set(&mut shell, H, Some(top), Arc::clone(&sink), 1_000);
    bars.set(&mut shell, H, Some(bottom()), sink, 1_100);
    bars.tick(&mut shell, 1_350);
    assert_eq!(shell.take(), vec![]);
    assert_eq!(drain(&reports), reported(H, bottom(), ABOVE_TASKBAR));
}

#[test]
fn a_release_is_sent_at_once_cancels_what_waits_and_reports_nothing() {
    let (mut shell, mut bars, sink, reports) = registered();
    let thicker = request(MONITOR, DockPosition::Bottom, 96);
    bars.set(&mut shell, H, Some(thicker), Arc::clone(&sink), 1_000);
    bars.set(&mut shell, H, None, Arc::clone(&sink), 1_010);
    assert_eq!(shell.take(), vec![Sent::Remove(H)]);
    assert_eq!(bars.next_due(), None);
    bars.tick(&mut shell, 5_000);
    assert_eq!(shell.take(), vec![]);
    assert_eq!(drain(&reports), vec![]);
    // Released again: nothing to give back; the shell's notifications
    // move nothing.
    bars.set(&mut shell, H, None, sink, 6_000);
    bars.notify(&mut shell, H, Notification::PosChanged, 6_100);
    assert_eq!(shell.take(), vec![]);
    assert_eq!(drain(&reports), vec![]);
}

#[test]
fn another_edge_gives_the_old_band_back_first_and_another_thickness_only_moves_it() {
    let (mut shell, mut bars, sink, reports) = registered();
    let top = request(MONITOR, DockPosition::Top, 72);
    let top_band = Rect::new(0, 0, 1920, 72);
    bars.set(&mut shell, H, Some(top), Arc::clone(&sink), 1_000);
    bars.tick(&mut shell, 1_250);
    assert_eq!(
        shell.take(),
        vec![
            Sent::Remove(H),
            Sent::New(H),
            Sent::Query(H, top_band),
            Sent::SetPos(H, top_band)
        ]
    );
    assert_eq!(drain(&reports), reported(H, top, top_band));
    let thicker = request(MONITOR, DockPosition::Top, 96);
    let thicker_band = Rect::new(0, 0, 1920, 96);
    bars.set(&mut shell, H, Some(thicker), sink, 2_000);
    bars.tick(&mut shell, 2_250);
    assert_eq!(
        shell.take(),
        vec![Sent::Query(H, thicker_band), Sent::SetPos(H, thicker_band)]
    );
    assert_eq!(drain(&reports), reported(H, thicker, thicker_band));
}

#[test]
fn a_band_the_shell_answers_badly_is_given_up_and_asked_for_again_with_a_backoff() {
    let mut shell = FakeShell::new();
    // Three answers on the next monitor (a display change racing the
    // request), then good ones.
    let elsewhere = Rect::new(1920, 960, 1920, 72);
    shell.set_answers = VecDeque::from([elsewhere; 3]);
    let (sink, reports) = recorder();
    let mut bars = Bars::default();
    bars.set(&mut shell, H, Some(bottom()), sink, 0);
    bars.tick(&mut shell, 250);
    let given_up = vec![
        Sent::New(H),
        Sent::Query(H, BOTTOM_ASK),
        Sent::SetPos(H, ABOVE_TASKBAR),
        Sent::Remove(H),
    ];
    assert_eq!(shell.take(), given_up);
    assert_eq!(drain(&reports), reported_none(H));
    assert_eq!(bars.next_due(), Some(1_250));
    bars.tick(&mut shell, 1_249);
    assert_eq!(shell.take(), vec![]);
    bars.tick(&mut shell, 1_250);
    assert_eq!(shell.take(), given_up);
    // Still no band: nothing new to report.
    assert_eq!(drain(&reports), vec![]);
    assert_eq!(bars.next_due(), Some(6_250));
    bars.tick(&mut shell, 6_250);
    assert_eq!(shell.take(), given_up);
    assert_eq!(bars.next_due(), Some(36_250));
    bars.tick(&mut shell, 36_250);
    assert_eq!(
        shell.take(),
        vec![
            Sent::New(H),
            Sent::Query(H, BOTTOM_ASK),
            Sent::SetPos(H, ABOVE_TASKBAR)
        ]
    );
    assert_eq!(drain(&reports), reported(H, bottom(), ABOVE_TASKBAR));
    assert_eq!(bars.next_due(), None);
}

#[test]
fn a_band_lost_to_a_bad_answer_after_a_notification_is_asked_for_again() {
    let (mut shell, mut bars, _sink, reports) = registered();
    // The taskbar hides itself and the shell answers the new band badly.
    shell.taskbar = 0;
    shell.set_answers = VecDeque::from([Rect::new(0, 0, 0, 0)]);
    bars.notify(&mut shell, H, Notification::PosChanged, 1_000);
    assert_eq!(
        shell.take(),
        vec![
            Sent::Query(H, BOTTOM_ASK),
            Sent::SetPos(H, BOTTOM_ASK),
            Sent::Remove(H)
        ]
    );
    assert_eq!(drain(&reports), reported_none(H));
    assert_eq!(bars.next_due(), Some(2_000));
    // Without a band the shell's notifications move nothing.
    bars.notify(&mut shell, H, Notification::PosChanged, 1_500);
    assert_eq!(shell.take(), vec![]);
    bars.tick(&mut shell, 2_000);
    assert_eq!(
        shell.take(),
        vec![
            Sent::New(H),
            Sent::Query(H, BOTTOM_ASK),
            Sent::SetPos(H, BOTTOM_ASK)
        ]
    );
    assert_eq!(drain(&reports), reported(H, bottom(), BOTTOM_ASK));
    assert_eq!(bars.next_due(), None);
}

#[test]
fn a_refused_registration_is_tried_once_more_at_once_then_after_a_delay() {
    let mut shell = FakeShell::new();
    let (sink, reports) = recorder();
    let mut bars = Bars::default();
    // Refused once (a stale registration): removed and registered.
    shell.refusals = 1;
    bars.set(&mut shell, H2, Some(bottom()), Arc::clone(&sink), 0);
    bars.tick(&mut shell, 250);
    assert_eq!(
        shell.take(),
        vec![
            Sent::New(H2),
            Sent::Remove(H2),
            Sent::New(H2),
            Sent::Query(H2, BOTTOM_ASK),
            Sent::SetPos(H2, ABOVE_TASKBAR)
        ]
    );
    assert_eq!(drain(&reports), reported(H2, bottom(), ABOVE_TASKBAR));
    // Refused twice: given up, and tried again later.
    shell.refusals = 2;
    bars.set(&mut shell, H, Some(bottom()), sink, 1_000);
    bars.tick(&mut shell, 1_250);
    assert_eq!(
        shell.take(),
        vec![Sent::New(H), Sent::Remove(H), Sent::New(H)]
    );
    assert_eq!(drain(&reports), reported_none(H));
    assert_eq!(bars.next_due(), Some(2_250));
    bars.tick(&mut shell, 2_250);
    assert_eq!(
        shell.take(),
        vec![
            Sent::New(H),
            Sent::Query(H, BOTTOM_ASK),
            Sent::SetPos(H, ABOVE_TASKBAR)
        ]
    );
    assert_eq!(drain(&reports), reported(H, bottom(), ABOVE_TASKBAR));
}

#[test]
fn without_the_shell_nothing_is_sent_and_taskbar_created_registers_at_once() {
    let mut shell = FakeShell::new();
    shell.present = false;
    let (sink, reports) = recorder();
    let mut bars = Bars::default();
    bars.set(&mut shell, H, Some(bottom()), Arc::clone(&sink), 0);
    bars.tick(&mut shell, 250);
    assert_eq!(shell.take(), vec![]);
    assert_eq!(drain(&reports), reported_none(H));
    assert_eq!(bars.next_due(), Some(1_250));
    bars.tick(&mut shell, 1_250);
    assert_eq!(shell.take(), vec![]);
    assert_eq!(drain(&reports), vec![]);
    assert_eq!(bars.next_due(), Some(6_250));
    // Explorer is back: `TaskbarCreated` registers at once.
    shell.present = true;
    bars.shell_restarted(&mut shell, H, 2_000);
    assert_eq!(
        shell.take(),
        vec![
            Sent::New(H),
            Sent::Query(H, BOTTOM_ASK),
            Sent::SetPos(H, ABOVE_TASKBAR)
        ]
    );
    assert_eq!(drain(&reports), reported(H, bottom(), ABOVE_TASKBAR));
    assert_eq!(bars.next_due(), None);
    // For a bar that wants nothing it sends nothing.
    bars.set(&mut shell, H, None, sink, 3_000);
    assert_eq!(shell.take(), vec![Sent::Remove(H)]);
    bars.shell_restarted(&mut shell, H, 3_100);
    assert_eq!(shell.take(), vec![]);
}

#[test]
fn a_bar_the_shell_left_unpositioned_is_positioned_again() {
    let (mut shell, mut bars, sink, reports) = registered();
    // Explorer is gone when the icon size changes.
    shell.present = false;
    let thicker = request(MONITOR, DockPosition::Bottom, 96);
    bars.set(&mut shell, H, Some(thicker), sink, 1_000);
    bars.tick(&mut shell, 1_250);
    assert_eq!(shell.take(), vec![]);
    assert_eq!(drain(&reports), reported_none(H));
    // Still registered: the next attempt positions only.
    shell.present = true;
    bars.tick(&mut shell, 2_250);
    assert_eq!(
        shell.take(),
        vec![
            Sent::Query(H, Rect::new(0, 984, 1920, 96)),
            Sent::SetPos(H, Rect::new(0, 936, 1920, 96))
        ]
    );
    assert_eq!(
        drain(&reports),
        reported(H, thicker, Rect::new(0, 936, 1920, 96))
    );
    assert_eq!(bars.next_due(), None);
}

#[test]
fn a_notification_sets_the_band_again_only_when_it_moved() {
    let (mut shell, mut bars, _sink, reports) = registered();
    bars.notify(&mut shell, H, Notification::PosChanged, 1_000);
    assert_eq!(shell.take(), vec![Sent::Query(H, BOTTOM_ASK)]);
    assert_eq!(drain(&reports), vec![]);
    // The taskbar hides itself: the band moves down to the screen's edge.
    shell.taskbar = 0;
    bars.notify(&mut shell, H, Notification::StateChange, 2_000);
    assert_eq!(
        shell.take(),
        vec![Sent::Query(H, BOTTOM_ASK), Sent::SetPos(H, BOTTOM_ASK)]
    );
    assert_eq!(drain(&reports), reported(H, bottom(), BOTTOM_ASK));
    // Fullscreen, tiling and unknown codes send nothing.
    for notification in [
        Notification::FullScreenApp(true),
        Notification::FullScreenApp(false),
        Notification::WindowArrange(true),
        Notification::Other(99),
    ] {
        bars.notify(&mut shell, H, notification, 3_000);
    }
    assert_eq!(shell.take(), vec![]);
    assert_eq!(bars.next_due(), None);
}

/// Drives [`RequeryGuard::BURST`] repositions of `H`'s band (the fake
/// taskbar shown and hidden in turn), then one more that the guard
/// refuses, with the taskbar hidden; returns when the pause ends.
fn burst_into_a_pause(shell: &mut FakeShell, bars: &mut Bars, reports: &Reports) -> u64 {
    let mut now = 1_000;
    for i in 0..RequeryGuard::BURST {
        shell.taskbar = if i % 2 == 0 { 0 } else { 48 };
        bars.notify(shell, H, Notification::PosChanged, now);
        assert_eq!(shell.take().len(), 2, "query and set, {i}");
        now += 10;
    }
    drain(reports);
    // The last band asked for is above the taskbar; the next is refused.
    shell.taskbar = 0;
    bars.notify(shell, H, Notification::PosChanged, now);
    assert_eq!(shell.take(), vec![Sent::Query(H, BOTTOM_ASK)]);
    now + RequeryGuard::PAUSE_MS
}

#[test]
fn after_the_requery_pause_the_bar_follows_a_band_moved_during_it() {
    let (mut shell, mut bars, _sink, reports) = registered();
    let end = burst_into_a_pause(&mut shell, &mut bars, &reports);
    assert_eq!(bars.next_due(), Some(end));
    // During the pause the band is queried, never set.
    bars.notify(&mut shell, H, Notification::PosChanged, end - 5_000);
    assert_eq!(shell.take(), vec![Sent::Query(H, BOTTOM_ASK)]);
    bars.tick(&mut shell, end - 1);
    assert_eq!(shell.take(), vec![]);
    // At its end the bar follows the band, without a notification.
    bars.tick(&mut shell, end);
    assert_eq!(
        shell.take(),
        vec![Sent::Query(H, BOTTOM_ASK), Sent::SetPos(H, BOTTOM_ASK)]
    );
    assert_eq!(drain(&reports), reported(H, bottom(), BOTTOM_ASK));
    assert_eq!(bars.next_due(), None);
}

#[test]
fn a_band_back_in_place_by_the_end_of_the_pause_is_not_set_again() {
    let (mut shell, mut bars, _sink, reports) = registered();
    let end = burst_into_a_pause(&mut shell, &mut bars, &reports);
    // The taskbar is back where the bar last asked for its band.
    shell.taskbar = 48;
    bars.tick(&mut shell, end);
    assert_eq!(shell.take(), vec![Sent::Query(H, BOTTOM_ASK)]);
    assert_eq!(drain(&reports), vec![]);
    assert_eq!(bars.next_due(), None);
    // Seen back in place by a notification during the pause: nothing is
    // owed at all.
    let (mut shell, mut bars, _sink, reports) = registered();
    let end = burst_into_a_pause(&mut shell, &mut bars, &reports);
    shell.taskbar = 48;
    bars.notify(&mut shell, H, Notification::PosChanged, end - 5_000);
    assert_eq!(shell.take(), vec![Sent::Query(H, BOTTOM_ASK)]);
    assert_eq!(bars.next_due(), None);
}

#[test]
fn activation_and_moves_reach_the_shell_only_for_registered_bars() {
    let mut shell = FakeShell::new();
    let mut bars = Bars::default();
    bars.activate(&mut shell, H, true);
    bars.window_pos_changed(&mut shell, H);
    assert_eq!(shell.take(), vec![]);
    let (mut shell, mut bars, _sink, _reports) = registered();
    bars.activate(&mut shell, H, true);
    bars.activate(&mut shell, H, false);
    bars.window_pos_changed(&mut shell, H);
    bars.activate(&mut shell, H2, true);
    assert_eq!(
        shell.take(),
        vec![
            Sent::Activate(H, true),
            Sent::Activate(H, false),
            Sent::PosChanged(H)
        ]
    );
}

#[test]
fn a_destroyed_window_or_the_quit_gives_the_bands_back_and_reports_nothing() {
    let (mut shell, mut bars, sink, reports) = registered();
    let left = request(MONITOR, DockPosition::Left, 72);
    bars.set(&mut shell, H2, Some(left), Arc::clone(&sink), 1_000);
    bars.tick(&mut shell, 1_250);
    assert_eq!(shell.take().len(), 3);
    drain(&reports);
    // A third window's request still waits.
    bars.set(&mut shell, H3, Some(bottom()), sink, 2_000);
    bars.destroyed(&mut shell, H);
    assert_eq!(shell.take(), vec![Sent::Remove(H)]);
    bars.notify(&mut shell, H, Notification::PosChanged, 2_010);
    bars.shell_restarted(&mut shell, H, 2_020);
    assert_eq!(shell.take(), vec![]);
    bars.release_all(&mut shell);
    assert_eq!(shell.take(), vec![Sent::Remove(H2)]);
    assert_eq!(bars.next_due(), None);
    bars.tick(&mut shell, 10_000);
    assert_eq!(shell.take(), vec![]);
    assert_eq!(drain(&reports), vec![]);
}
