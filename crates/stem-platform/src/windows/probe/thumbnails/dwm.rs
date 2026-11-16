//! The DWM thumbnail calls: register, update and unregister.

use windows::Win32::Foundation::{HWND, RECT, SIZE};
use windows::Win32::Graphics::Dwm::{
    DwmQueryThumbnailSourceSize, DwmRegisterThumbnail, DwmUnregisterThumbnail,
    DwmUpdateThumbnailProperties,
};

use super::facts::{frame_bounds, is_iconic, window_rect};
use super::geometry::{crop_to_frame, fit, height, thumbnail_props, width};
use super::timing::{Call, Clock};

/// What one registration did.
#[derive(Clone, Debug, Default)]
pub(super) struct Placement {
    pub(super) thumb: Option<isize>,
    pub(super) register: Option<Call>,
    pub(super) size: Option<SIZE>,
    size_call: Option<Call>,
    pub(super) window_rect: Option<RECT>,
    pub(super) frame: Option<RECT>,
    pub(super) crop: Option<RECT>,
    pub(super) crop_note: String,
    pub(super) placed: Option<RECT>,
    pub(super) update: Option<Call>,
}

impl Placement {
    pub(super) fn calls(&self) -> Vec<Call> {
        [&self.register, &self.size_call, &self.update]
            .into_iter()
            .flatten()
            .cloned()
            .collect()
    }
}

/// Registers `source` into `dest`, queries its size, crops it to its frame
/// when asked and possible, fits it into `image` and shows it.
pub(super) fn register_into(
    clock: Clock,
    dest: HWND,
    source: HWND,
    image: RECT,
    crop: bool,
) -> Placement {
    let mut placement = Placement::default();
    let (result, micros) = clock.time(|| {
        // SAFETY: both arguments are window handles (DWM reports a dead or
        // child window through the HRESULT); the destination belongs to
        // this process, as the function requires.
        unsafe { DwmRegisterThumbnail(dest, source) }
    });
    placement.register = Some(Call::of("DwmRegisterThumbnail", &result, micros));
    let Ok(thumb) = result else {
        return placement;
    };
    placement.thumb = Some(thumb);

    let (size, micros) = clock.time(|| {
        // SAFETY: `thumb` is a live registration of this process.
        unsafe { DwmQueryThumbnailSourceSize(thumb) }
    });
    placement.size_call = Some(Call::of("DwmQueryThumbnailSourceSize", &size, micros));
    placement.size = size.ok();
    placement.window_rect = window_rect(source);
    placement.frame = frame_bounds(source);

    let cropped = if !crop {
        Err("not asked for this slot".to_owned())
    } else {
        match (placement.window_rect, placement.frame, placement.size) {
            (Some(window), Some(frame), Some(size)) => {
                crop_to_frame(window, frame, size, is_iconic(source))
            }
            _ => Err("window rectangle, frame bounds or source size missing".to_owned()),
        }
    };
    match cropped {
        Ok(rect) => {
            placement.crop = Some(rect);
            placement.crop_note = format!(
                "rcSource ({}, {})-({}, {})",
                rect.left, rect.top, rect.right, rect.bottom
            );
        }
        Err(why) => placement.crop_note = format!("no rcSource: {why}"),
    }

    let (source_width, source_height) = match (placement.crop, placement.size) {
        (Some(rect), _) => (width(rect), height(rect)),
        (None, Some(size)) => (size.cx, size.cy),
        (None, None) => (16, 10),
    };
    let placed = fit(source_width, source_height, image);
    placement.placed = Some(placed);
    let props = thumbnail_props(placed, placement.crop);
    let (result, micros) = clock.time(|| {
        // SAFETY: `thumb` is a live registration of this process; `props` is
        // a fully initialised value that outlives the call.
        unsafe { DwmUpdateThumbnailProperties(thumb, &props) }
    });
    placement.update = Some(Call::of("DwmUpdateThumbnailProperties", &result, micros));
    placement
}

pub(super) fn update_again(clock: Clock, placement: &Placement) -> Option<Call> {
    let thumb = placement.thumb?;
    let props = thumbnail_props(placement.placed?, placement.crop);
    let (result, micros) = clock.time(|| {
        // SAFETY: `thumb` is a live registration of this process; `props` is
        // a fully initialised value that outlives the call.
        unsafe { DwmUpdateThumbnailProperties(thumb, &props) }
    });
    Some(Call::of("DwmUpdateThumbnailProperties", &result, micros))
}

pub(super) fn unregister(clock: Clock, thumb: isize) -> Call {
    let (result, micros) = clock.time(|| {
        // SAFETY: `thumb` is a registration handle of this process, a plain
        // value that DWM checks (a stale one fails with an HRESULT). Callers
        // take it out of its slot first; the phase 2 worker's slot forgets
        // it when the worker reports the call. Only a worker stuck inside
        // this very call can see the clean-up unregister it a second time;
        // that call fails, or ends a registration of this process that the
        // clean-up is ending anyway. No memory is involved either way.
        unsafe { DwmUnregisterThumbnail(thumb) }
    });
    Call::of("DwmUnregisterThumbnail", &result, micros)
}
