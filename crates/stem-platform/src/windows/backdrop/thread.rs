//! The backdrop thread itself: its message loop, and the backdrop windows
//! it owns.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;

use crossbeam_channel::Sender;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, IsWindow, MSG, PM_NOREMOVE, PM_REMOVE, PeekMessageW,
    TranslateMessage, WM_USER,
};

use super::class::WindowClass;
use super::window::BackdropWindow;
use super::{MSG_APPLY, MSG_RESTACK, Shared, THREAD_ID, variant_from_env};
use crate::rules::backdrop::Variant;
use crate::{DockBackdrop, PlatformResult};

pub(super) fn thread_main(shared: &Shared, done: &Sender<()>) {
    let mut msg = MSG::default();
    // SAFETY: `msg` is a live out-pointer; the call only makes the system
    // create this thread's message queue (nothing is removed).
    let _ = unsafe { PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE) };
    match Backdrops::new(variant_from_env()) {
        Ok(mut backdrops) => {
            // SAFETY: GetCurrentThreadId has no preconditions.
            let id = unsafe { GetCurrentThreadId() };
            THREAD_ID.store(id, Ordering::SeqCst);
            if !shared.quit.load(Ordering::SeqCst) {
                log::debug!(
                    "backdrop thread running (id {id}; {})",
                    backdrops.variant.describe()
                );
                backdrops.apply_pending(shared);
                backdrops.run(shared);
            }
            let _ = THREAD_ID.compare_exchange(id, 0, Ordering::SeqCst, Ordering::SeqCst);
            log::debug!("backdrop thread releasing its windows");
            drop(backdrops);
        }
        Err(e) => log::warn!("backdrop thread: the window class could not be registered: {e}"),
    }
    let _ = done.send(());
}

/// Everything the thread owns. Field order is drop order: the windows go
/// before their class.
struct Backdrops {
    /// Keyed by the dock window's raw `HWND` value.
    windows: HashMap<isize, BackdropWindow>,
    /// Dock windows whose backdrop could not be created; warned once each.
    failed: HashSet<isize>,
    class: WindowClass,
    variant: Variant,
}

impl Backdrops {
    fn new(variant: Variant) -> PlatformResult<Self> {
        Ok(Self {
            windows: HashMap::new(),
            failed: HashSet::new(),
            class: WindowClass::register()?,
            variant,
        })
    }

    fn run(&mut self, shared: &Shared) {
        loop {
            let mut msg = MSG::default();
            // SAFETY: `msg` is a live out-pointer; a NULL filter window asks
            // for every message of this thread.
            let result = unsafe { GetMessageW(&mut msg, None, 0, 0) };
            match result.0 {
                0 => break,
                -1 => {
                    log::error!(
                        "GetMessageW failed: {}; stopping the backdrop thread",
                        windows::core::Error::from_thread()
                    );
                    break;
                }
                _ => {}
            }
            if msg.hwnd.is_invalid() {
                match msg.message {
                    MSG_APPLY => {
                        drain_thread_messages(MSG_APPLY);
                        self.apply_pending(shared);
                    }
                    MSG_RESTACK => self.restack(msg.wParam.0 as isize),
                    _ => {}
                }
                continue;
            }
            // SAFETY: `msg` was filled in by GetMessageW.
            let _translated = unsafe { TranslateMessage(&msg) };
            // SAFETY: `msg` was filled in by GetMessageW.
            let _result = unsafe { DispatchMessageW(&msg) };
        }
    }

    fn apply_pending(&mut self, shared: &Shared) {
        let requests: Vec<(isize, Option<DockBackdrop>)> = shared.pending.lock().drain().collect();
        for (dock, wanted) in requests {
            self.apply(dock, wanted);
        }
    }

    fn apply(&mut self, dock_raw: isize, wanted: Option<DockBackdrop>) {
        let dock = HWND(dock_raw as *mut core::ffi::c_void);
        // SAFETY: IsWindow accepts any handle value.
        if !unsafe { IsWindow(Some(dock)) }.as_bool() {
            if self.windows.remove(&dock_raw).is_some() {
                log::debug!("dock window {dock_raw:#x} is gone; its backdrop window is destroyed");
            }
            return;
        }
        let Some(wanted) = wanted else {
            if let Some(window) = self.windows.get_mut(&dock_raw) {
                window.hide();
            }
            return;
        };
        if !self.windows.contains_key(&dock_raw) {
            if self.failed.contains(&dock_raw) {
                return;
            }
            match BackdropWindow::create(&self.class, dock, self.variant) {
                Ok(window) => {
                    self.windows.insert(dock_raw, window);
                }
                Err(e) => {
                    log::warn!(
                        "the backdrop window for dock window {dock_raw:#x} could not be created: {e}; the panel keeps its CSS look"
                    );
                    self.failed.insert(dock_raw);
                    return;
                }
            }
        }
        if let Some(window) = self.windows.get_mut(&dock_raw) {
            window.apply(&wanted, self.variant);
        }
    }

    fn restack(&mut self, dock_raw: isize) {
        if let Some(window) = self.windows.get_mut(&dock_raw) {
            window.restack("z-order change of the dock");
        }
    }
}

/// Remove further copies of a thread message already queued: several
/// requests in a row collapse into one pass.
fn drain_thread_messages(message: u32) {
    let mut msg = MSG::default();
    loop {
        // SAFETY: `msg` is a live out-pointer; the range selects only the
        // one thread message.
        let more = unsafe { PeekMessageW(&mut msg, None, message, message, PM_REMOVE) };
        if !more.as_bool() {
            break;
        }
    }
}
