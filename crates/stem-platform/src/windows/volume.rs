//! The default output device's volume: a thread that
//! follows it and reports its changes.
//!
//! Contract:
//! - [`VolumeThread::start`] spawns `stem-volume`, which enters the
//!   process's multithreaded apartment (`util::ComApartment::mta`; the Core
//!   Audio interfaces need no message loop there and call back on threads
//!   of their own), creates an `IMMDeviceEnumerator` and registers an
//!   `IMMNotificationClient` with it, binds the default console render
//!   endpoint (`GetDefaultAudioEndpoint(eRender, eConsole)`, then
//!   `IMMDevice::Activate` for its `IAudioEndpointVolume`) and registers an
//!   `IAudioEndpointVolumeCallback` on that. It then reports the first
//!   reading (`PlatformEvent::VolumeChanged`; `None` without a device) and
//!   waits on one channel for the callbacks, the callers' requests and its
//!   two timers. It never polls a bound device: with nothing happening it
//!   sleeps in `recv` (without a device, one retry a minute, below).
//! - The callbacks do the minimum: post a message (`try_send` on a bounded
//!   channel; a full channel already holds a notification that reads the
//!   latest value) and return `S_OK`. `OnNotify` (the level or the mute)
//!   always posts; `OnDefaultDeviceChanged` only for `eRender` / `eConsole`
//!   (`rules::volume::is_console_render`); `OnDeviceStateChanged`,
//!   `OnDeviceAdded` and `OnDeviceRemoved` always (the default may have gone
//!   with the device); `OnPropertyValueChanged` never. A device callback
//!   also raises a shared flag before it posts, and the thread looks at the
//!   default device whenever it finds the flag raised after a message, so a
//!   device message dropped from a full queue (a slider drag during a slow
//!   COM call) is made up for by the next message the thread takes.
//! - A device message makes the thread ask for the default endpoint's id
//!   again and re-bind when it differs from the bound one (or nothing is
//!   bound): the old endpoint's callback is unregistered and its COM objects
//!   released first. A read that fails drops the binding and binds once
//!   more; when the fresh binding cannot be read either, the reading is
//!   `None` and the binding is dropped and tried again later like a failed
//!   bind (below).
//! - Reports are coalesced (`rules::volume::Throttle`): at most one per
//!   50 ms, the leading edge at once and a burst's trailing edge when its
//!   interval ends, read then, so the last value always arrives; and only a
//!   reading that differs from the last one reported goes out
//!   (`rules::volume::ChangeFilter`). The reading is
//!   `GetMasterVolumeLevelScalar` and `GetMute`, as a percent
//!   (`rules::volume::status_of`). Debug line per report: `volume:
//!   <reading> (<cause>)`.
//! - A failed bind is logged once per streak (`warn`, then `debug`) and
//!   yields `None`, and is always tried again on a timer
//!   (`rules::volume::retry_after_ms`): no device (`E_NOTFOUND`) every
//!   60 s, unless a device notification comes first; anything else (the
//!   audio service stopped at start or restarted under Stem, an enumerator
//!   that could not be created) after a growing delay, at most every 60 s.
//!   Every timed retry drops the enumerator and its notification client,
//!   which a restart of the audio service may have left dead (a dead
//!   registration never calls back), and builds everything again; a
//!   failure other than no device drops them at once
//!   (`rules::volume::drops_watch_at_once`). While a device is bound and
//!   reads there is no timer, so the thread never wakes on its own.
//! - [`VolumeHandle`] answers [`crate::Platform::volume_status`] (a live
//!   read on the thread), `set_volume` (`SetMasterVolumeLevelScalar`),
//!   `set_volume_muted` (`SetMute`) and `step_volume` (one wheel notch:
//!   a live read, `stem_core::engine::volume_step`, then the level and the
//!   mute it changes, all in one message, so the read and the sets cannot
//!   interleave with another request), each waiting at most
//!   [`REQUEST_TIMEOUT`] for the thread. A set or a step carries its
//!   caller's deadline: one the thread takes after it (the thread was
//!   busy) is not done late, since its caller has already given up, and is
//!   answered with `PlatformError::Timeout` (as is a caller that finds its
//!   reply channel closed after its deadline). One the thread started in
//!   time finishes even when its answer comes too late. The sets pass no
//!   event context, so their own notification is reported like any other.
//! - [`VolumeThread::stop`] asks the thread to quit and waits at most
//!   [`STOP_TIMEOUT`]; the thread unregisters the endpoint callback, then
//!   the notification client, releases every COM object and leaves the
//!   apartment, in that order (RAII: [`Endpoint`](endpoint::Endpoint),
//!   [`DeviceWatch`](endpoint::DeviceWatch), `ComApartment`). A thread
//!   stuck in a COM call is detached with a `warn` line.
//!
//! Assumes that Windows delivers the notifications for the volume keys,
//! the Quick Settings slider, a mute from the keyboard, a new default
//! device and unplugged headphones.

use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, SendTimeoutError, Sender, bounded};
use stem_core::{ScrollDirection, VolumeStatus};
use windows::Win32::Foundation::ERROR_NOT_FOUND;
use windows::Win32::Media::Audio::{eConsole, eRender};

use crate::rules::volume as rules;
use crate::{EventSink, PlatformError, PlatformResult};
use thread::thread_main;

mod binding;
mod callbacks;
mod control;
mod endpoint;
mod thread;

// The host-independent values in `rules::volume` mirror the SDK; prove them
// against it here, where the `windows` crate is available.
const _: () = {
    assert!(rules::E_RENDER == eRender.0);
    assert!(rules::E_CONSOLE == eConsole.0);
    // HRESULT_FROM_WIN32: FACILITY_WIN32 (7) and the failure bit.
    assert!(rules::E_NOTFOUND == 0x8007_0000 | ERROR_NOT_FOUND.0);
};

/// How long a caller waits for the thread to take a request and answer it.
pub const REQUEST_TIMEOUT: Duration = Duration::from_millis(250);
/// How long [`VolumeThread::stop`] waits for the thread to finish.
pub const STOP_TIMEOUT: Duration = Duration::from_secs(2);
/// Messages the channel holds; a callback that finds it full drops its own.
const QUEUE: usize = 256;

/// What the thread receives.
#[derive(Debug)]
enum Msg {
    /// `IAudioEndpointVolumeCallback::OnNotify`: the level or the mute
    /// changed.
    VolumeNotified,
    /// `IMMNotificationClient`: the default render device, or some device's
    /// state, changed.
    DevicesChanged(&'static str),
    /// A live reading for `volume_status`.
    Read(Sender<Option<VolumeStatus>>),
    /// `set_volume` with this percent, before this deadline.
    SetLevel(u8, Instant, Sender<PlatformResult<()>>),
    /// `set_volume_muted` with this value, before this deadline.
    SetMuted(bool, Instant, Sender<PlatformResult<()>>),
    /// `step_volume` in this direction, before this deadline.
    Step(
        ScrollDirection,
        Instant,
        Sender<PlatformResult<Option<VolumeStatus>>>,
    ),
    Stop,
}

/// Handle to the running thread. Dropping it without [`Self::stop`] asks the
/// thread to quit but does not wait for it.
#[derive(Debug)]
pub struct VolumeThread {
    tx: Sender<Msg>,
    join: Option<JoinHandle<()>>,
    done: Receiver<()>,
}

/// What callers use to reach the thread, cheap to clone, so the platform's
/// lock is not held while one waits.
#[derive(Debug, Clone)]
pub struct VolumeHandle {
    tx: Sender<Msg>,
}

impl VolumeThread {
    /// Spawn the thread; it binds the device and reports the first reading
    /// on its own, so this returns at once.
    pub fn start(sink: EventSink) -> PlatformResult<Self> {
        let (tx, rx) = bounded::<Msg>(QUEUE);
        let (done_tx, done_rx) = bounded::<()>(1);
        let thread_tx = tx.clone();
        let join = std::thread::Builder::new()
            .name("stem-volume".into())
            .spawn(move || {
                thread_main(&sink, thread_tx, &rx);
                let _ = done_tx.send(());
            })
            .map_err(|e| PlatformError::Other(format!("spawn volume thread: {e}")))?;
        log::info!("volume thread started");
        Ok(Self {
            tx,
            join: Some(join),
            done: done_rx,
        })
    }

    pub fn handle(&self) -> VolumeHandle {
        VolumeHandle {
            tx: self.tx.clone(),
        }
    }

    pub fn stop(mut self) {
        if self.tx.send_timeout(Msg::Stop, REQUEST_TIMEOUT).is_err() {
            log::warn!("volume thread did not take the stop request");
        }
        match self.done.recv_timeout(STOP_TIMEOUT) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                if let Some(join) = self.join.take()
                    && join.join().is_err()
                {
                    log::error!("volume thread panicked");
                }
                log::info!("volume thread stopped");
            }
            Err(RecvTimeoutError::Timeout) => {
                log::warn!("volume thread did not stop within {STOP_TIMEOUT:?}; detaching it");
                self.join.take();
            }
        }
    }
}

impl Drop for VolumeThread {
    fn drop(&mut self) {
        if self.join.is_some() {
            let _ = self.tx.send_timeout(Msg::Stop, REQUEST_TIMEOUT);
        }
    }
}

impl VolumeHandle {
    /// The reading now, `None` without a device or when it cannot be read.
    pub fn read(&self) -> PlatformResult<Option<VolumeStatus>> {
        self.request(|_, reply| Msg::Read(reply))
    }

    pub fn set_level(&self, percent: u8) -> PlatformResult<()> {
        self.request(|deadline, reply| Msg::SetLevel(percent, deadline, reply))?
    }

    pub fn set_muted(&self, muted: bool) -> PlatformResult<()> {
        self.request(|deadline, reply| Msg::SetMuted(muted, deadline, reply))?
    }

    /// One wheel notch over the volume item: the volume it set, or `None`
    /// when there was nothing to do (no device, the end of the range).
    pub fn step(&self, direction: ScrollDirection) -> PlatformResult<Option<VolumeStatus>> {
        self.request(|deadline, reply| Msg::Step(direction, deadline, reply))?
    }

    /// Send the request `make` builds around its deadline and a reply
    /// channel, and wait for the answer, both within [`REQUEST_TIMEOUT`].
    fn request<T>(&self, make: impl FnOnce(Instant, Sender<T>) -> Msg) -> PlatformResult<T> {
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        let (reply_tx, reply_rx) = bounded::<T>(1);
        self.tx
            .send_deadline(make(deadline, reply_tx), deadline)
            .map_err(|e| match e {
                SendTimeoutError::Timeout(_) => PlatformError::Timeout,
                SendTimeoutError::Disconnected(_) => {
                    PlatformError::Other("the volume thread is not running".into())
                }
            })?;
        reply_rx.recv_deadline(deadline).map_err(|e| match e {
            RecvTimeoutError::Timeout => PlatformError::Timeout,
            // The thread drops a reply channel it no longer answers on; past
            // the deadline that is a late request, not a stopped thread.
            RecvTimeoutError::Disconnected if Instant::now() >= deadline => PlatformError::Timeout,
            RecvTimeoutError::Disconnected => {
                PlatformError::Other("the volume thread stopped before it answered".into())
            }
        })
    }
}

#[cfg(test)]
mod tests;
