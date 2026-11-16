//! The Recycle Bin poll thread.
//!
//! Contract:
//! - A second thread (`dock-recycle-bin`, [`RecycleBinPoller`]) polls
//!   `recycle_bin::query` every [`RECYCLE_BIN_POLL`] and emits
//!   `RecycleBinChanged` on change. `SHQueryRecycleBinW(NULL)` walks every
//!   drive and can block for seconds (network or sleeping drives), so the
//!   events thread never calls it: a slow query must not delay window events.

use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TryRecvError, bounded};
use stem_core::{PlatformEvent, RecycleBinState};

use super::STOP_TIMEOUT;
use crate::windows::recycle_bin;
use crate::{EventSink, PlatformError, PlatformResult};

/// How often the Recycle Bin is queried (on its own thread).
pub const RECYCLE_BIN_POLL: Duration = Duration::from_secs(5);

/// The `dock-recycle-bin` thread: queries the Recycle Bin every
/// [`RECYCLE_BIN_POLL`], starting right away (the initial emission), and
/// calls the sink with `RecycleBinChanged` only when the state differs from
/// the last one it emitted. Stopped by dropping the stop sender; `stop`
/// also waits (bounded) for the thread, which may be inside a slow
/// `SHQueryRecycleBinW` — in that case it is detached and finishes on its
/// own without emitting (it checks for the stop before every emission).
#[derive(Debug)]
pub(super) struct RecycleBinPoller {
    /// Dropping it wakes the thread and tells it to stop.
    stop: Option<Sender<()>>,
    join: Option<JoinHandle<()>>,
    done: Receiver<()>,
}

impl RecycleBinPoller {
    pub(super) fn start(sink: EventSink) -> PlatformResult<Self> {
        let (stop_tx, stop_rx) = bounded::<()>(0);
        let (done_tx, done_rx) = bounded::<()>(1);
        let join = std::thread::Builder::new()
            .name("dock-recycle-bin".into())
            .spawn(move || {
                recycle_bin_poll_loop(&sink, &stop_rx);
                let _ = done_tx.send(());
            })
            .map_err(|e| PlatformError::Other(format!("spawn recycle-bin thread: {e}")))?;
        Ok(Self {
            stop: Some(stop_tx),
            join: Some(join),
            done: done_rx,
        })
    }

    pub(super) fn stop(mut self) {
        self.stop.take();
        match self.done.recv_timeout(STOP_TIMEOUT) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                if let Some(join) = self.join.take()
                    && join.join().is_err()
                {
                    log::error!("recycle-bin poll thread panicked");
                }
                log::debug!("recycle-bin poll thread stopped");
            }
            Err(RecvTimeoutError::Timeout) => {
                log::warn!(
                    "recycle-bin poll thread did not stop within {STOP_TIMEOUT:?} (slow \
                     SHQueryRecycleBinW?); detaching it"
                );
            }
        }
    }
}

/// Poll until the stop sender is dropped. Errors are logged once per streak.
fn recycle_bin_poll_loop(sink: &EventSink, stop: &Receiver<()>) {
    let mut last: Option<RecycleBinState> = None;
    let mut error_logged = false;
    loop {
        match recycle_bin::query() {
            Ok(state) => {
                error_logged = false;
                if last != Some(state) {
                    // The query may have taken a while: do not emit into a
                    // platform that was stopped meanwhile.
                    if stop.try_recv() == Err(TryRecvError::Disconnected) {
                        break;
                    }
                    last = Some(state);
                    sink(PlatformEvent::RecycleBinChanged(state));
                }
            }
            Err(e) => {
                if !error_logged {
                    log::debug!("recycle bin unavailable: {e}");
                    error_logged = true;
                }
            }
        }
        // The sleep; a dropped sender ends it early.
        if stop.recv_timeout(RECYCLE_BIN_POLL) != Err(RecvTimeoutError::Timeout) {
            break;
        }
    }
    log::debug!("recycle-bin poll loop exiting");
}
