use stem_core::PlatformEvent;

use super::*;
use crate::windows::util;

#[test]
fn e_notfound_is_hresult_from_win32_error_not_found() {
    assert_eq!(
        util::hresult_of(ERROR_NOT_FOUND).0 as u32,
        rules::E_NOTFOUND
    );
}

#[test]
fn this_hosts_volume_reads_and_the_thread_stops() {
    // The real thread, reading only (it sets nothing): whatever this
    // machine has, the first report arrives at once, a live read
    // agrees with it, and the thread stops within its bound. A machine
    // without an output device (a CI runner) reports `None`.
    let (tx, rx) = bounded::<PlatformEvent>(16);
    let sink: EventSink = std::sync::Arc::new(move |ev| {
        let _ = tx.try_send(ev);
    });
    let thread = VolumeThread::start(sink).unwrap();
    let first = match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(PlatformEvent::VolumeChanged(reading)) => reading,
        other => panic!("no first reading: {other:?}"),
    };
    assert!(first.is_none_or(|v| v.percent <= 100));
    let live = thread.handle().read().unwrap();
    assert_eq!(live.is_some(), first.is_some());
    let started = Instant::now();
    thread.stop();
    assert!(started.elapsed() < STOP_TIMEOUT);
}
