//! GDI objects and the drawing helpers of the painting.

use windows::Win32::Foundation::{COLORREF, HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateFontIndirectW, CreateSolidBrush, DRAW_TEXT_FORMAT, DT_END_ELLIPSIS, DT_NOPREFIX,
    DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, FONT_WEIGHT, FillRect, FrameRect, HBRUSH,
    HDC, HGDIOBJ, LOGFONTW, SelectObject, SetTextColor,
};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

/// A GDI object we created (font, brush), deleted on drop. Callers select
/// the previous object back into the DC before the guard drops.
pub(super) struct GdiObject(HGDIOBJ);

impl GdiObject {
    pub(super) fn font(height: i32, weight: FONT_WEIGHT) -> Option<Self> {
        let mut face = [0u16; 32];
        for (slot, unit) in face.iter_mut().zip("Segoe UI".encode_utf16()) {
            *slot = unit;
        }
        let logfont = LOGFONTW {
            lfHeight: -height,
            lfWeight: weight.0 as i32,
            lfFaceName: face,
            ..Default::default()
        };
        // SAFETY: `logfont` is a fully initialised LOGFONTW that outlives
        // the call; the font is owned by the guard.
        let font = unsafe { CreateFontIndirectW(&logfont) };
        (!font.is_invalid()).then(|| Self(font.into()))
    }

    fn brush(colour: COLORREF) -> Option<Self> {
        // SAFETY: no pointer arguments; the brush is owned by the guard.
        let brush = unsafe { CreateSolidBrush(colour) };
        (!brush.is_invalid()).then(|| Self(brush.into()))
    }

    fn as_brush(&self) -> HBRUSH {
        HBRUSH(self.0.0)
    }
}

impl Drop for GdiObject {
    fn drop(&mut self) {
        // SAFETY: the object was created by this guard's constructor, is no
        // longer selected into a DC, and is deleted exactly once.
        let _ = unsafe { DeleteObject(self.0) };
    }
}

pub(super) fn fill(hdc: HDC, rect: RECT, colour: COLORREF) {
    if let Some(brush) = GdiObject::brush(colour) {
        // SAFETY: `hdc` is the DC being painted; `rect` outlives the call.
        let _ = unsafe { FillRect(hdc, &rect, brush.as_brush()) };
    }
}

pub(super) fn outline(hdc: HDC, rect: RECT, colour: COLORREF) {
    if let Some(brush) = GdiObject::brush(colour) {
        // SAFETY: `hdc` is the DC being painted; `rect` outlives the call.
        let _ = unsafe { FrameRect(hdc, &rect, brush.as_brush()) };
    }
}

pub(super) fn text(
    hdc: HDC,
    rect: RECT,
    content: &str,
    colour: COLORREF,
    font: Option<&GdiObject>,
    format: DRAW_TEXT_FORMAT,
) {
    let previous = font.map(|font| {
        // SAFETY: `hdc` is the DC being painted and `font` a live font.
        unsafe { SelectObject(hdc, font.0) }
    });
    // SAFETY: `hdc` is the DC being painted.
    let _ = unsafe { SetTextColor(hdc, colour) };
    let mut wide: Vec<u16> = content.encode_utf16().collect();
    let mut bounds = rect;
    // SAFETY: the text buffer and the rectangle are live for the call; the
    // format has no DT_MODIFYSTRING, so the buffer is only read.
    let _ = unsafe { DrawTextW(hdc, &mut wide, &mut bounds, format) };
    if let Some(previous) = previous {
        // SAFETY: puts back the DC's previous font before ours is deleted.
        let _ = unsafe { SelectObject(hdc, previous) };
    }
}

pub(super) fn single_line(align: DRAW_TEXT_FORMAT) -> DRAW_TEXT_FORMAT {
    align | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX
}

pub(super) fn client_rect(hwnd: HWND) -> RECT {
    let mut rect = RECT::default();
    // SAFETY: `rect` is a live out-pointer; a failure leaves it empty.
    let _ = unsafe { GetClientRect(hwnd, &mut rect) };
    rect
}
