//! The handle on the `stem-appbar` thread, and the bounded waits for it
//! at the quit.

use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded, unbounded};
use windows::Win32::UI::WindowsAndMessaging::{MSG, PM_NOREMOVE, PM_QS_SENDMESSAGE, PeekMessageW};

use super::subclass::taskbar_created_message;
use super::thread::thread_main;
use super::{Command, SENDER};
use crate::{AppBarRequest, AppBarSink, NativeWindowHandle, PlatformError, PlatformResult};

/// How often [`release_all`] looks for sent messages while it waits.
const PUMP_INTERVAL: Duration = Duration::from_millis(5);
/// How long [`AppBarThread::stop`] waits for the thread to end.
const STOP_TIMEOUT: Duration = Duration::from_millis(200);

/// Handle to the running `stem-appbar` thread. Dropping it without `stop`
/// asks the thread to end but does not wait for it; prefer `stop`.
pub struct AppBarThread {
    sender: Sender<Command>,
    /// Disconnects when the thread has ended.
    ended: Receiver<()>,
    join: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for AppBarThread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppBarThread")
            .field("running", &self.join.is_some())
            .finish()
    }
}

/// A clonable handle on the thread's queue, so a caller can wait on the
/// thread without holding the platform's lock ([`release_all`]).
#[derive(Clone)]
pub struct AppBarQueue(Sender<Command>);

impl std::fmt::Debug for AppBarQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AppBarQueue")
    }
}

impl AppBarThread {
    pub fn start() -> PlatformResult<Self> {
        let (sender, receiver) = unbounded::<Command>();
        let (ended_tx, ended_rx) = bounded::<()>(0);
        let join = std::thread::Builder::new()
            .name("stem-appbar".into())
            .spawn(move || {
                // Dropped when the thread ends, which disconnects `ended`.
                let _ended = ended_tx;
                thread_main(&receiver);
            })
            .map_err(|e| PlatformError::Other(format!("could not start the AppBar thread: {e}")))?;
        let created = taskbar_created_message();
        *SENDER.lock() = Some(sender.clone());
        log::debug!("AppBar thread started (TaskbarCreated = {created:#x})");
        Ok(Self {
            sender,
            ended: ended_rx,
            join: Some(join),
        })
    }

    /// Queue the reservation `request` wants for `handle` (`None`: give the
    /// band back), to be reported through `sink`. Returns at once.
    pub fn set(
        &self,
        handle: NativeWindowHandle,
        request: Option<AppBarRequest>,
        sink: AppBarSink,
    ) {
        let command = Command::Set {
            handle: handle.0,
            request,
            sink,
        };
        if self.sender.send(command).is_err() {
            log::warn!(
                "AppBar thread has ended; the request for {:#x} is dropped",
                handle.0
            );
        }
    }

    pub fn queue(&self) -> AppBarQueue {
        AppBarQueue(self.sender.clone())
    }

    /// Give every band back and end the thread, waiting at most
    /// [`STOP_TIMEOUT`] (handling sent messages meanwhile, as
    /// [`release_all`] does); a thread held by a hung Explorer is detached.
    pub fn stop(mut self) {
        *SENDER.lock() = None;
        if self.sender.send(Command::Stop).is_err() {
            log::debug!("AppBar thread already ended");
        }
        let deadline = Instant::now() + STOP_TIMEOUT;
        loop {
            pump_sent_messages();
            let now = Instant::now();
            if now >= deadline {
                log::warn!("AppBar thread did not stop within {STOP_TIMEOUT:?}; detaching it");
                return;
            }
            match self.ended.recv_timeout((deadline - now).min(PUMP_INTERVAL)) {
                Err(RecvTimeoutError::Timeout) => {}
                Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        if let Some(join) = self.join.take()
            && join.join().is_err()
        {
            log::error!("AppBar thread panicked");
        }
        log::debug!("AppBar thread stopped");
    }
}

impl Drop for AppBarThread {
    fn drop(&mut self) {
        if self.join.is_some() {
            *SENDER.lock() = None;
            let _ = self.sender.send(Command::Stop);
        }
    }
}

/// Give back every band the thread holds and wait at most `timeout` for the
/// shell to take them, dispatching the messages other threads send to the
/// calling thread's windows meanwhile ([`pump_sent_messages`]): on the main
/// thread at the quit, the shell may send the remaining bars'
/// `ABN_POSCHANGED` to the dock windows while it removes one, and must not
/// wait on a thread that waits on it. `Timeout` when the shell did not
/// finish in time.
pub fn release_all(queue: &AppBarQueue, timeout: Duration) -> PlatformResult<()> {
    let (done_tx, done_rx) = bounded::<()>(1);
    queue
        .0
        .send(Command::ReleaseAll(done_tx))
        .map_err(|_| PlatformError::Other("the AppBar thread has ended".into()))?;
    let deadline = Instant::now() + timeout;
    loop {
        pump_sent_messages();
        let now = Instant::now();
        if now >= deadline {
            log::warn!(
                "AppBar: the shell did not take the bands back within {timeout:?} (Explorer \
                 busy or hung); they are released if it answers before the process ends"
            );
            return Err(PlatformError::Timeout);
        }
        match done_rx.recv_timeout((deadline - now).min(PUMP_INTERVAL)) {
            Ok(()) => return Ok(()),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(PlatformError::Other(
                    "the AppBar thread ended without an answer".into(),
                ));
            }
        }
    }
}

/// Dispatch the messages other threads have sent to this thread's windows,
/// and nothing else: posted messages stay queued for the thread's own loop.
pub(super) fn pump_sent_messages() {
    let mut msg = MSG::default();
    // SAFETY: `msg` is a valid out-pointer for the call. `PM_NOREMOVE |
    // PM_QS_SENDMESSAGE` makes the call dispatch pending sent (nonqueued)
    // messages to this thread's window procedures and remove nothing from
    // the posted queue; on a thread without windows it does nothing.
    let _ = unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE | PM_QS_SENDMESSAGE) };
}
