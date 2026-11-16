//! What the probe's windows show: the sources' setup, the board, and the
//! layered destination of phase 3 (A5).

use std::time::Duration;

use windows::Win32::Foundation::{COLORREF, TRUE};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAK, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    InvalidateRect, RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE, RedrawWindow,
    UpdateWindow,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, LWA_ALPHA, SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE,
    SW_SHOWNORMAL, SetLayeredWindowAttributes, SetWindowDisplayAffinity, SetWindowLongPtrW,
    WDA_EXCLUDEFROMCAPTURE, WDA_MONITOR, WS_EX_LAYERED,
};
use windows::core::BOOL;

use super::emit;
use super::facts::is_iconic;
use super::own_windows::{pump_for, show};
use super::painting::{BOARD, Board, BoardSlot};
use super::plans::{Own, Source};
use super::probe::Probe;
use super::timing::Call;

impl Probe {
    /// Shows the windows, applies the cloak and the display affinities, and
    /// minimises the minimised source after it has painted a frame.
    pub(super) fn show_windows(&mut self) {
        let clock = self.clock;
        for slot in &mut self.slots {
            let (Source::Own(kind), Some(hwnd)) = (slot.plan.source, slot.source) else {
                continue;
            };
            match kind {
                Own::Plain | Own::Uncropped | Own::Minimised => show(hwnd, SW_SHOWNOACTIVATE),
                Own::Cloaked => {
                    let cloak = TRUE;
                    let (result, micros) = clock.time(|| {
                        // SAFETY: our own window; the buffer is a live BOOL
                        // and its size is passed.
                        unsafe {
                            DwmSetWindowAttribute(
                                hwnd,
                                DWMWA_CLOAK,
                                (&raw const cloak).cast(),
                                size_of::<BOOL>() as u32,
                            )
                        }
                    });
                    slot.setup.push(
                        Call::of("DwmSetWindowAttribute(DWMWA_CLOAK, TRUE)", &result, micros)
                            .describe(),
                    );
                    show(hwnd, SW_SHOWNOACTIVATE);
                }
                Own::ExcludeFromCapture | Own::MonitorOnly => {
                    show(hwnd, SW_SHOWNOACTIVATE);
                    let (wanted, name) = if kind == Own::MonitorOnly {
                        (WDA_MONITOR, "SetWindowDisplayAffinity(WDA_MONITOR)")
                    } else {
                        (
                            WDA_EXCLUDEFROMCAPTURE,
                            "SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)",
                        )
                    };
                    let (result, micros) = clock.time(|| {
                        // SAFETY: a top-level window of this process, as the
                        // function requires.
                        unsafe { SetWindowDisplayAffinity(hwnd, wanted) }
                    });
                    slot.setup.push(Call::of(name, &result, micros).describe());
                }
            }
        }
        let dest = self.dest();
        show(dest, SW_SHOWNORMAL);
        // SAFETY: our own window.
        let _ = unsafe { UpdateWindow(dest) };
        pump_for(Duration::from_millis(800));
        for slot in &mut self.slots {
            if let (Source::Own(Own::Minimised), Some(hwnd)) = (slot.plan.source, slot.source) {
                show(hwnd, SW_SHOWMINNOACTIVE);
                slot.setup.push(format!(
                    "ShowWindow(SW_SHOWMINNOACTIVE) after 0.8 s shown; minimised now: {}",
                    if is_iconic(hwnd) { "yes" } else { "NO" }
                ));
            }
        }
        pump_for(Duration::from_millis(600));
    }

    pub(super) fn set_header(&mut self, header: &str) {
        self.header = header.to_owned();
        emit("");
        say!("==== {header} ====");
        self.refresh_board();
    }

    pub(super) fn refresh_board(&self) {
        let board = Board {
            scale: self.scale,
            header: self.header.clone(),
            legend: "Yellow outline: where DWM was asked to draw (rcDestination). \
                     Grey 'no picture here': DWM draws nothing. Own sources show a running clock."
                .to_owned(),
            slots: self
                .slots
                .iter()
                .map(|slot| BoardSlot {
                    label: slot.rects.label,
                    image: slot.rects.image,
                    title: slot.title(),
                    status: slot.status(),
                    placed: slot
                        .placement
                        .as_ref()
                        .filter(|placement| placement.thumb.is_some())
                        .and_then(|placement| placement.placed),
                })
                .collect(),
        };
        let _ = BOARD.try_with(|cell| {
            if let Ok(mut current) = cell.try_borrow_mut() {
                *current = board;
            }
        });
        if let Some(dest) = self.dest {
            // SAFETY: our own window; a NULL rectangle is its whole client
            // area.
            let _ = unsafe { InvalidateRect(Some(dest), None, true) };
        }
    }

    pub(super) fn set_layered(&mut self, on: bool) -> String {
        let dest = self.dest();
        let layered = WS_EX_LAYERED.0 as isize;
        // SAFETY: reads a style of our own window.
        let style = unsafe { GetWindowLongPtrW(dest, GWL_EXSTYLE) };
        let wanted = if on {
            style | layered
        } else {
            style & !layered
        };
        // SAFETY: changes the extended style of our own window, on its own
        // thread; the previous value is not needed.
        let _ = unsafe { SetWindowLongPtrW(dest, GWL_EXSTYLE, wanted) };
        let mut parts = Vec::new();
        if on {
            let (result, micros) = self.clock.time(|| {
                // SAFETY: our own window, layered since the line above.
                unsafe { SetLayeredWindowAttributes(dest, COLORREF(0), 255, LWA_ALPHA) }
            });
            parts.push(
                Call::of("SetLayeredWindowAttributes(alpha 255)", &result, micros).describe(),
            );
        } else {
            // SAFETY: our own window; the documented repaint after removing
            // WS_EX_LAYERED.
            let _ = unsafe {
                RedrawWindow(
                    Some(dest),
                    None,
                    None,
                    RDW_ERASE | RDW_INVALIDATE | RDW_FRAME | RDW_ALLCHILDREN,
                )
            };
            parts.push("RedrawWindow".to_owned());
        }
        // SAFETY: reads a style of our own window.
        let now = unsafe { GetWindowLongPtrW(dest, GWL_EXSTYLE) };
        parts.push(format!(
            "WS_EX_LAYERED is now {}",
            if now & layered != 0 { "set" } else { "clear" }
        ));
        let line = parts.join("; ");
        say!("    {line}");
        line
    }
}
