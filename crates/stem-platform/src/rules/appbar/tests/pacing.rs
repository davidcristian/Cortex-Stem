use pretty_assertions::assert_eq;

use super::*;

#[test]
fn settle_applies_the_latest_request_once_it_has_waited() {
    let a = request(MONITOR, DockPosition::Bottom, 72);
    let b = request(MONITOR, DockPosition::Bottom, 80);
    let mut settle: Settle<u32> = Settle::default();
    assert_eq!(settle.next_due(), None);
    settle.push(1, a, 1_000);
    assert_eq!(settle.next_due(), Some(1_250));
    assert_eq!(settle.take_due(1_249), vec![]);
    // A newer request replaces it and waits again.
    settle.push(1, b, 1_200);
    assert_eq!(settle.next_due(), Some(1_450));
    assert_eq!(settle.take_due(1_300), vec![]);
    // The same request again keeps its time.
    settle.push(1, b, 1_400);
    assert_eq!(settle.next_due(), Some(1_450));
    assert_eq!(settle.take_due(1_450), vec![(1, b)]);
    assert_eq!(settle.next_due(), None);
    assert_eq!(settle.take_due(5_000), vec![]);
}

#[test]
fn settle_keeps_one_request_per_window_and_cancels_on_a_release() {
    let a = request(MONITOR, DockPosition::Bottom, 72);
    let b = request(LEFT_MONITOR, DockPosition::Right, 108);
    let mut settle: Settle<u32> = Settle::default();
    settle.push(2, b, 100);
    settle.push(1, a, 0);
    assert_eq!(settle.next_due(), Some(250));
    assert_eq!(settle.take_due(400), vec![(1, a), (2, b)]);
    settle.push(1, a, 1_000);
    settle.push(2, b, 1_000);
    assert!(settle.cancel(1));
    assert!(!settle.cancel(1));
    assert_eq!(settle.take_due(2_000), vec![(2, b)]);
}

#[test]
fn the_requery_guard_lets_a_burst_through_then_pauses() {
    let mut guard = RequeryGuard::default();
    for i in 0..RequeryGuard::BURST {
        assert_eq!(guard.allow(100 + u64::from(i)), Requery::Go, "{i}");
    }
    assert_eq!(guard.allow(200), Requery::Pause);
    assert_eq!(guard.allow(300), Requery::Paused);
    assert_eq!(
        guard.allow(200 + RequeryGuard::PAUSE_MS - 1),
        Requery::Paused
    );
    // After the pause, counting starts again.
    assert_eq!(guard.allow(200 + RequeryGuard::PAUSE_MS), Requery::Go);
}

#[test]
fn the_requery_guard_forgets_repositions_older_than_its_window() {
    let mut guard = RequeryGuard::default();
    let mut now = 0;
    // One reposition every 300 ms never pauses: at most 7 per window.
    for _ in 0..40 {
        assert_eq!(guard.allow(now), Requery::Go, "at {now} ms");
        now += 300;
    }
}

#[test]
fn the_requery_guard_owes_a_query_for_a_reposition_it_refused() {
    let mut guard = RequeryGuard::default();
    assert_eq!(guard.owed_at(), None);
    for i in 0..RequeryGuard::BURST {
        assert_eq!(guard.allow(100 + u64::from(i)), Requery::Go, "{i}");
    }
    assert_eq!(guard.owed_at(), None, "nothing refused yet");
    assert_eq!(guard.allow(200), Requery::Pause);
    let end = 200 + RequeryGuard::PAUSE_MS;
    assert_eq!(guard.owed_at(), Some(end));
    // The band seen back in place: nothing owed, the pause goes on.
    guard.caught_up();
    assert_eq!(guard.owed_at(), None);
    assert_eq!(guard.allow(300), Requery::Paused);
    assert_eq!(guard.owed_at(), Some(end));
    assert!(!guard.take_owed(end - 1));
    assert!(guard.take_owed(end));
    assert_eq!(guard.owed_at(), None);
    assert!(!guard.take_owed(end + 1));
    // The query owed goes through: the pause is over.
    assert_eq!(guard.allow(end), Requery::Go);
    assert_eq!(guard.owed_at(), None);
}

#[test]
fn retry_waits_longer_after_each_failure_in_a_row_up_to_a_cap() {
    let mut retry = Retry::default();
    assert_eq!(retry.due(), None);
    assert_eq!(retry.failed(0), 1_000);
    assert_eq!(retry.due(), Some(1_000));
    assert!(!retry.take_due(999));
    assert!(retry.take_due(1_000));
    assert_eq!(retry.due(), None);
    assert!(!retry.take_due(5_000));
    assert_eq!(retry.failed(1_000), 5_000);
    assert_eq!(retry.failed(6_000), 30_000);
    assert_eq!(retry.failed(36_000), 60_000);
    assert_eq!(retry.failed(96_000), 60_000);
    assert_eq!(retry.due(), Some(156_000));
    assert_eq!(retry.failures(), 5);
    retry.reset();
    assert_eq!(retry, Retry::default());
    assert_eq!(retry.failed(0), 1_000);
}

#[test]
fn settle_says_which_windows_have_a_request_waiting() {
    let mut settle: Settle<u32> = Settle::default();
    assert!(!settle.is_waiting(1));
    settle.push(1, request(MONITOR, DockPosition::Bottom, 72), 0);
    assert!(settle.is_waiting(1));
    assert!(!settle.is_waiting(2));
    settle.take_due(250);
    assert!(!settle.is_waiting(1));
}
