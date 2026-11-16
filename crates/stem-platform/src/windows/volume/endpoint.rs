//! The thread's COM objects, released in order (the device enumerator with
//! its notification client, and the bound endpoint with its callback), and
//! the calls that bind and read the endpoint.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crossbeam_channel::Sender;
use stem_core::VolumeStatus;
use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioEndpointVolumeCallback};
use windows::Win32::Media::Audio::{
    IMMDeviceEnumerator, IMMNotificationClient, MMDeviceEnumerator, eConsole, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};

use super::Msg;
use super::callbacks::{DeviceCallback, VolumeCallback};
use crate::rules::volume::{self as rules, BindFailure};
use crate::windows::util;
use crate::{PlatformError, PlatformResult};

/// The enumerator with the notification client registered on it;
/// unregistered on drop.
pub(super) struct DeviceWatch {
    pub(super) enumerator: IMMDeviceEnumerator,
    client: IMMNotificationClient,
}

impl DeviceWatch {
    pub(super) fn new(tx: &Sender<Msg>, devices_changed: &Arc<AtomicBool>) -> PlatformResult<Self> {
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
pub(super) struct Endpoint {
    pub(super) id: String,
    pub(super) volume: IAudioEndpointVolume,
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
pub(super) struct BindError {
    pub(super) failure: BindFailure,
    pub(super) error: PlatformError,
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

    pub(super) fn other(error: PlatformError) -> Self {
        Self {
            failure: BindFailure::Other,
            error,
        }
    }
}

/// The default console render endpoint's id, or why there is none.
pub(super) fn default_endpoint_id(
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
pub(super) fn bind(
    enumerator: &IMMDeviceEnumerator,
    tx: &Sender<Msg>,
) -> Result<Endpoint, BindError> {
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
pub(super) fn read_endpoint(endpoint: &Endpoint) -> PlatformResult<VolumeStatus> {
    // SAFETY: a plain COM call on a live control.
    let scalar = unsafe { endpoint.volume.GetMasterVolumeLevelScalar() }?;
    // SAFETY: a plain COM call on a live control.
    let muted = unsafe { endpoint.volume.GetMute() }?;
    Ok(rules::status_of(scalar, muted.as_bool()))
}
