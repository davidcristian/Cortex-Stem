//! What a backdrop window shows and where: the material, the light or dark
//! mode, the region, and its place directly under the dock window.

use windows::Win32::Graphics::Dwm::{DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::UI::WindowsAndMessaging::{
    GW_HWNDNEXT, GetWindow, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SWP_SHOWWINDOW, SetWindowPos,
};

use super::region::set_region;
use super::window::{BackdropWindow, set_attribute};
use crate::DockBackdrop;
use crate::rules::backdrop::{Variant, material_attribute, region_args};

impl BackdropWindow {
    pub(super) fn apply(&mut self, wanted: &DockBackdrop, variant: Variant) {
        if self.attempted != Some(wanted.material) {
            self.attempted = Some(wanted.material);
            let (name, value, constant) = material_attribute(wanted.material);
            match set_attribute(self.hwnd, DWMWA_SYSTEMBACKDROP_TYPE, value) {
                Ok(()) => {
                    self.refused = false;
                    log::info!(
                        "native backdrop applied: {name} ({constant}) on window {:#x} under dock window {:#x} ({})",
                        self.hwnd.0 as isize,
                        self.dock.0 as isize,
                        variant.describe()
                    );
                }
                Err(e) => {
                    self.refused = true;
                    log::warn!(
                        "native backdrop refused by DWM: {name} ({constant}) on window {:#x}: {e}; the panel keeps its CSS look",
                        self.hwnd.0 as isize
                    );
                }
            }
        }
        if self.refused {
            self.hide();
            return;
        }
        if self.dark != Some(wanted.dark) {
            // Tried once per change: a refusal is not retried on every move.
            self.dark = Some(wanted.dark);
            match set_attribute(
                self.hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                u32::from(wanted.dark),
            ) {
                Ok(()) => {
                    log::debug!(
                        "backdrop window {:#x}: {} material",
                        self.hwnd.0 as isize,
                        if wanted.dark { "dark" } else { "light" }
                    );
                }
                Err(e) => log::warn!(
                    "backdrop window {:#x}: DWMWA_USE_IMMERSIVE_DARK_MODE failed: {e}",
                    self.hwnd.0 as isize
                ),
            }
        }
        let rect = wanted.rect;
        let Some(region) = region_args(rect.width, rect.height, wanted.corner_radius) else {
            log::debug!(
                "backdrop window {:#x}: panel {rect:?} too small for a region; hidden",
                self.hwnd.0 as isize
            );
            self.hide();
            return;
        };
        if self.region != Some(region) {
            match set_region(self.hwnd, region) {
                Ok(()) => self.region = Some(region),
                Err(e) => {
                    log::warn!(
                        "backdrop window {:#x}: SetWindowRgn failed: {e}; hidden",
                        self.hwnd.0 as isize
                    );
                    self.region = None;
                    self.hide();
                    return;
                }
            }
        }
        if self.visible && self.placed == Some(rect) {
            self.restack("unchanged request");
            return;
        }
        let mut flags = SWP_NOACTIVATE;
        if !self.visible {
            flags |= SWP_SHOWWINDOW;
        }
        // SAFETY: a window of this thread, placed directly under the dock
        // window (any thread's window is a valid insert-after handle);
        // failure is reported in the Result.
        let placed = unsafe {
            SetWindowPos(
                self.hwnd,
                Some(self.dock),
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                flags,
            )
        };
        match placed {
            Ok(()) => {
                self.placed = Some(rect);
                if !self.visible {
                    self.visible = true;
                    log::debug!(
                        "backdrop window {:#x} shown at {rect:?} under dock window {:#x}, region {region:?}",
                        self.hwnd.0 as isize,
                        self.dock.0 as isize
                    );
                } else {
                    log::trace!(
                        "backdrop window {:#x} moved to {rect:?}",
                        self.hwnd.0 as isize
                    );
                }
            }
            Err(e) => log::warn!(
                "backdrop window {:#x}: SetWindowPos to {rect:?} failed: {e}",
                self.hwnd.0 as isize
            ),
        }
    }

    pub(super) fn hide(&mut self) {
        if !self.visible {
            return;
        }
        // SAFETY: hides a window of this thread; no move, size, z-order or
        // activation change; failure is reported in the Result.
        let hidden = unsafe {
            SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_HIDEWINDOW | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
        match hidden {
            Ok(()) => {
                self.visible = false;
                log::debug!("backdrop window {:#x} hidden", self.hwnd.0 as isize);
            }
            Err(e) => log::warn!(
                "backdrop window {:#x}: hiding it failed: {e}",
                self.hwnd.0 as isize
            ),
        }
    }

    /// Put the window back directly under its dock window unless it is
    /// already there. Nothing while it is hidden.
    pub(super) fn restack(&mut self, why: &str) {
        if !self.visible {
            return;
        }
        // SAFETY: GetWindow only reads window-manager state; a handle that
        // is gone makes the call fail.
        if let Ok(next) = unsafe { GetWindow(self.dock, GW_HWNDNEXT) }
            && next == self.hwnd
        {
            return;
        }
        // SAFETY: the flags change the z-order of a window of this thread
        // only; failure is reported in the Result.
        let result = unsafe {
            SetWindowPos(
                self.hwnd,
                Some(self.dock),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
        };
        match result {
            Ok(()) => log::debug!(
                "backdrop window {:#x} restacked under dock window {:#x} ({why})",
                self.hwnd.0 as isize,
                self.dock.0 as isize
            ),
            Err(e) => log::warn!(
                "backdrop window {:#x}: restacking under dock window {:#x} failed: {e}",
                self.hwnd.0 as isize,
                self.dock.0 as isize
            ),
        }
    }
}
