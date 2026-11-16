//! One dock window's backdrop window: its creation with the DWM attributes
//! of *The window* in the parent module, and its destruction.

use stem_core::Rect;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_TRANSITIONS_FORCEDISABLED,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWMWINDOWATTRIBUTE,
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, LWA_ALPHA, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_NOZORDER, SendMessageW, SetLayeredWindowAttributes, SetWindowPos,
    WINDOW_EX_STYLE, WM_NCACTIVATE, WS_CAPTION, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use super::class::WindowClass;
use crate::rules::backdrop::{RegionArgs, Variant};
use crate::windows::util;
use crate::{BackdropMaterial, PlatformResult};

const WINDOW_TITLE: &str = "Stem backdrop";

/// One dock window's backdrop. Lives and dies on the backdrop thread.
pub(super) struct BackdropWindow {
    pub(super) hwnd: HWND,
    pub(super) dock: HWND,
    /// The material last asked of DWM; `None` before the first request.
    pub(super) attempted: Option<BackdropMaterial>,
    /// DWM refused [`Self::attempted`]: the window stays hidden, since
    /// without the material it would only be a black rectangle.
    pub(super) refused: bool,
    pub(super) dark: Option<bool>,
    /// Where the window was last placed.
    pub(super) placed: Option<Rect>,
    /// The region it has; `None` before the first.
    pub(super) region: Option<RegionArgs>,
    pub(super) visible: bool,
}

impl BackdropWindow {
    pub(super) fn create(
        class: &WindowClass,
        dock: HWND,
        variant: Variant,
    ) -> PlatformResult<Self> {
        let mut ex_style = WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST;
        if variant.layered {
            ex_style |= WINDOW_EX_STYLE(WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0);
        }
        let title = util::to_wide(WINDOW_TITLE);
        // SAFETY: the class is registered on this thread; the string buffers
        // outlive the call; the window is created hidden, unowned and with
        // no menu.
        let hwnd = unsafe {
            CreateWindowExW(
                ex_style,
                util::pcwstr(&class.name),
                util::pcwstr(&title),
                WS_POPUP | WS_CAPTION,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(class.instance),
                None,
            )
        }?;
        // From here on `Drop` destroys the window, whatever fails below.
        let window = Self {
            hwnd,
            dock,
            attempted: None,
            refused: false,
            dark: None,
            placed: None,
            region: None,
            visible: false,
        };
        if variant.layered {
            // SAFETY: a layered window of this thread; alpha 255 and no
            // colour key: the window is drawn as it paints itself.
            unsafe { SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA) }?;
        }
        for (attribute, value, what) in [
            (DWMWA_TRANSITIONS_FORCEDISABLED, 1, "transitions off"),
            (DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, "no border"),
            (
                DWMWA_WINDOW_CORNER_PREFERENCE,
                DWMWCP_DONOTROUND.0 as u32,
                "no DWM rounding",
            ),
        ] {
            // Warn, not debug: each one is part of the recipe, and none
            // should fail on a supported build.
            if let Err(e) = set_attribute(hwnd, attribute, value) {
                log::warn!(
                    "backdrop window {:#x}: DwmSetWindowAttribute({what}) failed: {e}",
                    hwnd.0 as isize
                );
            }
        }
        if variant.extend_frame {
            let margins = MARGINS {
                cxLeftWidth: -1,
                cxRightWidth: -1,
                cyTopHeight: -1,
                cyBottomHeight: -1,
            };
            // SAFETY: `margins` is a live MARGINS for the duration of the
            // call; the window belongs to this thread.
            if let Err(e) = unsafe { DwmExtendFrameIntoClientArea(hwnd, &margins) } {
                log::warn!(
                    "backdrop window {:#x}: DwmExtendFrameIntoClientArea(-1) failed: {e}",
                    hwnd.0 as isize
                );
            }
        }
        // The window procedure's WM_NCCALCSIZE answer applies from here.
        frame_changed(hwnd);
        // One WM_NCACTIVATE(TRUE), so DWM treats the frame as active (A6).
        // Sent to a window of this very thread: a direct call of its
        // procedure, no cross-thread or cross-process wait.
        // SAFETY: plain integers to a live window of this thread.
        let _ = unsafe { SendMessageW(hwnd, WM_NCACTIVATE, Some(WPARAM(1)), Some(LPARAM(0))) };
        log::debug!(
            "backdrop window {:#x} created for dock window {:#x} ({})",
            hwnd.0 as isize,
            dock.0 as isize,
            variant.describe()
        );
        Ok(window)
    }
}

impl Drop for BackdropWindow {
    fn drop(&mut self) {
        // SAFETY: the window was created by this thread and is destroyed once.
        if let Err(e) = unsafe { DestroyWindow(self.hwnd) } {
            log::debug!(
                "DestroyWindow(backdrop {:#x}) failed: {e}",
                self.hwnd.0 as isize
            );
        }
    }
}

/// `DwmSetWindowAttribute` with a 4-byte payload (every attribute used
/// here: `BOOL`, `COLORREF` and the two enums are all 32-bit).
pub(super) fn set_attribute(
    hwnd: HWND,
    attribute: DWMWINDOWATTRIBUTE,
    value: u32,
) -> PlatformResult<()> {
    let ptr = std::ptr::from_ref(&value).cast::<core::ffi::c_void>();
    // SAFETY: `ptr` addresses a live u32 and the size passed is exactly that
    // of a u32, the documented payload size of each attribute used here.
    unsafe { DwmSetWindowAttribute(hwnd, attribute, ptr, size_of::<u32>() as u32) }?;
    Ok(())
}

/// Make the window manager send `WM_NCCALCSIZE` again: no move, size,
/// z-order or activation change. Failure is logged.
fn frame_changed(hwnd: HWND) {
    // SAFETY: the flags request a frame refresh only.
    let result = unsafe {
        SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        )
    };
    if let Err(e) = result {
        log::debug!(
            "SetWindowPos(SWP_FRAMECHANGED) on backdrop {:#x} failed: {e}",
            hwnd.0 as isize
        );
    }
}
