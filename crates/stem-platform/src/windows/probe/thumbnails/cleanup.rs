//! Unregistering every thumbnail and destroying the probe's windows.

use std::sync::atomic::Ordering;

use windows::Win32::UI::WindowsAndMessaging::{DestroyWindow, KillTimer, UnregisterClassW};

use super::dwm::unregister;
use super::painting::SOURCES;
use super::probe::Probe;
use super::quit::DEST;
use super::{TICK_TIMER, emit};
use crate::windows::util;

impl Probe {
    /// Unregisters every thumbnail, then destroys every window of its own
    /// and the classes. Runs once; also from `Drop`.
    pub(super) fn cleanup(&mut self) {
        if self.cleaned {
            return;
        }
        self.cleaned = true;
        self.collect_late_worker();
        let clock = self.clock;
        let mut done = 0;
        let mut failed = 0;
        let mut slowest = 0.0f64;
        for slot in &mut self.slots {
            if let Some(thumb) = slot.placement.as_mut().and_then(|p| p.thumb.take()) {
                let call = unregister(clock, thumb);
                if call.ok() {
                    done += 1;
                } else {
                    failed += 1;
                    say!("[{}] {}", slot.number, call.describe());
                }
                slowest = slowest.max(call.micros);
            }
        }
        say!("unregistered {done} thumbnails ({failed} failed, slowest {slowest:.0} us)");
        DEST.store(0, Ordering::SeqCst);
        let mut destroyed = 0;
        let mut windows = self.own_windows.len();
        for hwnd in self.own_windows.drain(..) {
            // SAFETY: the timer belongs to our own window; a missing timer
            // only makes the call fail.
            let _ = unsafe { KillTimer(Some(hwnd), TICK_TIMER) };
            // SAFETY: a window this thread created, destroyed once.
            match unsafe { DestroyWindow(hwnd) } {
                Ok(()) => destroyed += 1,
                Err(e) => say!("DestroyWindow(source) failed: {e}"),
            }
        }
        if let Some(dest) = self.dest.take() {
            windows += 1;
            // SAFETY: a window this thread created, destroyed once, after its
            // thumbnails were unregistered.
            match unsafe { DestroyWindow(dest) } {
                Ok(()) => destroyed += 1,
                Err(e) => say!("DestroyWindow(probe window) failed: {e}"),
            }
        }
        for class in [&self.dest_class, &self.source_class] {
            // SAFETY: the name buffer is alive; every window of the class has
            // just been destroyed.
            let _ = unsafe { UnregisterClassW(util::pcwstr(class), Some(self.instance)) };
        }
        let _ = SOURCES.try_with(|sources| {
            if let Ok(mut list) = sources.try_borrow_mut() {
                list.clear();
            }
        });
        say!("destroyed {destroyed} of the probe's {windows} windows");
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        self.cleanup();
    }
}
