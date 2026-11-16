//! The thread: its loop, its state ([`Follower`]) and its reports.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use stem_core::PlatformEvent;

use super::Msg;
use super::endpoint::{DeviceWatch, Endpoint};
use crate::rules::volume::{ChangeFilter, Throttle};
use crate::windows::util;
use crate::{EventSink, PlatformError};

/// Everything the thread owns. Field order is drop order: the endpoint's
/// callback goes before the notification client, both before the apartment
/// (which `thread_main` holds outside).
pub(super) struct Follower<'a> {
    pub(super) endpoint: Option<Endpoint>,
    pub(super) watch: Option<DeviceWatch>,
    sink: &'a EventSink,
    pub(super) tx: Sender<Msg>,
    /// Raised by the device callbacks before they post (see the `volume`
    /// module docs); lowered by [`Follower::devices_changed`].
    pub(super) devices_flag: Arc<AtomicBool>,
    epoch: Instant,
    throttle: Throttle,
    filter: ChangeFilter,
    /// Failures in a row (a bind, or a read of a fresh binding) since the
    /// last good reading, for the retry delay and the log.
    pub(super) failures: u32,
    /// The last reading failed (logged at `warn` once per streak).
    pub(super) read_failing: bool,
    /// When the next bind is to be tried (ms since `epoch`), if one is.
    pub(super) retry_at: Option<u64>,
}

pub(super) fn thread_main(sink: &EventSink, tx: Sender<Msg>, rx: &Receiver<Msg>) {
    let _com = util::ComApartment::mta();
    let mut follower = Follower {
        endpoint: None,
        watch: None,
        sink,
        tx,
        devices_flag: Arc::new(AtomicBool::new(false)),
        epoch: Instant::now(),
        throttle: Throttle::new(),
        filter: ChangeFilter::default(),
        failures: 0,
        read_failing: false,
        retry_at: None,
    };
    follower.follow_default("start");
    follower.changed("start");
    loop {
        let msg = match follower.next_deadline() {
            Some(deadline) => rx.recv_deadline(deadline),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match msg {
            Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Ok(Msg::VolumeNotified) => follower.changed("OnNotify"),
            Ok(Msg::DevicesChanged(cause)) => follower.devices_changed(cause),
            Ok(Msg::Read(reply)) => {
                let _ = reply.send(follower.read());
            }
            Ok(Msg::SetLevel(percent, deadline, reply)) => {
                let answer = if too_late(deadline, "set_volume") {
                    Err(PlatformError::Timeout)
                } else {
                    follower.set_level(percent)
                };
                let _ = reply.send(answer);
            }
            Ok(Msg::SetMuted(muted, deadline, reply)) => {
                let answer = if too_late(deadline, "set_volume_muted") {
                    Err(PlatformError::Timeout)
                } else {
                    follower.set_muted(muted)
                };
                let _ = reply.send(answer);
            }
            Ok(Msg::Step(direction, deadline, reply)) => {
                let answer = if too_late(deadline, "step_volume") {
                    Err(PlatformError::Timeout)
                } else {
                    follower.step(direction)
                };
                let _ = reply.send(answer);
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        // A device message the full queue dropped: its flag is still up.
        if follower.devices_flag.load(Ordering::Acquire) {
            follower.devices_changed("a device callback whose message was dropped");
        }
        follower.on_time();
    }
    // Unregister and release in order before the apartment goes.
    drop(follower);
}

/// Whether a request the thread takes now came after its caller's
/// `deadline` (the caller has given up with a timeout): then it is not
/// done but answered with [`PlatformError::Timeout`], which a caller woken
/// just after its deadline reads as the timeout it is, with a debug line.
fn too_late(deadline: Instant, what: &str) -> bool {
    let late = Instant::now() > deadline;
    if late {
        log::debug!("volume: {what} reached the thread after its caller gave up; not done");
    }
    late
}

impl Follower<'_> {
    pub(super) fn now_ms(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn at(&self, ms: u64) -> Instant {
        self.epoch + Duration::from_millis(ms)
    }

    /// The nearest of the pending report and the pending retry.
    fn next_deadline(&self) -> Option<Instant> {
        [self.throttle.deadline(), self.retry_at]
            .into_iter()
            .flatten()
            .min()
            .map(|ms| self.at(ms))
    }

    /// Run whatever is due now.
    fn on_time(&mut self) {
        let now = self.now_ms();
        if self.retry_at.is_some_and(|at| now >= at) {
            self.retry_at = None;
            // Start from a fresh enumerator and client: one a restart of the
            // audio service left dead would never call back. The endpoint's
            // callback goes before the client (there is no binding while a
            // retry is pending; `None` is only the drop order made sure).
            self.endpoint = None;
            self.watch = None;
            self.follow_default("retry");
            self.changed("retry");
        }
        if self.throttle.take_due(self.now_ms()) {
            self.report("coalesced");
        }
    }

    /// A device callback fired: lower its flag, follow the default device
    /// again and report what it reads. The flag goes down before the look,
    /// so a callback that fires meanwhile raises it again and is not lost.
    fn devices_changed(&mut self, cause: &str) {
        self.devices_flag.store(false, Ordering::Release);
        self.follow_default(cause);
        self.changed(cause);
    }

    /// Something may have changed: report now or when the interval ends.
    fn changed(&mut self, cause: &str) {
        if self.throttle.notify(self.now_ms()) {
            self.report(cause);
        }
    }

    /// Read now and report the reading if it differs from the last one.
    fn report(&mut self, cause: &str) {
        let reading = self.read();
        if self.filter.observe(reading) {
            self.throttle.reported(self.now_ms());
            log::debug!("volume: {reading:?} ({cause})");
            (self.sink)(PlatformEvent::VolumeChanged(reading));
        }
    }
}
