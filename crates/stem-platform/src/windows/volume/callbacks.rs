//! The COM callbacks: each one posts a message to the thread and returns.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crossbeam_channel::Sender;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::Endpoints::{
    IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl,
};
use windows::Win32::Media::Audio::{
    AUDIO_VOLUME_NOTIFICATION_DATA, DEVICE_STATE, EDataFlow, ERole, IMMNotificationClient,
    IMMNotificationClient_Impl,
};
use windows::core::{PCWSTR, implement};

use super::Msg;
use crate::rules::volume as rules;

/// Post `msg` without waiting; a full queue already holds a message that
/// makes the thread read the latest value.
fn post(tx: &Sender<Msg>, msg: Msg) {
    // Full: a queued message reads the value anyway. Disconnected: the
    // thread is gone and nobody is listening.
    let _ = tx.try_send(msg);
}

/// The endpoint's `IAudioEndpointVolumeCallback`.
#[implement(IAudioEndpointVolumeCallback)]
pub(super) struct VolumeCallback {
    pub(super) tx: Sender<Msg>,
}

impl IAudioEndpointVolumeCallback_Impl for VolumeCallback_Impl {
    fn OnNotify(&self, _data: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> windows::core::Result<()> {
        post(&self.tx, Msg::VolumeNotified);
        Ok(())
    }
}

/// The enumerator's `IMMNotificationClient`.
#[implement(IMMNotificationClient)]
pub(super) struct DeviceCallback {
    pub(super) tx: Sender<Msg>,
    /// Raised before every post, so a post the full queue drops is not
    /// lost: the thread checks it after each message.
    pub(super) devices_changed: Arc<AtomicBool>,
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
