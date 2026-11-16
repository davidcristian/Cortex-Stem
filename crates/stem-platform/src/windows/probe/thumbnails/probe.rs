//! The probe's state: one slot per plan, and the creation of every window.

use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;

use windows::Win32::Foundation::{HINSTANCE, HWND, RECT, SIZE};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    SWP_NOACTIVATE, SWP_NOZORDER, SetTimer, SetWindowPos, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW,
};

use super::dwm::Placement;
use super::external::{External, describe_found};
use super::geometry::{SlotRects, height, inflate, layout, px, width};
use super::own_windows::{create_window, destination_proc, register_class, source_proc, work_area};
use super::painting::{SOURCES, SourcePaint};
use super::plans::{Find, PLANS, Plan, Source, missing_hint, own_colour};
use super::quit::DEST;
use super::timing::{Call, Clock};
use super::worker::WorkerEvent;
use super::{DEST_CLASS, DEST_STYLE, DEST_TITLE, SOURCE_CLASS, TICK_MS, TICK_TIMER, emit};
use crate::windows::util;

pub(super) struct Slot {
    pub(super) number: usize,
    pub(super) plan: Plan,
    pub(super) source: Option<HWND>,
    /// "own window ...", "found ..." or "not found: ...".
    pub(super) origin: String,
    /// What the probe did to its own source before registering it.
    pub(super) setup: Vec<String>,
    pub(super) rects: SlotRects,
    pub(super) placement: Option<Placement>,
}

impl Slot {
    pub(super) fn title(&self) -> String {
        format!(
            "{}. {}: {} [{}]",
            self.number, self.plan.key, self.plan.what, self.plan.answers
        )
    }

    pub(super) fn status(&self) -> String {
        if self.source.is_none() {
            return self.origin.clone();
        }
        let Some(placement) = &self.placement else {
            return if self.plan.source == Source::SecondOfFirst {
                "registered in phase 4".to_owned()
            } else {
                "not registered".to_owned()
            };
        };
        if let Some(register) = &placement.register
            && !register.ok()
        {
            return format!("DwmRegisterThumbnail {}", register.short());
        }
        if placement.thumb.is_none() {
            return "unregistered".to_owned();
        }
        let size = placement
            .size
            .map_or_else(|| "size ?".to_owned(), |s| format!("{}x{}", s.cx, s.cy));
        let crop = if placement.crop.is_some() {
            "cropped"
        } else {
            "whole"
        };
        let update = placement
            .update
            .as_ref()
            .map_or_else(|| "-".to_owned(), Call::short);
        format!("registered, source {size}, {crop}, update {update}")
    }
}

pub(super) struct Probe {
    pub(super) clock: Clock,
    pub(super) instance: HINSTANCE,
    pub(super) dest_class: Vec<u16>,
    pub(super) source_class: Vec<u16>,
    pub(super) dest: Option<HWND>,
    pub(super) own_windows: Vec<HWND>,
    pub(super) scale: f64,
    pub(super) dpi: u32,
    pub(super) header: String,
    pub(super) slots: Vec<Slot>,
    /// Every DWM call, for the A6 lines.
    pub(super) calls: Vec<Call>,
    /// One line per phase step, for the summary.
    pub(super) results: Vec<String>,
    /// What was typed at each pause.
    pub(super) notes: Vec<(String, String)>,
    /// The phase 2 worker's messages when it did not finish in time; the
    /// clean-up reads what arrived late.
    pub(super) late_worker: Option<Receiver<WorkerEvent>>,
    pub(super) cleaned: bool,
}

impl Probe {
    pub(super) fn create(clock: Clock, mut external: External) -> Result<Self, String> {
        // SAFETY: a NULL module name asks for this executable's handle.
        let module = unsafe { GetModuleHandleW(None) };
        let module = module.map_err(|e| format!("GetModuleHandleW failed: {e}"))?;
        let instance = HINSTANCE::from(module);
        let mut probe = Self {
            clock,
            instance,
            dest_class: util::to_wide(DEST_CLASS),
            source_class: util::to_wide(SOURCE_CLASS),
            dest: None,
            own_windows: Vec::new(),
            scale: 1.0,
            dpi: 96,
            header: String::new(),
            slots: Vec::new(),
            calls: Vec::new(),
            results: Vec::new(),
            notes: Vec::new(),
            late_worker: None,
            cleaned: false,
        };
        register_class(instance, &probe.dest_class, destination_proc)?;
        register_class(instance, &probe.source_class, source_proc)?;

        let work = work_area();
        let start = RECT {
            left: work.left + 24,
            top: work.top + 24,
            right: work.left + 424,
            bottom: work.top + 324,
        };
        let dest = create_window(instance, &probe.dest_class, DEST_TITLE, DEST_STYLE, start)?;
        probe.dest = Some(dest);
        DEST.store(dest.0 as isize, Ordering::SeqCst);
        // SAFETY: our own window.
        let dpi = unsafe { GetDpiForWindow(dest) };
        probe.dpi = if dpi == 0 { 96 } else { dpi };
        probe.scale = f64::from(probe.dpi) / 96.0;
        let scale = probe.scale;

        let grid = layout(
            scale,
            SIZE {
                cx: width(work),
                cy: height(work),
            },
            PLANS.len(),
        );
        let mut frame = RECT {
            left: 0,
            top: 0,
            right: grid.client.cx,
            bottom: grid.client.cy,
        };
        // SAFETY: `frame` is a live RECT; the style matches the window's.
        let adjusted = unsafe {
            AdjustWindowRectExForDpi(&mut frame, DEST_STYLE, false, WINDOW_EX_STYLE(0), probe.dpi)
        };
        if let Err(e) = adjusted {
            say!("AdjustWindowRectExForDpi failed ({e}); the window gets a guessed frame");
            frame = inflate(frame, px(8.0, scale));
            frame.top -= px(24.0, scale);
        }
        // SAFETY: our own window; no z-order change and no activation.
        let moved = unsafe {
            SetWindowPos(
                dest,
                None,
                work.left + px(16.0, scale),
                work.top + px(16.0, scale),
                width(frame),
                height(frame),
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
        if let Err(e) = moved {
            say!("SetWindowPos(probe window) failed: {e}");
        }

        let source_width = px(480.0, scale).min(width(work) / 2);
        let source_height = px(300.0, scale).min(height(work) / 2);
        let mut cascade = 0;
        for (index, plan) in PLANS.iter().enumerate() {
            let number = index + 1;
            let rects = grid.slots.get(index).copied().unwrap_or(SlotRects {
                label: RECT::default(),
                image: RECT::default(),
            });
            let (source, origin) = match plan.source {
                Source::Own(kind) => {
                    let offset = px(40.0, scale) * cascade;
                    cascade += 1;
                    let right = work.right - px(24.0, scale) - offset;
                    let bottom = work.bottom - px(24.0, scale) - offset;
                    let rect = RECT {
                        left: right - source_width,
                        top: bottom - source_height,
                        right,
                        bottom,
                    };
                    let title = format!("Stem probe source {number} - {}", plan.key);
                    match create_window(
                        instance,
                        &probe.source_class,
                        &title,
                        WS_OVERLAPPEDWINDOW,
                        rect,
                    ) {
                        Ok(hwnd) => {
                            probe.own_windows.push(hwnd);
                            let _ = SOURCES.try_with(|sources| {
                                if let Ok(mut list) = sources.try_borrow_mut() {
                                    list.push(SourcePaint {
                                        hwnd: hwnd.0 as isize,
                                        number,
                                        key: plan.key,
                                        colour: own_colour(kind),
                                        scale,
                                    });
                                }
                            });
                            // SAFETY: a timer on our own window, without a
                            // callback: WM_TIMER goes to this thread's queue.
                            if unsafe { SetTimer(Some(hwnd), TICK_TIMER, TICK_MS, None) } == 0 {
                                say!("SetTimer on source {number} failed; its clock stands still");
                            }
                            (Some(hwnd), format!("own window {:#x}", hwnd.0 as usize))
                        }
                        Err(e) => (None, format!("not created: {e}")),
                    }
                }
                Source::Find(kind) => {
                    let found = match kind {
                        Find::Elevated => external.elevated.take(),
                        Find::StoreApp => external.store_app.take(),
                        Find::OtherDesktop => external.other_desktop.take(),
                    };
                    match found {
                        Some(window) => (Some(util::hwnd(window.id)), describe_found(&window)),
                        None => (None, missing_hint(kind).to_owned()),
                    }
                }
                Source::SecondOfFirst => {
                    let first = probe.slots.first().and_then(|slot| slot.source);
                    let origin = match first {
                        Some(_) => "slot 1's source".to_owned(),
                        None => "slot 1 has no source".to_owned(),
                    };
                    (first, origin)
                }
            };
            probe.slots.push(Slot {
                number,
                plan: *plan,
                source,
                origin,
                setup: Vec::new(),
                rects,
                placement: None,
            });
        }
        Ok(probe)
    }

    pub(super) fn dest(&self) -> HWND {
        self.dest.unwrap_or_default()
    }
}
