//! DWM thumbnail probe.
//!
//! A diagnostic program, not part of the app. It settles the open questions
//! of ADR-0009 (`docs/adr/0009-window-thumbnails.md`, assumptions A1 to A11)
//! on a Windows host, before window previews are built on DWM
//! thumbnails. Run it from a normal (not elevated) terminal with
//!
//! ```text
//! cargo run -p stem-platform --example thumbnail-probe --features probes
//! ```
//!
//! `docs/research/thumbnail-probe/` says what to open first, what it
//! prints and how to read the result.
//!
//! What it does on the desktop, and what it never does:
//! - It creates one destination window: plain, normal, not topmost, not
//!   resizable. Every thumbnail is drawn into it, in a labelled grid.
//! - It creates six source windows of its own: two plain ones (one shown
//!   cropped to its frame, one whole), one minimised, one cloaked with
//!   `DWMWA_CLOAK`, one with `WDA_EXCLUDEFROMCAPTURE` and one with
//!   `WDA_MONITOR`. Each paints its name and a running clock, so a live
//!   picture can be told from a frozen one.
//! - It looks for three windows it cannot create (an elevated Task Manager,
//!   a minimised or suspended Store app such as Calculator, a window on
//!   another virtual desktop) through the platform's own enumeration, and
//!   only reads them. It never minimises, moves, closes, activates or sends
//!   a message to another app's window; a thumbnail registration does not
//!   change its source.
//! - It pauses for Enter between phases: registration; calls from a worker
//!   thread (A6); a layered destination (A5); a second registration of one
//!   source (A11). It ends with a summary table. It waits for the worker at
//!   most `WORKER_DEADLINE`, pumping its messages meanwhile; a worker that
//!   does not finish is recorded as the A6 answer and the probe stops there,
//!   summary first. Only a line typed after a pause has printed its list
//!   answers it: earlier keys (a second Enter, Enter during the build) are
//!   thrown away, except `q`.
//! - It unregisters every thumbnail and destroys every window of its own
//!   before it exits: on Enter at the end, `q` and Enter at any pause,
//!   Ctrl+C, Ctrl+Break, closing the console, or the destination's close
//!   button. A second Ctrl+C while it is stopping ends the process at once.
//!
//! Failures are printed and the probe goes on; nothing here panics. Output
//! goes to stdout through `writeln!`, whose errors are ignored (a closed
//! console must not stop the clean-up).

use std::io::Write;
use std::process::ExitCode;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::COLORREF;
use windows::Win32::System::Console::SetConsoleCtrlHandler;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    WINDOW_STYLE, WM_APP, WS_CAPTION, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU,
};

use crate::windows::util;
use external::{find_external_windows, own_elevation};
use input::{flush_console_input, settle_input, spawn_stdin_reader, wait_for_line};
use probe::Probe;
use quit::{CLEANED, START, on_console_event, quit_text};
use timing::Clock;

/// Print one line to stdout (see the module documentation).
macro_rules! say {
    ($($arg:tt)*) => {
        emit(&format!($($arg)*))
    };
}

fn emit(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

mod cleanup;
mod display;
mod dwm;
mod external;
mod facts;
mod gdi;
mod geometry;
mod input;
mod own_windows;
mod painting;
mod phases;
mod plans;
mod probe;
mod quit;
mod registration;
mod summary;
#[cfg(test)]
mod tests;
mod timing;
mod worker;

const DEST_CLASS: &str = "StemThumbnailProbeDestination";
const SOURCE_CLASS: &str = "StemThumbnailProbeSource";
const DEST_TITLE: &str = "Stem thumbnail probe";
/// Plain, normal, not resizable (the grid does not reflow), not topmost.
const DEST_STYLE: WINDOW_STYLE =
    WINDOW_STYLE(WS_OVERLAPPED.0 | WS_CAPTION.0 | WS_SYSMENU.0 | WS_MINIMIZEBOX.0);
/// Posted to the destination to wake the message loop (stdin, Ctrl+C).
const WM_PROBE_WAKE: u32 = WM_APP + 0x31;
/// The source windows repaint their clock on this timer.
const TICK_TIMER: usize = 1;
const TICK_MS: u32 = 250;
/// Columns of the grid.
const COLUMNS: usize = 5;
/// A thumbnail call slower than this is flagged (ADR-0009: one frame).
const SLOW_MICROS: f64 = 16_000.0;
/// How long phase 2 waits for its worker thread (A6). A DWM call takes well
/// under a millisecond; a worker still busy after this is itself the answer.
const WORKER_DEADLINE: Duration = Duration::from_secs(5);
/// The slice phase 2 waits for a worker message between two message pumps.
const WORKER_SLICE: Duration = Duration::from_millis(10);
/// How long a pause lets the window repaint before it asks for Enter. Lines
/// typed until then (an Enter pressed twice) answer nothing.
const PAUSE_SETTLE: Duration = Duration::from_millis(400);
/// Windows' iconic position of a minimised window.
const ICONIC_COORDINATE: i32 = -32_000;
const TASK_MANAGER_CLASS: &str = "TaskManagerWindow";
const STORE_FRAME_CLASS: &str = "ApplicationFrameWindow";
const CALCULATOR_AUMID_PREFIX: &str = "Microsoft.WindowsCalculator";

const BACKGROUND: COLORREF = rgb(0x20, 0x20, 0x20);
const SLOT_FILL: COLORREF = rgb(0x4A, 0x4A, 0x4A);
const SLOT_TEXT: COLORREF = rgb(0xA0, 0xA0, 0xA0);
const SLOT_EDGE: COLORREF = rgb(0x30, 0x60, 0xC0);
const OUTLINE: COLORREF = rgb(0xFF, 0xD6, 0x00);
const TEXT: COLORREF = rgb(0xF0, 0xF0, 0xF0);
const TEXT_DIM: COLORREF = rgb(0xB8, 0xB8, 0xB8);
const WHITE: COLORREF = rgb(0xFF, 0xFF, 0xFF);

const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

fn banner() {
    emit("Stem thumbnail probe. docs/research/thumbnail-probe/ explains it.");
    emit("It opens one window of its own ('Stem thumbnail probe') and six source windows of");
    emit("its own, reads Task Manager, Calculator and a window on another virtual desktop if");
    emit("they are open, and changes no other app's window. Stop at any pause with q and");
    emit("Enter, Ctrl+C, or the probe window's close button; it always cleans up. If it");
    emit("ever hangs, a second Ctrl+C ends the process at once (Windows then frees it all).");
}

/// Runs the probe (see the module documentation). Never panics.
pub fn run() -> ExitCode {
    let _ = START.set(Instant::now());
    banner();
    // SAFETY: called before this process creates any window.
    let awareness =
        unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    match awareness {
        Ok(()) => emit("DPI awareness: per-monitor v2 (as Stem); every rectangle is physical"),
        Err(e) => say!("SetProcessDpiAwarenessContext(per-monitor v2) failed: {e}"),
    }
    let elevation = own_elevation();
    say!("probe process: {elevation}");
    if elevation == "ELEVATED" {
        emit("WARNING: run the probe from a terminal that is not elevated, or A8 is not tested");
    }
    let clock = Clock::new();
    let _com = util::ComApartment::sta();
    let external = find_external_windows();
    if let Some(note) = &external.note {
        say!("note: {note}");
    }
    let mut probe = match Probe::create(clock, external) {
        Ok(probe) => probe,
        Err(e) => {
            say!("the probe cannot start: {e}");
            CLEANED.store(true, Ordering::SeqCst);
            return ExitCode::FAILURE;
        }
    };
    // SAFETY: a handler with the signature the function expects; it only
    // touches atomics and posts a message.
    let handler = unsafe { SetConsoleCtrlHandler(Some(on_console_event), true) };
    if let Err(e) = handler {
        say!("SetConsoleCtrlHandler failed ({e}); Ctrl+C ends the probe without clean-up");
    }
    flush_console_input();
    let input = spawn_stdin_reader(probe.dest().0 as isize);
    probe.show_windows();
    let completed = probe.run_phases(&input);
    probe.summary();
    if completed {
        probe.set_header("Done: press Enter in the console to unregister everything and exit");
        emit("");
        settle_input(&input);
        emit("Press Enter to unregister every thumbnail and exit.");
        let _ = wait_for_line(&input);
    }
    probe.cleanup();
    CLEANED.store(true, Ordering::SeqCst);
    // SAFETY: removes the handler added above.
    let _ = unsafe { SetConsoleCtrlHandler(Some(on_console_event), false) };
    say!("stopped: {}", quit_text());
    ExitCode::SUCCESS
}
