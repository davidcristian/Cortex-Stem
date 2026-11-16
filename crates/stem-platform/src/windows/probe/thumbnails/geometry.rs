//! Pure geometry: rectangles, the crop to a window's frame, the thumbnail
//! properties and the destination's grid.

use windows::Win32::Foundation::{FALSE, RECT, SIZE, TRUE};
use windows::Win32::Graphics::Dwm::{
    DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION, DWM_TNP_RECTSOURCE,
    DWM_TNP_SOURCECLIENTAREAONLY, DWM_TNP_VISIBLE,
};

use super::{COLUMNS, ICONIC_COORDINATE};

pub(super) fn px(value: f64, scale: f64) -> i32 {
    (value * scale).round() as i32
}

pub(super) fn width(rect: RECT) -> i32 {
    rect.right - rect.left
}

pub(super) fn height(rect: RECT) -> i32 {
    rect.bottom - rect.top
}

pub(super) fn inflate(rect: RECT, by: i32) -> RECT {
    RECT {
        left: rect.left - by,
        top: rect.top - by,
        right: rect.right + by,
        bottom: rect.bottom + by,
    }
}

pub(super) fn rect_text(rect: RECT) -> String {
    format!(
        "{}x{} at ({}, {})",
        width(rect),
        height(rect),
        rect.left,
        rect.top
    )
}

/// The largest rectangle with the source's aspect ratio that fits `slot`,
/// centred, never scaled above 1:1. A degenerate source fills the slot.
pub(super) fn fit(source_width: i32, source_height: i32, slot: RECT) -> RECT {
    let slot_width = width(slot);
    let slot_height = height(slot);
    if source_width <= 0 || source_height <= 0 || slot_width <= 0 || slot_height <= 0 {
        return slot;
    }
    let scale = (f64::from(slot_width) / f64::from(source_width))
        .min(f64::from(slot_height) / f64::from(source_height))
        .min(1.0);
    let fitted_width = ((f64::from(source_width) * scale).round() as i32).clamp(1, slot_width);
    let fitted_height = ((f64::from(source_height) * scale).round() as i32).clamp(1, slot_height);
    let left = slot.left + (slot_width - fitted_width) / 2;
    let top = slot.top + (slot_height - fitted_height) / 2;
    RECT {
        left,
        top,
        right: left + fitted_width,
        bottom: top + fitted_height,
    }
}

/// `rcSource` for the visible frame: the extended frame bounds relative to
/// the window rectangle's origin, checked against the source size DWM
/// reports. `Err` says why no crop applies.
pub(super) fn crop_to_frame(
    window: RECT,
    frame: RECT,
    source: SIZE,
    iconic: bool,
) -> Result<RECT, String> {
    if iconic || (window.left <= ICONIC_COORDINATE && window.top <= ICONIC_COORDINATE) {
        return Err("minimised: the rectangles are the iconic ones".to_owned());
    }
    let crop = RECT {
        left: frame.left - window.left,
        top: frame.top - window.top,
        right: frame.right - window.left,
        bottom: frame.bottom - window.top,
    };
    if width(crop) <= 0 || height(crop) <= 0 {
        return Err(format!("empty frame {}", rect_text(frame)));
    }
    if crop.left < 0 || crop.top < 0 || crop.right > source.cx || crop.bottom > source.cy {
        return Err(format!(
            "frame ({}, {})-({}, {}) is outside the {}x{} source",
            crop.left, crop.top, crop.right, crop.bottom, source.cx, source.cy
        ));
    }
    Ok(crop)
}

pub(super) fn thumbnail_props(destination: RECT, crop: Option<RECT>) -> DWM_THUMBNAIL_PROPERTIES {
    let mut flags =
        DWM_TNP_RECTDESTINATION | DWM_TNP_VISIBLE | DWM_TNP_OPACITY | DWM_TNP_SOURCECLIENTAREAONLY;
    if crop.is_some() {
        flags |= DWM_TNP_RECTSOURCE;
    }
    // The struct is `packed(1)`: built by value, no field is ever borrowed.
    DWM_THUMBNAIL_PROPERTIES {
        dwFlags: flags,
        rcDestination: destination,
        rcSource: crop.unwrap_or_default(),
        opacity: 255,
        fVisible: TRUE,
        fSourceClientAreaOnly: FALSE,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SlotRects {
    pub(super) label: RECT,
    pub(super) image: RECT,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Layout {
    pub(super) client: SIZE,
    pub(super) slots: Vec<SlotRects>,
}

/// The destination's grid in physical pixels: `count` slots in rows of
/// [`COLUMNS`], each a two-line label over a 16:10 image area, shrunk to fit
/// the work area.
pub(super) fn layout(scale: f64, work: SIZE, count: usize) -> Layout {
    let count = count.max(1);
    let columns = COLUMNS.min(count) as i32;
    let rows = count.div_ceil(COLUMNS) as i32;
    let margin = px(12.0, scale);
    let line = px(18.0, scale);
    let label = 2 * line + px(4.0, scale);
    let header = 2 * line + px(12.0, scale);
    let max_width = (work.cx - px(64.0, scale)).max(480);
    let max_height = (work.cy - px(96.0, scale)).max(320);
    let width_for = |image_width: i32| columns * image_width + (columns + 1) * margin;
    let height_for = |image_width: i32| header + rows * (label + image_width * 10 / 16 + margin);

    let mut image_width = px(288.0, scale);
    if width_for(image_width) > max_width {
        image_width = (max_width - (columns + 1) * margin) / columns;
    }
    if height_for(image_width) > max_height {
        let image_height = (max_height - header) / rows - label - margin;
        image_width = image_width.min(image_height * 16 / 10);
    }
    let image_width = image_width.max(64);
    let image_height = image_width * 10 / 16;
    let client = SIZE {
        cx: width_for(image_width),
        cy: height_for(image_width),
    };
    let slots = (0..count)
        .map(|index| {
            let column = (index % COLUMNS) as i32;
            let row = (index / COLUMNS) as i32;
            let left = margin + column * (image_width + margin);
            let top = header + row * (label + image_height + margin);
            SlotRects {
                label: RECT {
                    left,
                    top,
                    right: left + image_width,
                    bottom: top + label,
                },
                image: RECT {
                    left,
                    top: top + label,
                    right: left + image_width,
                    bottom: top + label + image_height,
                },
            }
        })
        .collect();
    Layout { client, slots }
}
