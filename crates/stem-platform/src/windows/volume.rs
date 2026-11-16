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
//!   two timers. It never polls: with nothing happening it sleeps in
//!   `recv`.
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
//!   yields `None`. No device (`E_NOTFOUND`) waits for a device
//!   notification; anything else is tried again after a growing delay, at
//!   most every 60 s (`rules::volume::retry_after_ms`): the audio service
//!   stopped at start, an enumerator that could not be created.
//! - [`VolumeHandle`] answers [`crate::Platform::volume_status`] (a live
//!   read on the thread), `set_volume` (`SetMasterVolumeLevelScalar`) and
//!   `set_volume_muted` (`SetMute`), each waiting at most
//!   [`REQUEST_TIMEOUT`] for the thread. The sets pass no event context, so
//!   their own notification is reported like any other.
//! - [`VolumeThread::stop`] asks the thread to quit and waits at most
//!   [`STOP_TIMEOUT`]; the thread unregisters the endpoint callback, then
//!   the notification client, releases every COM object and leaves the
//!   apartment, in that order (RAII: [`Endpoint`], [`DeviceWatch`],
//!   `ComApartment`). A thread stuck in a COM call is detached with a
//!   `warn` line.
//!
//! Assumes that Windows delivers the notifications for the volume keys,
//! the Quick Settings slider, a mute from the keyboard, a new default
//! device and unplugged headphones.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, SendTimeoutError, Sender, bounded};
use stem_core::{PlatformEvent, VolumeStatus};
use windows::Win32::Foundation::{ERROR_NOT_FOUND, PROPERTYKEY};
use windows::Win32::Media::Audio::Endpoints::{
    IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl,
};
use windows::Win32::Media::Audio::{
    AUDIO_VOLUME_NOTIFICATION_DATA, DEVICE_STATE, EDataFlow, ERole, IMMDeviceEnumerator,
    IMMNotificationClient, IMMNotificationClient_Impl, MMDeviceEnumerator, eConsole, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::core::{PCWSTR, implement};

use super::util;
use crate::rules::volume::{self as rules, BindFailure, ChangeFilter, Throttle};
use crate::{EventSink, PlatformError, PlatformResult};

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
    /// `set_volume` with this percent.
    SetLevel(u8, Sender<PlatformResult<()>>),
    /// `set_volume_muted` with this value.
    SetMuted(bool, Sender<PlatformResult<()>>),
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
        self.request(Msg::Read)
    }

    pub fn set_level(&self, percent: u8) -> PlatformResult<()> {
        self.request(|reply| Msg::SetLevel(percent, reply))?
    }

    pub fn set_muted(&self, muted: bool) -> PlatformResult<()> {
        self.request(|reply| Msg::SetMuted(muted, reply))?
    }

    /// Send the request `make` builds around a reply channel and wait for
    /// the answer, both within [`REQUEST_TIMEOUT`].
    fn request<T>(&self, make: impl FnOnce(Sender<T>) -> Msg) -> PlatformResult<T> {
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        let (reply_tx, reply_rx) = bounded::<T>(1);
        self.tx
            .send_deadline(make(reply_tx), deadline)
            .map_err(|e| match e {
                SendTimeoutError::Timeout(_) => PlatformError::Timeout,
                SendTimeoutError::Disconnected(_) => {
                    PlatformError::Other("the volume thread is not running".into())
                }
            })?;
        reply_rx.recv_deadline(deadline).map_err(|e| match e {
            RecvTimeoutError::Timeout => PlatformError::Timeout,
            RecvTimeoutError::Disconnected => {
                PlatformError::Other("the volume thread stopped before it answered".into())
            }
        })
    }
}

// ---------------------------------------------------------------------------
// The COM callbacks: post and return.
// ---------------------------------------------------------------------------

/// Post `msg` without waiting; a full queue already holds a message that
/// makes the thread read the latest value.
fn post(tx: &Sender<Msg>, msg: Msg) {
    // Full: a queued message reads the value anyway. Disconnected: the
    // thread is gone and nobody is listening.
    let _ = tx.try_send(msg);
}

/// The endpoint's `IAudioEndpointVolumeCallback`.
#[implement(IAudioEndpointVolumeCallback)]
struct VolumeCallback {
    tx: Sender<Msg>,
}

impl IAudioEndpointVolumeCallback_Impl for VolumeCallback_Impl {
    fn OnNotify(&self, _data: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> windows::core::Result<()> {
        post(&self.tx, Msg::VolumeNotified);
        Ok(())
    }
}

/// The enumerator's `IMMNotificationClient`.
#[implement(IMMNotificationClient)]
struct DeviceCallback {
    tx: Sender<Msg>,
    /// Raised before every post, so a post the full queue drops is not
    /// lost: the thread checks it after each message.
    devices_changed: Arc<AtomicBool>,
}

impl DeviceCallback {
    fn devices_changed(&self, cause: &'static str) {
        self.devices_changed.store(true, Ordering::Release);
        post(&self.tx, Msg::DevicesChanged(cause));
    }
}

impl IMMNotificationClient_Impl for DeviceCallback_Impl {
    fn OnDeviceStateChanged(
        &self,
        _device_id: &PCWSTR,
        _new_state: DEVICE_STATE,
    ) -> windows::core::Result<()> {
        self.devices_changed("OnDeviceStateChanged");
        Ok(())
    }

    fn OnDeviceAdded(&self, _device_id: &PCWSTR) -> windows::core::Result<()> {
        self.devices_changed("OnDeviceAdded");
        Ok(())
    }

    fn OnDeviceRemoved(&self, _device_id: &PCWSTR) -> windows::core::Result<()> {
        self.devices_changed("OnDeviceRemoved");
        Ok(())
    }

    fn OnDefaultDeviceChanged(
        &self,
        flow: EDataFlow,
        role: ERole,
        _default_device_id: &PCWSTR,
    ) -> windows::core::Result<()> {
        if rules::is_console_render(flow.0, role.0) {
            self.devices_changed("OnDefaultDeviceChanged");
        }
        Ok(())
    }

    fn OnPropertyValueChanged(
        &self,
        _device_id: &PCWSTR,
        _key: &PROPERTYKEY,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// The thread's COM objects, released in order.
// ---------------------------------------------------------------------------

/// The enumerator with the notification client registered on it;
/// unregistered on drop.
struct DeviceWatch {
    enumerator: IMMDeviceEnumerator,
    client: IMMNotificationClient,
}

impl DeviceWatch {
    fn new(tx: &Sender<Msg>, devices_changed: &Arc<AtomicBool>) -> PlatformResult<Self> {
        // SAFETY: in-process activation of the documented MMDeviceEnumerator
        // class on a thread in the MTA; the result is owned by us.
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }?;
        let client: IMMNotificationClient = DeviceCallback {
            tx: tx.clone(),
            devices_changed: Arc::clone(devices_changed),
        }
        .into();
        // SAFETY: `client` is a live COM object that outlives the
        // registration: `Drop` unregisters it before it is released.
        unsafe { enumerator.RegisterEndpointNotificationCallback(&client) }?;
        Ok(Self { enumerator, client })
    }
}

impl Drop for DeviceWatch {
    fn drop(&mut self) {
        // SAFETY: the client was registered on this enumerator in `new` and
        // is unregistered exactly once, here.
        if let Err(e) = unsafe {
            self.enumerator
                .UnregisterEndpointNotificationCallback(&self.client)
        } {
            log::debug!("UnregisterEndpointNotificationCallback failed: {e}");
        }
    }
}

/// The bound default endpoint: its id, its volume control and the callback
/// registered on it; unregistered on drop.
struct Endpoint {
    id: String,
    volume: IAudioEndpointVolume,
    callback: IAudioEndpointVolumeCallback,
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        // SAFETY: the callback was registered on this control in `bind` and
        // is unregistered exactly once, here.
        if let Err(e) = unsafe { self.volume.UnregisterControlChangeNotify(&self.callback) } {
            log::debug!("UnregisterControlChangeNotify failed: {e}");
        }
    }
}

/// Why a bind failed, with the error for the log.
struct BindError {
    failure: BindFailure,
    error: PlatformError,
}

impl BindError {
    fn from_com(e: &windows::core::Error) -> Self {
        let code = e.code().0 as u32;
        Self {
            failure: if rules::is_no_device(code) {
                BindFailure::NoDevice
            } else {
                BindFailure::Other
            },
            error: e.clone().into(),
        }
    }

    fn other(error: PlatformError) -> Self {
        Self {
            failure: BindFailure::Other,
            error,
        }
    }
}

/// The default console render endpoint's id, or why there is none.
fn default_endpoint_id(
    enumerator: &IMMDeviceEnumerator,
) -> Result<(windows::Win32::Media::Audio::IMMDevice, String), BindError> {
    // SAFETY: a plain COM call on a live enumerator; the device is owned.
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }
        .map_err(|e| BindError::from_com(&e))?;
    // SAFETY: a plain COM call on a live device; the string it allocates is
    // owned by the guard and freed once.
    let id = unsafe { device.GetId() }.map_err(|e| BindError::from_com(&e))?;
    let id = util::CoTaskWideString::new(id)
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();
    Ok((device, id))
}

/// Bind the default console render endpoint and register `tx`'s callback.
fn bind(enumerator: &IMMDeviceEnumerator, tx: &Sender<Msg>) -> Result<Endpoint, BindError> {
    let (device, id) = default_endpoint_id(enumerator)?;
    // SAFETY: a plain COM call on a live device; no activation parameters;
    // the interface it returns is owned.
    let volume: IAudioEndpointVolume =
        unsafe { device.Activate(CLSCTX_ALL, None) }.map_err(|e| BindError::from_com(&e))?;
    let callback: IAudioEndpointVolumeCallback = VolumeCallback { tx: tx.clone() }.into();
    // SAFETY: `callback` is a live COM object that outlives the
    // registration: `Endpoint`'s drop unregisters it before it is released.
    unsafe { volume.RegisterControlChangeNotify(&callback) }
        .map_err(|e| BindError::from_com(&e))?;
    Ok(Endpoint {
        id,
        volume,
        callback,
    })
}

/// One reading of a bound endpoint.
fn read_endpoint(endpoint: &Endpoint) -> PlatformResult<VolumeStatus> {
    // SAFETY: a plain COM call on a live control.
    let scalar = unsafe { endpoint.volume.GetMasterVolumeLevelScalar() }?;
    // SAFETY: a plain COM call on a live control.
    let muted = unsafe { endpoint.volume.GetMute() }?;
    Ok(rules::status_of(scalar, muted.as_bool()))
}

// ---------------------------------------------------------------------------
// The thread.
// ---------------------------------------------------------------------------

/// Everything the thread owns. Field order is drop order: the endpoint's
/// callback goes before the notification client, both before the apartment
/// (which `thread_main` holds outside).
struct Follower<'a> {
    endpoint: Option<Endpoint>,
    watch: Option<DeviceWatch>,
    sink: &'a EventSink,
    tx: Sender<Msg>,
    /// Raised by the device callbacks before they post (see the module
    /// docs); lowered by [`Follower::devices_changed`].
    devices_flag: Arc<AtomicBool>,
    epoch: Instant,
    throttle: Throttle,
    filter: ChangeFilter,
    /// Failures in a row (a bind, or a read of a fresh binding) since the
    /// last good reading, for the retry delay and the log.
    failures: u32,
    /// The last reading failed (logged at `warn` once per streak).
    read_failing: bool,
    /// When the next bind is to be tried (ms since `epoch`), if one is.
    retry_at: Option<u64>,
}

fn thread_main(sink: &EventSink, tx: Sender<Msg>, rx: &Receiver<Msg>) {
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
            Ok(Msg::SetLevel(percent, reply)) => {
                let _ = reply.send(follower.set_level(percent));
            }
            Ok(Msg::SetMuted(muted, reply)) => {
                let _ = reply.send(follower.set_muted(muted));
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

impl Follower<'_> {
    fn now_ms(&self) -> u64 {
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

    /// Make sure the enumerator and its client exist and that the bound
    /// endpoint is the default one; re-bind when it is not.
    fn follow_default(&mut self, cause: &str) {
        if self.watch.is_none() {
            match DeviceWatch::new(&self.tx, &self.devices_flag) {
                Ok(watch) => self.watch = Some(watch),
                Err(e) => {
                    self.endpoint = None;
                    self.failed(&BindError::other(e), cause);
                    return;
                }
            }
        }
        let Some(watch) = &self.watch else {
            return;
        };
        match default_endpoint_id(&watch.enumerator) {
            Ok((_, id)) if self.endpoint.as_ref().is_some_and(|e| e.id == id) => {}
            Ok(_) => self.bind_default(cause),
            Err(e) => {
                self.endpoint = None;
                self.failed(&e, cause);
            }
        }
    }

    /// Drop the binding (its callback first) and bind the default endpoint.
    fn bind_default(&mut self, cause: &str) {
        self.endpoint = None;
        let Some(watch) = &self.watch else {
            return;
        };
        match bind(&watch.enumerator, &self.tx) {
            Ok(endpoint) => {
                log::info!("volume: following the default output device ({cause})");
                log::debug!("volume: default output device {}", endpoint.id);
                self.endpoint = Some(endpoint);
                self.retry_at = None;
            }
            Err(e) => self.failed(&e, cause),
        }
    }

    /// Log a failed bind once per streak and plan the retry.
    fn failed(&mut self, e: &BindError, cause: &str) {
        self.failures = self.failures.saturating_add(1);
        // Without the notifications nothing says when a device comes.
        let failure = if self.watch.is_some() {
            e.failure
        } else {
            BindFailure::Other
        };
        let retry = rules::retry_after_ms(failure, self.failures);
        self.retry_at = retry.map(|delay| self.now_ms().saturating_add(delay));
        let next = match retry {
            Some(delay) => format!("trying again in {} s", delay / 1000),
            None => "waiting for a device".to_owned(),
        };
        if self.failures == 1 {
            log::warn!(
                "volume: no default output device to follow ({cause}): {}; {next}",
                e.error
            );
        } else {
            log::debug!(
                "volume: still no default output device ({cause}, {} in a row): {}; {next}",
                self.failures,
                e.error
            );
        }
    }

    /// A live reading. A failed one (logged once per streak) drops the
    /// binding and binds once more; when the fresh binding cannot be read
    /// either, it is dropped too and tried again later like a failed bind,
    /// so a broken device costs one try per retry delay, not one per
    /// notification.
    fn read(&mut self) -> Option<VolumeStatus> {
        let endpoint = self.endpoint.as_ref()?;
        let error = match read_endpoint(endpoint) {
            Ok(status) => return Some(self.read_ok(status)),
            Err(e) => e,
        };
        if self.read_failing {
            log::debug!("volume: reading the default output device failed again: {error}");
        } else {
            log::warn!("volume: reading the default output device failed: {error}; binding again");
        }
        self.read_failing = true;
        self.bind_default("read failed");
        let endpoint = self.endpoint.as_ref()?;
        match read_endpoint(endpoint) {
            Ok(status) => Some(self.read_ok(status)),
            Err(e) => {
                self.endpoint = None;
                self.failed(&BindError::other(e), "read failed");
                None
            }
        }
    }

    /// A good reading ends every failure streak.
    fn read_ok(&mut self, status: VolumeStatus) -> VolumeStatus {
        self.read_failing = false;
        self.failures = 0;
        status
    }

    fn bound(&self) -> PlatformResult<&Endpoint> {
        self.endpoint
            .as_ref()
            .ok_or_else(|| PlatformError::Other("no audio output device".into()))
    }

    fn set_level(&self, percent: u8) -> PlatformResult<()> {
        let endpoint = self.bound()?;
        // SAFETY: a plain COM call on a live control; a null event context
        // is documented as allowed.
        unsafe {
            endpoint
                .volume
                .SetMasterVolumeLevelScalar(rules::scalar_of(percent), std::ptr::null())
        }?;
        Ok(())
    }

    fn set_muted(&self, muted: bool) -> PlatformResult<()> {
        let endpoint = self.bound()?;
        // SAFETY: a plain COM call on a live control; a null event context
        // is documented as allowed.
        unsafe { endpoint.volume.SetMute(muted, std::ptr::null()) }?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
