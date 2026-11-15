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

use std::cell::RefCell;
use std::ffi::c_void;
use std::io::{BufRead, Write};
use std::process::ExitCode;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::time::{Duration, Instant};

use stem_core::RawWindow;
use windows::Win32::Foundation::{
    COLORREF, CloseHandle, ERROR_CLASS_ALREADY_EXISTS, ERROR_SUCCESS, FALSE, HANDLE, HINSTANCE,
    HWND, LPARAM, LRESULT, RECT, SIZE, TRUE, WPARAM,
};
use windows::Win32::Graphics::Dwm::{
    DWM_CLOAKED_APP, DWM_CLOAKED_INHERITED, DWM_CLOAKED_SHELL, DWM_THUMBNAIL_PROPERTIES,
    DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION, DWM_TNP_RECTSOURCE, DWM_TNP_SOURCECLIENTAREAONLY,
    DWM_TNP_VISIBLE, DWMWA_CLOAK, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
    DwmGetWindowAttribute, DwmQueryThumbnailSourceSize, DwmRegisterThumbnail,
    DwmSetWindowAttribute, DwmUnregisterThumbnail, DwmUpdateThumbnailProperties,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontIndirectW, CreateSolidBrush, DRAW_TEXT_FORMAT, DT_CENTER,
    DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW,
    EndPaint, FONT_WEIGHT, FW_NORMAL, FW_SEMIBOLD, FillRect, FrameRect, HBRUSH, HDC, HGDIOBJ,
    InvalidateRect, LOGFONTW, PAINTSTRUCT, RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE,
    RedrawWindow, SelectObject, SetBkMode, SetTextColor, TRANSPARENT, UpdateWindow,
};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Console::{
    CTRL_BREAK_EVENT, CTRL_C_EVENT, CTRL_CLOSE_EVENT, FlushConsoleInputBuffer, GetStdHandle,
    STD_INPUT_HANDLE, SetConsoleCtrlHandler,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::Registry::{
    HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow,
    SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GWL_EXSTYLE, GetClientRect, GetMessageW, GetWindowDisplayAffinity, GetWindowLongPtrW,
    GetWindowRect, IDC_ARROW, IsIconic, KillTimer, LWA_ALPHA, LoadCursorW, MSG, PM_REMOVE,
    PeekMessageW, PostMessageW, RegisterClassExW, SHOW_WINDOW_CMD, SPI_GETWORKAREA,
    SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE, SW_SHOWNORMAL, SWP_NOACTIVATE, SWP_NOZORDER,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetLayeredWindowAttributes, SetTimer,
    SetWindowDisplayAffinity, SetWindowLongPtrW, SetWindowPos, ShowWindow, SystemParametersInfoW,
    TranslateMessage, UnregisterClassW, WDA_EXCLUDEFROMCAPTURE, WDA_MONITOR, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_APP, WM_CLOSE, WM_ERASEBKGND, WM_PAINT, WM_TIMER, WNDCLASSEXW, WS_CAPTION,
    WS_EX_LAYERED, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_OVERLAPPEDWINDOW, WS_SYSMENU,
};
use windows::core::{BOOL, HRESULT, w};

use crate::windows::{enumerate, util, virtual_desktop};

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

// ---- constants -------------------------------------------------------------

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

// ---- what the grid holds ---------------------------------------------------

/// A source the probe creates itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Own {
    Plain,
    Uncropped,
    Minimised,
    Cloaked,
    ExcludeFromCapture,
    MonitorOnly,
}

/// A source the probe can only find.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Find {
    Elevated,
    StoreApp,
    OtherDesktop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Own(Own),
    Find(Find),
    /// The source of slot 1 again (phase 4, A11).
    SecondOfFirst,
}

#[derive(Clone, Copy, Debug)]
struct Plan {
    key: &'static str,
    what: &'static str,
    answers: &'static str,
    source: Source,
    /// Crop `rcSource` to the extended frame bounds.
    crop: bool,
}

const PLANS: [Plan; 10] = [
    Plan {
        key: "normal",
        what: "own window, cropped to its frame",
        answers: "A1 A2 A7",
        source: Source::Own(Own::Plain),
        crop: true,
    },
    Plan {
        key: "no crop",
        what: "own window, whole source",
        answers: "A7 A6",
        source: Source::Own(Own::Uncropped),
        crop: false,
    },
    Plan {
        key: "minimised",
        what: "own window, minimised",
        answers: "A3",
        source: Source::Own(Own::Minimised),
        crop: true,
    },
    Plan {
        key: "cloaked",
        what: "own window, DWMWA_CLOAK",
        answers: "A4",
        source: Source::Own(Own::Cloaked),
        crop: true,
    },
    Plan {
        key: "exclude",
        what: "own window, WDA_EXCLUDEFROMCAPTURE",
        answers: "A9",
        source: Source::Own(Own::ExcludeFromCapture),
        crop: true,
    },
    Plan {
        key: "monitor",
        what: "own window, WDA_MONITOR",
        answers: "A9",
        source: Source::Own(Own::MonitorOnly),
        crop: true,
    },
    Plan {
        key: "elevated",
        what: "Task Manager, elevated",
        answers: "A8",
        source: Source::Find(Find::Elevated),
        crop: true,
    },
    Plan {
        key: "store app",
        what: "Calculator, minimised or suspended",
        answers: "A4",
        source: Source::Find(Find::StoreApp),
        crop: true,
    },
    Plan {
        key: "other desktop",
        what: "a window on another virtual desktop",
        answers: "A4",
        source: Source::Find(Find::OtherDesktop),
        crop: true,
    },
    Plan {
        key: "second",
        what: "slot 1's source again (phase 4)",
        answers: "A11",
        source: Source::SecondOfFirst,
        crop: true,
    },
];

const fn own_colour(kind: Own) -> COLORREF {
    match kind {
        Own::Plain => rgb(0x2E, 0x7D, 0x32),
        Own::Uncropped => rgb(0x15, 0x65, 0xC0),
        Own::Minimised => rgb(0x6A, 0x1B, 0x9A),
        Own::Cloaked => rgb(0xC6, 0x28, 0x28),
        Own::ExcludeFromCapture => rgb(0xE6, 0x51, 0x00),
        Own::MonitorOnly => rgb(0x00, 0x83, 0x8F),
    }
}

const fn missing_hint(kind: Find) -> &'static str {
    match kind {
        Find::Elevated => "not found: open Task Manager (Ctrl+Shift+Esc) before the probe",
        Find::StoreApp => "not found: open Calculator and minimise it before the probe",
        Find::OtherDesktop => {
            "not found: open Notepad on a second virtual desktop (Win+Ctrl+D), then come back"
        }
    }
}

// ---- quitting ----------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum Quit {
    Typed = 1,
    CtrlC = 2,
    CtrlBreak = 3,
    ConsoleClosed = 4,
    CloseButton = 5,
    StdinClosed = 6,
    MessageLoop = 7,
    WorkerStuck = 8,
}

/// Why the probe stops (a `Quit` value), 0 while it runs.
static QUIT: AtomicU8 = AtomicU8::new(0);
/// The destination window's handle, for the console handler and the stdin
/// thread to wake the message loop; 0 when there is none.
static DEST: AtomicIsize = AtomicIsize::new(0);
/// Set once every thumbnail is unregistered and every window destroyed.
static CLEANED: AtomicBool = AtomicBool::new(false);
/// When the probe started: the source windows' clock.
static START: OnceLock<Instant> = OnceLock::new();

fn request_quit(reason: Quit) {
    let _ = QUIT.compare_exchange(0, reason as u8, Ordering::SeqCst, Ordering::SeqCst);
}

fn quitting() -> bool {
    QUIT.load(Ordering::SeqCst) != 0
}

fn quit_text() -> &'static str {
    match QUIT.load(Ordering::SeqCst) {
        1 => "q was typed",
        2 => "Ctrl+C",
        3 => "Ctrl+Break",
        4 => "the console was closed",
        5 => "the probe window's close button",
        6 => "stdin was closed",
        7 => "the message loop ended",
        8 => "the A6 worker thread did not finish",
        _ => "the probe finished",
    }
}

/// Wakes the destination's message loop from another thread.
fn wake(dest: isize) {
    if dest == 0 {
        return;
    }
    let hwnd = HWND(dest as *mut c_void);
    // SAFETY: PostMessageW accepts any handle value and fails for a dead
    // one; the message carries no pointers.
    let _ = unsafe { PostMessageW(Some(hwnd), WM_PROBE_WAKE, WPARAM(0), LPARAM(0)) };
}

/// Console control handler: Ctrl+C, Ctrl+Break and closing the console stop
/// the probe through its normal clean-up instead of killing it. A Ctrl+C or
/// Ctrl+Break once the probe is already stopping (for example while a DWM
/// call hangs) is left to Windows' default handler, which ends the process;
/// Windows then frees every thumbnail and window the process had.
unsafe extern "system" fn on_console_event(kind: u32) -> BOOL {
    let reason = match kind {
        CTRL_C_EVENT => Quit::CtrlC,
        CTRL_BREAK_EVENT => Quit::CtrlBreak,
        CTRL_CLOSE_EVENT => Quit::ConsoleClosed,
        _ => return FALSE,
    };
    if kind != CTRL_CLOSE_EVENT && quitting() {
        return FALSE;
    }
    request_quit(reason);
    wake(DEST.load(Ordering::SeqCst));
    if kind == CTRL_CLOSE_EVENT {
        // Windows ends the process when this handler returns: give the main
        // thread a moment to unregister and destroy first.
        let deadline = Instant::now() + Duration::from_secs(3);
        while !CLEANED.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    TRUE
}

// ---- timing and call records -------------------------------------------------

/// `QueryPerformanceCounter` timing.
#[derive(Clone, Copy, Debug)]
struct Clock {
    frequency: i64,
}

impl Clock {
    fn new() -> Self {
        let mut frequency = 0i64;
        // SAFETY: `frequency` is a live out-pointer for the call.
        let result = unsafe { QueryPerformanceFrequency(&mut frequency) };
        if let Err(e) = result {
            say!("QueryPerformanceFrequency failed ({e}); every call time reads 0");
            frequency = 0;
        }
        Self { frequency }
    }

    fn ticks() -> i64 {
        let mut now = 0i64;
        // SAFETY: `now` is a live out-pointer for the call; it cannot fail
        // on Windows XP and later, and a failure leaves 0.
        let _ = unsafe { QueryPerformanceCounter(&mut now) };
        now
    }

    /// Runs `call` and returns its value and its duration in microseconds.
    fn time<T>(self, call: impl FnOnce() -> T) -> (T, f64) {
        let start = Self::ticks();
        let value = call();
        let end = Self::ticks();
        let micros = if self.frequency > 0 {
            (end - start) as f64 * 1_000_000.0 / self.frequency as f64
        } else {
            0.0
        };
        (value, micros)
    }
}

/// One timed API call and its `HRESULT`.
#[derive(Clone, Debug)]
struct Call {
    api: &'static str,
    hr: HRESULT,
    micros: f64,
}

impl Call {
    fn of<T>(api: &'static str, result: &windows::core::Result<T>, micros: f64) -> Self {
        let hr = match result {
            Ok(_) => HRESULT(0),
            Err(e) => e.code(),
        };
        Self { api, hr, micros }
    }

    fn ok(&self) -> bool {
        self.hr.is_ok()
    }

    fn short(&self) -> String {
        if self.ok() {
            "ok".to_owned()
        } else {
            hex(self.hr)
        }
    }

    fn describe(&self) -> String {
        if self.ok() {
            format!("{} ok in {:.0} us", self.api, self.micros)
        } else {
            format!(
                "{} FAILED {} ({}) in {:.0} us",
                self.api,
                hex(self.hr),
                self.hr.message().trim(),
                self.micros
            )
        }
    }
}

fn hex(hr: HRESULT) -> String {
    format!("0x{:08X}", hr.0 as u32)
}

// ---- pure geometry -----------------------------------------------------------

fn px(value: f64, scale: f64) -> i32 {
    (value * scale).round() as i32
}

fn width(rect: RECT) -> i32 {
    rect.right - rect.left
}

fn height(rect: RECT) -> i32 {
    rect.bottom - rect.top
}

fn inflate(rect: RECT, by: i32) -> RECT {
    RECT {
        left: rect.left - by,
        top: rect.top - by,
        right: rect.right + by,
        bottom: rect.bottom + by,
    }
}

fn rect_text(rect: RECT) -> String {
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
fn fit(source_width: i32, source_height: i32, slot: RECT) -> RECT {
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
fn crop_to_frame(window: RECT, frame: RECT, source: SIZE, iconic: bool) -> Result<RECT, String> {
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

fn thumbnail_props(destination: RECT, crop: Option<RECT>) -> DWM_THUMBNAIL_PROPERTIES {
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
struct SlotRects {
    label: RECT,
    image: RECT,
}

#[derive(Clone, Debug, PartialEq)]
struct Layout {
    client: SIZE,
    slots: Vec<SlotRects>,
}

/// The destination's grid in physical pixels: `count` slots in rows of
/// [`COLUMNS`], each a two-line label over a 16:10 image area, shrunk to fit
/// the work area.
fn layout(scale: f64, work: SIZE, count: usize) -> Layout {
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

/// Left-aligned text columns separated by ` | `.
fn table(header: &[&str], rows: &[Vec<String>]) -> Vec<String> {
    let mut widths: Vec<usize> = header.iter().map(|h| h.len()).collect();
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            if let Some(width) = widths.get_mut(index) {
                *width = (*width).max(cell.len());
            }
        }
    }
    let line = |cells: Vec<&str>| {
        cells
            .iter()
            .zip(&widths)
            .map(|(cell, &width)| format!("{cell:<width$}"))
            .collect::<Vec<_>>()
            .join(" | ")
            .trim_end()
            .to_owned()
    };
    let mut out = vec![line(header.to_vec())];
    out.push(
        widths
            .iter()
            .map(|width| "-".repeat(*width))
            .collect::<Vec<_>>()
            .join("-+-"),
    );
    for row in rows {
        out.push(line(row.iter().map(String::as_str).collect()));
    }
    out
}

fn cloak_text(value: Option<u32>) -> String {
    let Some(value) = value else {
        return "cloak ?".to_owned();
    };
    if value == 0 {
        return "not cloaked".to_owned();
    }
    let mut parts = Vec::new();
    if value & DWM_CLOAKED_APP != 0 {
        parts.push("app");
    }
    if value & DWM_CLOAKED_SHELL != 0 {
        parts.push("shell");
    }
    if value & DWM_CLOAKED_INHERITED != 0 {
        parts.push("inherited");
    }
    format!("cloaked 0x{value:X} ({})", parts.join("+"))
}

// ---- finding the windows the probe cannot create -----------------------------

/// The windows found for the `Find` slots.
#[derive(Debug, Default)]
struct External {
    elevated: Option<RawWindow>,
    store_app: Option<RawWindow>,
    other_desktop: Option<RawWindow>,
    /// Why nothing could be looked for, or why the desktop is unknown.
    note: Option<String>,
}

fn is_notepad(window: &RawWindow) -> bool {
    let exe = window
        .exe_path
        .as_ref()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("notepad.exe"));
    let aumid = window
        .app_user_model_id
        .as_deref()
        .is_some_and(|id| id.contains("Notepad"));
    exe || aumid
}

fn is_calculator(window: &RawWindow) -> bool {
    window
        .app_user_model_id
        .as_deref()
        .is_some_and(|id| id.starts_with(CALCULATOR_AUMID_PREFIX))
}

/// Picks the sources the probe cannot create from the taskbar windows:
/// Task Manager by class, Calculator by AUMID (else another minimised or
/// cloaked Store frame), and a window the shell places on another desktop
/// (Notepad first). Windows on the current desktop come first for the
/// first two; no window fills two slots.
fn pick_external(windows: &[RawWindow]) -> External {
    let here = |window: &RawWindow| window.on_current_desktop != Some(false);
    let elevated = windows
        .iter()
        .filter(|window| window.class_name == TASK_MANAGER_CLASS)
        .min_by_key(|window| !here(window))
        .cloned();
    let store_app = windows
        .iter()
        .filter(|window| Some(window.id) != elevated.as_ref().map(|w| w.id))
        .filter_map(|window| {
            let rank = if is_calculator(window) && here(window) {
                0
            } else if window.class_name == STORE_FRAME_CLASS
                && (window.is_minimized || window.is_cloaked)
                && here(window)
            {
                1
            } else if is_calculator(window) {
                2
            } else {
                return None;
            };
            Some((rank, window))
        })
        .min_by_key(|(rank, _)| *rank)
        .map(|(_, window)| window.clone());
    let taken = [
        elevated.as_ref().map(|w| w.id),
        store_app.as_ref().map(|w| w.id),
    ];
    let other_desktop = windows
        .iter()
        .filter(|window| window.on_current_desktop == Some(false))
        .filter(|window| !taken.contains(&Some(window.id)))
        .min_by_key(|window| !is_notepad(window))
        .cloned();
    let note = windows
        .iter()
        .all(|window| window.on_current_desktop.is_none())
        .then(|| {
            "the shell gave no virtual desktop facts, so no window counts as on another desktop"
                .to_owned()
        });
    External {
        elevated,
        store_app,
        other_desktop,
        note,
    }
}

fn find_external_windows() -> External {
    // The calling thread holds a COM apartment (`run`); the manager slot is
    // released before it.
    let desktops = virtual_desktop::ThreadManager::enable();
    let windows = enumerate::enumerate_taskbar_windows();
    drop(desktops);
    match windows {
        Ok(list) => {
            say!("found {} taskbar windows of other processes", list.len());
            pick_external(&list)
        }
        Err(e) => External {
            note: Some(format!("the window enumeration failed: {e}")),
            ..External::default()
        },
    }
}

/// Owned token handle, closed on drop.
struct Token(HANDLE);

impl Drop for Token {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful OpenProcessToken and is
        // closed exactly once, here.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn token_elevation(process: HANDLE) -> Result<bool, String> {
    let mut raw = HANDLE::default();
    // SAFETY: `process` is a live handle with query access (or the current
    // process pseudo-handle); `raw` is a live out-pointer.
    let opened = unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut raw) };
    opened.map_err(|e| format!("OpenProcessToken: {e}"))?;
    let token = Token(raw);
    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0u32;
    // SAFETY: the buffer is a live TOKEN_ELEVATION and its size is passed;
    // `returned` is a live out-pointer.
    let read = unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            Some((&raw mut elevation).cast()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    };
    read.map_err(|e| format!("GetTokenInformation: {e}"))?;
    Ok(elevation.TokenIsElevated != 0)
}

fn own_elevation() -> String {
    // SAFETY: GetCurrentProcess returns a pseudo-handle; it needs no close.
    let process = unsafe { GetCurrentProcess() };
    match token_elevation(process) {
        Ok(true) => "ELEVATED".to_owned(),
        Ok(false) => "not elevated".to_owned(),
        Err(e) => format!("unknown ({e})"),
    }
}

fn process_elevation(process_id: u32) -> String {
    match util::ProcessHandle::open_limited(process_id) {
        Err(e) => format!("elevation unknown (OpenProcess: {e})"),
        Ok(process) => match token_elevation(process.raw()) {
            Ok(true) => "elevated".to_owned(),
            Ok(false) => "NOT elevated".to_owned(),
            Err(e) => format!("elevation unknown ({e})"),
        },
    }
}

fn describe_found(window: &RawWindow) -> String {
    let exe = window
        .exe_path
        .as_ref()
        .and_then(|path| path.file_name())
        .map_or_else(
            || "?".to_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
    let desktop = match window.on_current_desktop {
        Some(true) => "this desktop",
        Some(false) => "another desktop",
        None => "desktop unknown",
    };
    format!(
        "found '{}' ({}, pid {}, {exe}, {}, {}, {desktop})",
        window.title,
        window.class_name,
        window.process_id,
        process_elevation(window.process_id),
        if window.is_minimized {
            "minimised"
        } else {
            "not minimised"
        },
    )
}

// ---- facts about a window ----------------------------------------------------

fn window_rect(hwnd: HWND) -> Option<RECT> {
    let mut rect = RECT::default();
    // SAFETY: `rect` is a live out-pointer; a dead handle fails the call.
    unsafe { GetWindowRect(hwnd, &mut rect) }.ok()?;
    Some(rect)
}

fn frame_bounds(hwnd: HWND) -> Option<RECT> {
    let mut rect = RECT::default();
    // SAFETY: the buffer is a live RECT and its size is passed.
    let result = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut rect).cast(),
            size_of::<RECT>() as u32,
        )
    };
    result.ok()?;
    Some(rect)
}

fn cloak_value(hwnd: HWND) -> Option<u32> {
    let mut value = 0u32;
    // SAFETY: the buffer is a live u32 and its size is passed.
    let result = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            (&raw mut value).cast(),
            size_of::<u32>() as u32,
        )
    };
    result.ok()?;
    Some(value)
}

fn is_iconic(hwnd: HWND) -> bool {
    // SAFETY: IsIconic accepts any handle value.
    unsafe { IsIconic(hwnd) }.as_bool()
}

fn affinity(hwnd: HWND) -> Option<u32> {
    let mut value = 0u32;
    // SAFETY: `value` is a live out-pointer; a dead handle fails the call.
    unsafe { GetWindowDisplayAffinity(hwnd, &mut value) }.ok()?;
    Some(value)
}

fn facts(hwnd: HWND) -> String {
    let affinity = affinity(hwnd).map_or_else(|| "?".to_owned(), |v| format!("0x{v:X}"));
    format!(
        "{}, {}, display affinity {affinity}",
        if is_iconic(hwnd) {
            "minimised"
        } else {
            "not minimised"
        },
        cloak_text(cloak_value(hwnd)),
    )
}

fn short_state(hwnd: HWND) -> String {
    let mut parts = Vec::new();
    if is_iconic(hwnd) {
        parts.push("iconic".to_owned());
    }
    match cloak_value(hwnd) {
        Some(0) => {}
        Some(value) => parts.push(format!("cloak 0x{value:X}")),
        None => parts.push("cloak ?".to_owned()),
    }
    match affinity(hwnd) {
        Some(0) => {}
        Some(value) => parts.push(format!("wda 0x{value:X}")),
        None => parts.push("wda ?".to_owned()),
    }
    if parts.is_empty() {
        "-".to_owned()
    } else {
        parts.join(", ")
    }
}

fn windows_build() -> String {
    let key = w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
    let mut buffer = [0u16; 32];
    let mut size = (buffer.len() * size_of::<u16>()) as u32;
    // SAFETY: `buffer` is writable for `size` bytes; RRF_RT_REG_SZ makes the
    // call fail for a value of another type; the names are static literals.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key,
            w!("CurrentBuild"),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    let build = if status == ERROR_SUCCESS {
        util::from_wide(&buffer)
    } else {
        "?".to_owned()
    };
    let mut revision = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: `revision` is a 4-byte buffer and `size` says so;
    // RRF_RT_REG_DWORD refuses other types; the names are static literals.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key,
            w!("UBR"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut revision).cast()),
            Some(&mut size),
        )
    };
    if status == ERROR_SUCCESS {
        format!("{build}.{revision}")
    } else {
        build
    }
}

// ---- DWM calls ---------------------------------------------------------------

/// What one registration did.
#[derive(Clone, Debug, Default)]
struct Placement {
    thumb: Option<isize>,
    register: Option<Call>,
    size: Option<SIZE>,
    size_call: Option<Call>,
    window_rect: Option<RECT>,
    frame: Option<RECT>,
    crop: Option<RECT>,
    crop_note: String,
    placed: Option<RECT>,
    update: Option<Call>,
}

impl Placement {
    fn calls(&self) -> Vec<Call> {
        [&self.register, &self.size_call, &self.update]
            .into_iter()
            .flatten()
            .cloned()
            .collect()
    }
}

/// Registers `source` into `dest`, queries its size, crops it to its frame
/// when asked and possible, fits it into `image` and shows it.
fn register_into(clock: Clock, dest: HWND, source: HWND, image: RECT, crop: bool) -> Placement {
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

fn update_again(clock: Clock, placement: &Placement) -> Option<Call> {
    let thumb = placement.thumb?;
    let props = thumbnail_props(placement.placed?, placement.crop);
    let (result, micros) = clock.time(|| {
        // SAFETY: `thumb` is a live registration of this process; `props` is
        // a fully initialised value that outlives the call.
        unsafe { DwmUpdateThumbnailProperties(thumb, &props) }
    });
    Some(Call::of("DwmUpdateThumbnailProperties", &result, micros))
}

fn unregister(clock: Clock, thumb: isize) -> Call {
    let (result, micros) = clock.time(|| {
        // SAFETY: `thumb` is a registration of this process that the caller
        // has taken out of its slot, so it is unregistered exactly once.
        unsafe { DwmUnregisterThumbnail(thumb) }
    });
    Call::of("DwmUnregisterThumbnail", &result, micros)
}

// ---- the worker thread of phase 2 (A6) ------------------------------------------

/// What the phase 2 worker sends back: one message per finished step, so a
/// call that never returns shows as the step that is missing.
enum WorkerEvent {
    /// A call returned on the worker. `unregistered` is the thumbnail it
    /// unregistered (slot 2's old one), which the slot then forgets.
    Call {
        call: Call,
        unregistered: Option<isize>,
    },
    /// Slot 2's new registration, made on the worker.
    Placement(Placement),
    /// Every step is done.
    Finished,
}

/// How the wait for the worker ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkerEnd {
    Finished,
    /// The worker's channel closed before `Finished` (it panicked).
    Vanished,
    /// The probe was asked to stop while it waited.
    Stopped,
    /// `WORKER_DEADLINE` passed first.
    TimedOut,
}

/// The A6 worker-thread result line: what returned, and, when the worker did
/// not finish, the step it was still in.
fn worker_result(
    end: WorkerEnd,
    returned: &[String],
    pending: Option<&str>,
    waited: Duration,
) -> String {
    let returned = if returned.is_empty() {
        "nothing".to_owned()
    } else {
        returned.join("; ")
    };
    let pending = pending.unwrap_or("its last message");
    let waited = waited.as_millis();
    match end {
        WorkerEnd::Finished => returned,
        WorkerEnd::Vanished => format!(
            "the worker thread ended before it finished (it panicked) in {pending}; \
             returned before: {returned}"
        ),
        WorkerEnd::Stopped => format!(
            "stopped waiting after {waited} ms: still in {pending}; returned before: {returned}"
        ),
        WorkerEnd::TimedOut => format!(
            "did not finish within {waited} ms: still in {pending} (a thread affinity or a \
             blocking call); returned before: {returned}"
        ),
    }
}

// ---- painting ----------------------------------------------------------------

/// What the destination paints; filled by the probe, read by `WM_PAINT`.
#[derive(Debug)]
struct Board {
    scale: f64,
    header: String,
    legend: String,
    slots: Vec<BoardSlot>,
}

impl Board {
    const fn empty() -> Self {
        Self {
            scale: 1.0,
            header: String::new(),
            legend: String::new(),
            slots: Vec::new(),
        }
    }
}

#[derive(Debug)]
struct BoardSlot {
    label: RECT,
    image: RECT,
    title: String,
    status: String,
    placed: Option<RECT>,
}

/// What a source window paints.
#[derive(Debug)]
struct SourcePaint {
    hwnd: isize,
    number: usize,
    key: &'static str,
    colour: COLORREF,
    scale: f64,
}

thread_local! {
    static BOARD: RefCell<Board> = const { RefCell::new(Board::empty()) };
    static SOURCES: RefCell<Vec<SourcePaint>> = const { RefCell::new(Vec::new()) };
}

/// A GDI object we created (font, brush), deleted on drop. Callers select
/// the previous object back into the DC before the guard drops.
struct GdiObject(HGDIOBJ);

impl GdiObject {
    fn font(height: i32, weight: FONT_WEIGHT) -> Option<Self> {
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

fn fill(hdc: HDC, rect: RECT, colour: COLORREF) {
    if let Some(brush) = GdiObject::brush(colour) {
        // SAFETY: `hdc` is the DC being painted; `rect` outlives the call.
        let _ = unsafe { FillRect(hdc, &rect, brush.as_brush()) };
    }
}

fn outline(hdc: HDC, rect: RECT, colour: COLORREF) {
    if let Some(brush) = GdiObject::brush(colour) {
        // SAFETY: `hdc` is the DC being painted; `rect` outlives the call.
        let _ = unsafe { FrameRect(hdc, &rect, brush.as_brush()) };
    }
}

fn text(
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

fn single_line(align: DRAW_TEXT_FORMAT) -> DRAW_TEXT_FORMAT {
    align | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX
}

fn client_rect(hwnd: HWND) -> RECT {
    let mut rect = RECT::default();
    // SAFETY: `rect` is a live out-pointer; a failure leaves it empty.
    let _ = unsafe { GetClientRect(hwnd, &mut rect) };
    rect
}

fn draw_board(hdc: HDC, board: &Board, client: RECT) {
    fill(hdc, client, BACKGROUND);
    let scale = board.scale;
    let header_font = GdiObject::font(px(15.0, scale), FW_SEMIBOLD);
    let label_font = GdiObject::font(px(13.0, scale), FW_NORMAL);
    // SAFETY: `hdc` is the DC being painted.
    let _ = unsafe { SetBkMode(hdc, TRANSPARENT) };
    let line = px(18.0, scale);
    let header = RECT {
        left: client.left + px(12.0, scale),
        top: client.top + px(6.0, scale),
        right: client.right - px(12.0, scale),
        bottom: client.top + px(6.0, scale) + line,
    };
    text(
        hdc,
        header,
        &board.header,
        TEXT,
        header_font.as_ref(),
        single_line(DT_LEFT),
    );
    let legend = RECT {
        top: header.bottom,
        bottom: header.bottom + line,
        ..header
    };
    text(
        hdc,
        legend,
        &board.legend,
        TEXT_DIM,
        label_font.as_ref(),
        single_line(DT_LEFT),
    );
    for slot in &board.slots {
        let first = RECT {
            bottom: slot.label.top + line,
            ..slot.label
        };
        let second = RECT {
            top: first.bottom,
            bottom: first.bottom + line,
            ..slot.label
        };
        text(
            hdc,
            first,
            &slot.title,
            TEXT,
            header_font.as_ref(),
            single_line(DT_LEFT),
        );
        text(
            hdc,
            second,
            &slot.status,
            TEXT_DIM,
            label_font.as_ref(),
            single_line(DT_LEFT),
        );
        fill(hdc, slot.image, SLOT_FILL);
        text(
            hdc,
            slot.image,
            "no picture here",
            SLOT_TEXT,
            label_font.as_ref(),
            single_line(DT_CENTER),
        );
        outline(hdc, inflate(slot.image, 1), SLOT_EDGE);
        if let Some(placed) = slot.placed {
            outline(hdc, inflate(placed, 1), OUTLINE);
            outline(hdc, inflate(placed, 2), OUTLINE);
        }
    }
}

fn paint_board(hwnd: HWND) {
    let mut paint = PAINTSTRUCT::default();
    // SAFETY: `hwnd` is the window being painted, on its own thread;
    // `paint` is a live out-struct.
    let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
    if !hdc.is_invalid() {
        let client = client_rect(hwnd);
        let _ = BOARD.try_with(|board| {
            if let Ok(board) = board.try_borrow() {
                draw_board(hdc, &board, client);
            }
        });
    }
    // SAFETY: balances the BeginPaint above with the same structure.
    let _ = unsafe { EndPaint(hwnd, &paint) };
}

fn paint_source(hwnd: HWND) {
    let mut paint = PAINTSTRUCT::default();
    // SAFETY: `hwnd` is the window being painted, on its own thread;
    // `paint` is a live out-struct.
    let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
    let id = hwnd.0 as isize;
    let found = SOURCES
        .try_with(|sources| {
            sources.try_borrow().ok().and_then(|list| {
                list.iter()
                    .find(|source| source.hwnd == id)
                    .map(|source| (source.number, source.key, source.colour, source.scale))
            })
        })
        .ok()
        .flatten();
    if let Some((number, key, colour, scale)) = found
        && !hdc.is_invalid()
    {
        let client = client_rect(hwnd);
        fill(hdc, client, WHITE);
        fill(hdc, inflate(client, -px(4.0, scale)), colour);
        // SAFETY: `hdc` is the DC being painted.
        let _ = unsafe { SetBkMode(hdc, TRANSPARENT) };
        let big = GdiObject::font(px(44.0, scale), FW_SEMIBOLD);
        let small = GdiObject::font(px(26.0, scale), FW_NORMAL);
        let elapsed = START
            .get()
            .map_or(0.0, |start| start.elapsed().as_secs_f64());
        let middle = client.top + height(client) / 2;
        let upper = RECT {
            bottom: middle,
            ..client
        };
        let lower = RECT {
            top: middle,
            ..client
        };
        text(
            hdc,
            upper,
            &format!("{number} {key}"),
            WHITE,
            big.as_ref(),
            single_line(DT_CENTER),
        );
        text(
            hdc,
            lower,
            &format!("t = {elapsed:.1} s"),
            WHITE,
            small.as_ref(),
            single_line(DT_CENTER),
        );
    }
    // SAFETY: balances the BeginPaint above with the same structure.
    let _ = unsafe { EndPaint(hwnd, &paint) };
}

unsafe extern "system" fn destination_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_PAINT => {
            paint_board(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND | WM_PROBE_WAKE => LRESULT(1),
        WM_CLOSE => {
            // The probe destroys the window itself, after unregistering.
            request_quit(Quit::CloseButton);
            LRESULT(0)
        }
        _ => {
            // SAFETY: forwarding the arguments we were given to the default
            // procedure.
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
    }
}

unsafe extern "system" fn source_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_PAINT => {
            paint_source(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_TIMER => {
            // SAFETY: invalidates our own window; no rectangle pointer.
            let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
            LRESULT(0)
        }
        // A source closes with the probe, never on its own.
        WM_CLOSE => LRESULT(0),
        _ => {
            // SAFETY: forwarding the arguments we were given to the default
            // procedure.
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
    }
}

// ---- windows -----------------------------------------------------------------

type WindowProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

fn register_class(instance: HINSTANCE, name: &[u16], procedure: WindowProc) -> Result<(), String> {
    // SAFETY: IDC_ARROW is a predefined system cursor; no module is needed.
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default();
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(procedure),
        hInstance: instance,
        hCursor: cursor,
        lpszClassName: util::pcwstr(name),
        ..Default::default()
    };
    // SAFETY: `class` is fully initialised and the name buffer outlives the
    // call (the probe keeps it until UnregisterClassW).
    if unsafe { RegisterClassExW(&class) } == 0 {
        let err = windows::core::Error::from_thread();
        if err.code() != util::hresult_of(ERROR_CLASS_ALREADY_EXISTS) {
            return Err(format!("RegisterClassExW failed: {err}"));
        }
    }
    Ok(())
}

fn create_window(
    instance: HINSTANCE,
    class: &[u16],
    title: &str,
    style: WINDOW_STYLE,
    rect: RECT,
) -> Result<HWND, String> {
    let wide_title = util::to_wide(title);
    // SAFETY: the class is registered by this process; both string buffers
    // outlive the call; no parent, menu or creation data. Extended style 0:
    // a normal window, not topmost, not a tool window.
    let created = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            util::pcwstr(class),
            util::pcwstr(&wide_title),
            style,
            rect.left,
            rect.top,
            width(rect),
            height(rect),
            None,
            None,
            Some(instance),
            None,
        )
    };
    created.map_err(|e| format!("CreateWindowExW('{title}') failed: {e}"))
}

fn show(hwnd: HWND, command: SHOW_WINDOW_CMD) {
    // SAFETY: our own window; the return value is the previous visibility.
    let _ = unsafe { ShowWindow(hwnd, command) };
}

fn work_area() -> RECT {
    let mut rect = RECT::default();
    // SAFETY: `rect` is a live RECT, as SPI_GETWORKAREA expects; nothing is
    // written to the profile (flags 0).
    let result = unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some((&raw mut rect).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    if result.is_err() || width(rect) <= 0 || height(rect) <= 0 {
        RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        }
    } else {
        rect
    }
}

/// Pumps this thread's messages for `duration` (so new windows paint and
/// DWM composes a frame before the next step).
fn pump_for(duration: Duration) {
    let end = Instant::now() + duration;
    while Instant::now() < end && !quitting() {
        pump_pending();
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Dispatches every message waiting for this thread, including messages
/// other threads send to its windows, and returns.
fn pump_pending() {
    loop {
        let mut message = MSG::default();
        // SAFETY: `message` is a live out-struct; a NULL window takes any
        // message of this thread.
        let more = unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) };
        if !more.as_bool() {
            break;
        }
        // SAFETY: `message` was filled by PeekMessageW.
        let _ = unsafe { TranslateMessage(&message) };
        // SAFETY: as above; dispatches to this thread's procedures.
        unsafe { DispatchMessageW(&message) };
    }
}

enum Step {
    Line(String),
    Stop,
}

/// Runs the message loop until a line arrives on stdin or the probe is
/// asked to stop.
fn wait_for_line(input: &Receiver<Option<String>>) -> Step {
    loop {
        if quitting() {
            return Step::Stop;
        }
        match input.try_recv() {
            Ok(Some(line)) => return Step::Line(line),
            Ok(None) | Err(TryRecvError::Disconnected) => {
                request_quit(Quit::StdinClosed);
                return Step::Stop;
            }
            Err(TryRecvError::Empty) => {}
        }
        let mut message = MSG::default();
        // SAFETY: `message` is a live out-struct; a NULL window takes any
        // message of this thread.
        let got = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if got.0 == 0 || got.0 == -1 {
            request_quit(Quit::MessageLoop);
            return Step::Stop;
        }
        // SAFETY: `message` was filled by GetMessageW.
        let _ = unsafe { TranslateMessage(&message) };
        // SAFETY: as above; dispatches to this thread's procedures.
        unsafe { DispatchMessageW(&message) };
    }
}

/// Reads stdin lines on a thread of its own; each line (or `None` at the
/// end of input) is sent and the destination's loop woken.
fn spawn_stdin_reader(dest: isize) -> Receiver<Option<String>> {
    let (sender, receiver) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("probe-stdin".to_owned())
        .spawn(move || {
            let stdin = std::io::stdin();
            loop {
                let mut line = String::new();
                let read = match stdin.lock().read_line(&mut line) {
                    Ok(0) | Err(_) => None,
                    Ok(_) => Some(line.trim().to_owned()),
                };
                let end = read.is_none();
                if sender.send(read).is_err() {
                    break;
                }
                wake(dest);
                if end {
                    break;
                }
            }
        });
    if let Err(e) = spawned {
        say!("could not start the stdin reader ({e}); the probe stops at the first pause");
    }
    receiver
}

/// Throws away keys typed before the stdin reader starts, for example an
/// Enter pressed during the first build, which would otherwise answer the
/// first pause.
fn flush_console_input() {
    // SAFETY: reads this process's standard input handle; nothing is closed.
    let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    let flushed = handle.and_then(|handle| {
        // SAFETY: the process's own standard input handle, for this call
        // only; a handle that is not a console makes the call fail.
        unsafe { FlushConsoleInputBuffer(handle) }
    });
    if let Err(e) = flushed {
        say!("could not flush the console input ({e}); keys typed before now may answer a pause");
    }
}

/// What was already waiting on stdin when a pause began.
#[derive(Debug, Default, PartialEq, Eq)]
struct Drained {
    /// Lines typed before the pause asked for one; they answer nothing.
    ignored: Vec<String>,
    /// `q` among them, or the end of input.
    stop: Option<Quit>,
}

/// Takes every line already waiting, without blocking. `q` still stops the
/// probe and the end of input still ends it; any other line is ignored.
fn drain_early(input: &Receiver<Option<String>>) -> Drained {
    let mut drained = Drained::default();
    loop {
        match input.try_recv() {
            Ok(Some(line)) if line.eq_ignore_ascii_case("q") => {
                drained.stop = Some(Quit::Typed);
                return drained;
            }
            Ok(Some(line)) => drained.ignored.push(line),
            Ok(None) | Err(TryRecvError::Disconnected) => {
                drained.stop = Some(Quit::StdinClosed);
                return drained;
            }
            Err(TryRecvError::Empty) => return drained,
        }
    }
}

/// Before a pause asks for Enter: lets the windows repaint for
/// `PAUSE_SETTLE`, then drops every line typed before now (an Enter pressed
/// twice, or during a phase), so each pause waits for a fresh Enter.
fn settle_input(input: &Receiver<Option<String>>) {
    pump_for(PAUSE_SETTLE);
    let drained = drain_early(input);
    for line in &drained.ignored {
        if line.is_empty() {
            emit("(ignored an Enter typed before this pause)");
        } else {
            say!("(ignored a line typed before this pause: {line})");
        }
    }
    if let Some(reason) = drained.stop {
        request_quit(reason);
    }
}

// ---- the probe ---------------------------------------------------------------

struct Slot {
    number: usize,
    plan: Plan,
    source: Option<HWND>,
    /// "own window ...", "found ..." or "not found: ...".
    origin: String,
    /// What the probe did to its own source before registering it.
    setup: Vec<String>,
    rects: SlotRects,
    placement: Option<Placement>,
}

impl Slot {
    fn title(&self) -> String {
        format!(
            "{}. {}: {} [{}]",
            self.number, self.plan.key, self.plan.what, self.plan.answers
        )
    }

    fn status(&self) -> String {
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

struct Probe {
    clock: Clock,
    instance: HINSTANCE,
    dest_class: Vec<u16>,
    source_class: Vec<u16>,
    dest: Option<HWND>,
    own_windows: Vec<HWND>,
    scale: f64,
    dpi: u32,
    header: String,
    slots: Vec<Slot>,
    /// Every DWM call, for the A6 lines.
    calls: Vec<Call>,
    /// One line per phase step, for the summary.
    results: Vec<String>,
    /// What was typed at each pause.
    notes: Vec<(String, String)>,
    /// The phase 2 worker's messages when it did not finish in time; the
    /// clean-up reads what arrived late.
    late_worker: Option<Receiver<WorkerEvent>>,
    cleaned: bool,
}

impl Probe {
    fn create(clock: Clock, mut external: External) -> Result<Self, String> {
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

    fn dest(&self) -> HWND {
        self.dest.unwrap_or_default()
    }

    /// Shows the windows, applies the cloak and the display affinities, and
    /// minimises the minimised source after it has painted a frame.
    fn show_windows(&mut self) {
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

    fn set_header(&mut self, header: &str) {
        self.header = header.to_owned();
        emit("");
        say!("==== {header} ====");
        self.refresh_board();
    }

    fn refresh_board(&self) {
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

    /// Unregisters the slot's thumbnail, if any, and registers its source
    /// again; prints what happened.
    fn place(&mut self, index: usize) {
        let clock = self.clock;
        let dest = self.dest();
        let Some(slot) = self.slots.get_mut(index) else {
            return;
        };
        let Some(source) = slot.source else {
            say!("[{}] {}: {}", slot.number, slot.plan.key, slot.origin);
            return;
        };
        if let Some(old) = slot.placement.as_mut().and_then(|p| p.thumb.take()) {
            let call = unregister(clock, old);
            say!("[{}] {}", slot.number, call.describe());
            self.calls.push(call);
        }
        let state = facts(source);
        let placement = register_into(clock, dest, source, slot.rects.image, slot.plan.crop);
        self.calls.extend(placement.calls());
        say!("[{}] {}", slot.number, slot.title());
        say!("    source {:#x}: {}", source.0 as usize, slot.origin);
        say!("    state now: {state}");
        for line in &slot.setup {
            say!("    setup: {line}");
        }
        for call in placement.calls() {
            say!("    {}", call.describe());
        }
        if let Some(size) = placement.size {
            say!("    source size (DWM): {}x{}", size.cx, size.cy);
        }
        let rect_or = |rect: Option<RECT>| rect.map_or_else(|| "?".to_owned(), rect_text);
        say!(
            "    GetWindowRect {}; extended frame bounds {}",
            rect_or(placement.window_rect),
            rect_or(placement.frame)
        );
        say!("    crop: {}", placement.crop_note);
        if let Some(placed) = placement.placed {
            say!(
                "    rcDestination {} (client coordinates)",
                rect_text(placed)
            );
        }
        slot.placement = Some(placement);
    }

    fn pause(&mut self, input: &Receiver<Option<String>>, phase: &str, look: &[&str]) -> bool {
        self.refresh_board();
        settle_input(input);
        emit("");
        emit("Look:");
        for line in look {
            say!("  - {line}");
        }
        emit("Take a screenshot (Win+Shift+S). Type a note if you like, then press Enter;");
        emit("q and Enter stops the probe (it still cleans up).");
        match wait_for_line(input) {
            Step::Line(line) if line.eq_ignore_ascii_case("q") => {
                request_quit(Quit::Typed);
                say!("stopping: {}", quit_text());
                false
            }
            Step::Line(line) => {
                if !line.is_empty() {
                    self.notes.push((phase.to_owned(), line));
                }
                true
            }
            Step::Stop => {
                say!("stopping: {}", quit_text());
                false
            }
        }
    }

    fn run_phases(&mut self, input: &Receiver<Option<String>>) -> bool {
        if quitting() {
            say!("stopping before phase 1: {}", quit_text());
            return false;
        }
        self.set_header("Phase 1 of 4: every source registered on the window's own thread");
        for index in self.primary_slots() {
            self.place(index);
        }
        if !self.pause(
            input,
            "phase 1",
            &[
                "each slot: a picture, a black or empty picture, or the grey 'no picture here'?",
                "own sources (1 to 6): does the clock in the picture tick, or is it frozen?",
                "slot 1 vs slot 2: slot 1 should end exactly at the source's visible frame; \
                 slot 2 may show a transparent margin (the invisible resize borders, A7)",
                "does every picture sit exactly inside its yellow outline (A2)?",
                "slots 5 and 6: compare the screen with your screenshot (a screenshot is itself \
                 a capture, A9)",
            ],
        ) {
            return false;
        }

        self.set_header("Phase 2 of 4: calls from a worker thread (A6)");
        if !self.phase_worker_thread() {
            return false;
        }
        if !self.pause(
            input,
            "phase 2",
            &[
                "slot 2 was unregistered and registered again on a worker thread: does it still \
                 show its picture?",
                "slot 1 was updated from the worker thread: unchanged?",
            ],
        ) {
            return false;
        }

        self.set_header("Phase 3 of 4, step 1: WS_EX_LAYERED on, alpha 255 (A5)");
        let line = self.set_layered(true);
        self.results.push(format!("A5 step 1 (layered on): {line}"));
        if !self.pause(
            input,
            "phase 3.1",
            &["do the pictures still show, flicker, freeze or vanish?"],
        ) {
            return false;
        }
        self.set_header("Phase 3 of 4, step 2: every thumbnail updated again while layered");
        let line = self.update_all();
        self.results
            .push(format!("A5 step 2 (update while layered): {line}"));
        if !self.pause(
            input,
            "phase 3.2",
            &["do the pictures show now? (record any change from step 1)"],
        ) {
            return false;
        }
        self.set_header("Phase 3 of 4, step 3: WS_EX_LAYERED off again");
        let line = self.set_layered(false);
        self.results
            .push(format!("A5 step 3 (layered off): {line}"));
        if !self.pause(
            input,
            "phase 3.3",
            &["did the pictures come back on their own?"],
        ) {
            return false;
        }
        self.set_header("Phase 3 of 4, step 4: every thumbnail unregistered and registered again");
        for index in self.primary_slots() {
            self.place(index);
        }
        let line = self.registration_count();
        self.results
            .push(format!("A5 step 4 (fresh registrations): {line}"));
        if !self.pause(
            input,
            "phase 3.4",
            &["every picture back as in phase 1? (what Stem does after click-through)"],
        ) {
            return false;
        }

        self.set_header("Phase 4 of 4, step 1: a second registration of slot 1's source (A11)");
        let second = self.slots.len().saturating_sub(1);
        self.place(second);
        let line = self
            .slots
            .get(second)
            .and_then(|slot| slot.placement.as_ref())
            .and_then(|placement| placement.register.as_ref())
            .map_or_else(|| "not registered (no source)".to_owned(), Call::describe);
        self.results
            .push(format!("A11 step 1 (second registration): {line}"));
        if !self.pause(
            input,
            "phase 4.1",
            &["do slot 1 and slot 10 both show a live picture (ticking clock)?"],
        ) {
            return false;
        }
        self.set_header("Phase 4 of 4, step 2: the second registration removed");
        let clock = self.clock;
        let line = match self
            .slots
            .get_mut(second)
            .and_then(|slot| slot.placement.as_mut())
            .and_then(|placement| placement.thumb.take())
        {
            Some(thumb) => {
                let call = unregister(clock, thumb);
                let text = call.describe();
                self.calls.push(call);
                text
            }
            None => "nothing to unregister".to_owned(),
        };
        say!("[10] {line}");
        self.results
            .push(format!("A11 step 2 (second removed): {line}"));
        self.pause(
            input,
            "phase 4.2",
            &["does slot 1 still show its live picture?"],
        )
    }

    /// Every slot but the second registration's (phase 4).
    fn primary_slots(&self) -> Vec<usize> {
        self.slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.plan.source != Source::SecondOfFirst)
            .map(|(index, _)| index)
            .collect()
    }

    fn registration_count(&self) -> String {
        let with_source = self
            .slots
            .iter()
            .filter(|slot| slot.source.is_some() && slot.plan.source != Source::SecondOfFirst)
            .count();
        let registered = self
            .slots
            .iter()
            .filter(|slot| {
                slot.placement
                    .as_ref()
                    .is_some_and(|placement| placement.thumb.is_some())
            })
            .count();
        format!("{registered} of {with_source} sources registered")
    }

    /// Phase 2 (A6): a worker thread updates slot 1, unregisters slot 2 and
    /// registers it again. This thread waits at most `WORKER_DEADLINE` for
    /// it, in short slices, and keeps pumping its windows' messages
    /// meanwhile (as the dock's UI thread would), so neither a worker call
    /// that needs the destination's thread nor one that blocks can hang the
    /// probe. Returns false when the worker did not finish: the probe then
    /// stops, since later phases call DWM again.
    fn phase_worker_thread(&mut self) -> bool {
        let clock = self.clock;
        let dest = self.dest().0 as isize;
        let normal = self.slots.first().and_then(|slot| {
            let placement = slot.placement.as_ref()?;
            Some((placement.thumb?, placement.placed?, placement.crop))
        });
        // Slot 2 keeps its registration until the worker reports that it
        // unregistered it, so a worker that cannot start, or never gets
        // there, leaves the slot's record and the clean-up right.
        let uncropped = self.slots.get(1).and_then(|slot| {
            let source = slot.source?.0 as isize;
            let old = slot.placement.as_ref().and_then(|p| p.thumb);
            Some((source, old, slot.rects.image, slot.plan.crop))
        });
        let mut steps: Vec<&'static str> = Vec::new();
        if normal.is_some() {
            steps.push("DwmUpdateThumbnailProperties(slot 1)");
        }
        if let Some((_, old, _, _)) = uncropped {
            if old.is_some() {
                steps.push("DwmUnregisterThumbnail(slot 2)");
            }
            steps.push(
                "slot 2's registration (DwmRegisterThumbnail, DwmQueryThumbnailSourceSize, \
                 DwmUpdateThumbnailProperties)",
            );
        }
        let (sender, events) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("probe-worker".to_owned())
            .spawn(move || {
                // A send fails only when the probe stopped listening; the
                // worker then has nobody to tell and just goes on.
                if let Some((thumb, placed, crop)) = normal {
                    let props = thumbnail_props(placed, crop);
                    let (result, micros) = clock.time(|| {
                        // SAFETY: `thumb` is a live registration of this
                        // process; `props` outlives the call.
                        unsafe { DwmUpdateThumbnailProperties(thumb, &props) }
                    });
                    let call = Call::of(
                        "worker: DwmUpdateThumbnailProperties(slot 1)",
                        &result,
                        micros,
                    );
                    let _ = sender.send(WorkerEvent::Call {
                        call,
                        unregistered: None,
                    });
                }
                if let Some((source, old, image, crop)) = uncropped {
                    if let Some(old) = old {
                        let mut call = unregister(clock, old);
                        call.api = "worker: DwmUnregisterThumbnail(slot 2)";
                        let _ = sender.send(WorkerEvent::Call {
                            call,
                            unregistered: Some(old),
                        });
                    }
                    let dest = HWND(dest as *mut c_void);
                    let source = HWND(source as *mut c_void);
                    let placement = register_into(clock, dest, source, image, crop);
                    let _ = sender.send(WorkerEvent::Placement(placement));
                }
                let _ = sender.send(WorkerEvent::Finished);
            });
        if let Err(e) = spawned {
            let why =
                format!("could not start a worker thread ({e}); slot 2 keeps its registration");
            say!("    {why}");
            self.results.push(format!("A6 worker thread: {why}"));
            return true;
        }
        // The thread is not joined: a worker stuck in a call must not stop
        // this one. Once finished it ends on its own; if it never does, the
        // process ending takes it along.

        let started = Instant::now();
        let mut returned = Vec::new();
        let mut done = 0;
        let end = loop {
            pump_pending();
            match events.recv_timeout(WORKER_SLICE) {
                Ok(WorkerEvent::Finished) => break WorkerEnd::Finished,
                Ok(event) => {
                    done += 1;
                    for line in self.take_worker_event(event) {
                        say!("    {line}");
                        returned.push(line);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break WorkerEnd::Vanished,
            }
            if quitting() {
                break WorkerEnd::Stopped;
            }
            if started.elapsed() >= WORKER_DEADLINE {
                break WorkerEnd::TimedOut;
            }
        };
        let line = worker_result(end, &returned, steps.get(done).copied(), started.elapsed());
        if end != WorkerEnd::Finished {
            say!("    {line}");
        }
        self.results.push(format!("A6 worker thread: {line}"));
        match end {
            WorkerEnd::Finished => true,
            WorkerEnd::Stopped => {
                self.late_worker = Some(events);
                say!("stopping: {}", quit_text());
                false
            }
            WorkerEnd::Vanished | WorkerEnd::TimedOut => {
                self.late_worker = Some(events);
                request_quit(Quit::WorkerStuck);
                emit(
                    "The probe stops here and prints its summary: later phases call DWM again \
                     and could hang the same way. The line above is the A6 answer.",
                );
                false
            }
        }
    }

    /// Applies one worker message to the slots and the call record; returns
    /// the lines to print.
    fn take_worker_event(&mut self, event: WorkerEvent) -> Vec<String> {
        match event {
            WorkerEvent::Call { call, unregistered } => {
                if let Some(thumb) = unregistered {
                    self.forget_thumb(1, thumb);
                }
                let line = call.describe();
                self.calls.push(call);
                vec![line]
            }
            WorkerEvent::Placement(mut placement) => {
                let calls = placement.calls();
                let lines = calls
                    .iter()
                    .map(|call| format!("worker: {}", call.describe()))
                    .collect();
                self.calls.extend(calls);
                placement.crop_note = format!("{} (worker thread)", placement.crop_note);
                if let Some(slot) = self.slots.get_mut(1) {
                    slot.placement = Some(placement);
                }
                lines
            }
            WorkerEvent::Finished => Vec::new(),
        }
    }

    /// Drops `thumb` from slot `index`'s record once it was unregistered.
    fn forget_thumb(&mut self, index: usize, thumb: isize) {
        if let Some(placement) = self
            .slots
            .get_mut(index)
            .and_then(|slot| slot.placement.as_mut())
            && placement.thumb == Some(thumb)
        {
            placement.thumb = None;
        }
    }

    /// Reads what a phase 2 worker that did not finish in time has sent
    /// since, so its registration of slot 2 is unregistered with the rest.
    fn collect_late_worker(&mut self) {
        let Some(events) = self.late_worker.take() else {
            return;
        };
        while let Ok(event) = events.try_recv() {
            for line in self.take_worker_event(event) {
                say!("late from the worker thread: {line}");
            }
        }
    }

    fn set_layered(&mut self, on: bool) -> String {
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

    fn update_all(&mut self) -> String {
        let clock = self.clock;
        let mut total = 0;
        let mut failed = Vec::new();
        for slot in &self.slots {
            let Some(call) = slot
                .placement
                .as_ref()
                .and_then(|placement| update_again(clock, placement))
            else {
                continue;
            };
            total += 1;
            say!("[{}] {}", slot.number, call.describe());
            if !call.ok() {
                failed.push(format!("slot {} {}", slot.number, call.short()));
            }
            self.calls.push(call);
        }
        if failed.is_empty() {
            format!("{total} of {total} updates ok")
        } else {
            format!(
                "{} of {total} updates ok; failed: {}",
                total - failed.len(),
                failed.join(", ")
            )
        }
    }

    fn summary(&self) {
        emit("");
        emit("==== thumbnail probe summary: copy from here to the end ====");
        say!(
            "Windows build {}; probe window DPI {} ({:.0} %); probe process {}",
            windows_build(),
            self.dpi,
            self.scale * 100.0,
            own_elevation()
        );
        let header = [
            "#",
            "source",
            "origin",
            "state",
            "register",
            "us",
            "size (DWM)",
            "window",
            "frame",
            "crop",
            "update",
            "us",
        ];
        let size_text = |size: Option<SIZE>| {
            size.map_or_else(|| "-".to_owned(), |s| format!("{}x{}", s.cx, s.cy))
        };
        let rect_size = |rect: Option<RECT>| {
            rect.map_or_else(|| "-".to_owned(), |r| format!("{}x{}", width(r), height(r)))
        };
        let rows: Vec<Vec<String>> = self
            .slots
            .iter()
            .map(|slot| {
                let origin = match (slot.plan.source, slot.source) {
                    (_, None) => "not found",
                    (Source::Own(_), Some(_)) => "own",
                    (Source::Find(_), Some(_)) => "found",
                    (Source::SecondOfFirst, Some(_)) => "slot 1",
                };
                let state = slot.source.map_or_else(|| "-".to_owned(), short_state);
                let placement = slot.placement.clone().unwrap_or_default();
                let call = |call: &Option<Call>| {
                    call.as_ref().map_or_else(
                        || ("-".to_owned(), "-".to_owned()),
                        |c| (c.short(), format!("{:.0}", c.micros)),
                    )
                };
                let (register, register_us) = call(&placement.register);
                let (update, update_us) = call(&placement.update);
                let crop = if placement.crop.is_some() {
                    "yes"
                } else if placement.register.is_some() {
                    "no"
                } else {
                    "-"
                };
                vec![
                    slot.number.to_string(),
                    slot.plan.key.to_owned(),
                    origin.to_owned(),
                    state,
                    register,
                    register_us,
                    size_text(placement.size),
                    rect_size(placement.window_rect),
                    rect_size(placement.frame),
                    crop.to_owned(),
                    update,
                    update_us,
                ]
            })
            .collect();
        for line in table(&header, &rows) {
            emit(&line);
        }
        emit(
            "(register, size, window, frame, crop and update are from each slot's last \
             registration; state is read now)",
        );
        for slot in &self.slots {
            if slot.plan.source != Source::SecondOfFirst {
                say!("slot {}: {}", slot.number, slot.origin);
            }
        }
        for line in timing_lines(&self.calls) {
            emit(&line);
        }
        for line in &self.results {
            emit(line);
        }
        if self.notes.is_empty() {
            emit("notes: none typed");
        }
        for (phase, note) in &self.notes {
            say!("note at {phase}: {note}");
        }
        emit("==== end of summary ====");
    }

    /// Unregisters every thumbnail, then destroys every window of its own
    /// and the classes. Runs once; also from `Drop`.
    fn cleanup(&mut self) {
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

/// Count, slowest and mean per API, and the calls over one frame (A6).
fn timing_lines(calls: &[Call]) -> Vec<String> {
    let mut apis: Vec<&'static str> = Vec::new();
    for call in calls {
        if !apis.contains(&call.api) {
            apis.push(call.api);
        }
    }
    let mut lines = Vec::new();
    for api in apis {
        let of: Vec<&Call> = calls.iter().filter(|call| call.api == api).collect();
        let slowest = of.iter().map(|call| call.micros).fold(0.0f64, f64::max);
        let mean = of.iter().map(|call| call.micros).sum::<f64>() / of.len().max(1) as f64;
        let failed = of.iter().filter(|call| !call.ok()).count();
        lines.push(format!(
            "A6 {api}: {} calls, {failed} failed, slowest {slowest:.0} us, mean {mean:.0} us",
            of.len()
        ));
    }
    let slow = calls
        .iter()
        .filter(|call| call.micros > SLOW_MICROS)
        .count();
    lines.push(if slow == 0 {
        format!(
            "A6: none of {} DWM calls took longer than 16 ms",
            calls.len()
        )
    } else {
        format!(
            "A6: {slow} of {} DWM calls took longer than 16 ms",
            calls.len()
        )
    });
    lines
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use stem_core::WindowId;

    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn fit_keeps_the_aspect_ratio_and_centres() {
        let slot = rect(10, 20, 330, 220);
        assert_eq!(fit(1600, 1000, slot), rect(10, 20, 330, 220));
        assert_eq!(fit(1000, 1000, slot), rect(70, 20, 270, 220));
        assert_eq!(fit(1920, 600, slot), rect(10, 70, 330, 170));
    }

    #[test]
    fn fit_never_scales_up_and_survives_bad_input() {
        let slot = rect(0, 0, 320, 200);
        assert_eq!(fit(100, 50, slot), rect(110, 75, 210, 125));
        assert_eq!(fit(0, 50, slot), slot);
        assert_eq!(fit(100, 50, rect(5, 5, 5, 5)), rect(5, 5, 5, 5));
    }

    #[test]
    fn crop_removes_the_invisible_borders() {
        let window = rect(100, 100, 1114, 708);
        let frame = rect(107, 100, 1107, 701);
        let source = SIZE { cx: 1014, cy: 608 };
        assert_eq!(
            crop_to_frame(window, frame, source, false),
            Ok(rect(7, 0, 1007, 601))
        );
    }

    #[test]
    fn crop_refuses_iconic_empty_and_outside_frames() {
        let source = SIZE { cx: 1014, cy: 608 };
        let iconic = rect(-32000, -32000, -31840, -31972);
        assert!(crop_to_frame(iconic, iconic, source, false).is_err());
        let window = rect(100, 100, 1114, 708);
        assert!(crop_to_frame(window, window, source, true).is_err());
        assert!(crop_to_frame(window, rect(107, 100, 107, 701), source, false).is_err());
        let small = SIZE { cx: 676, cy: 405 };
        assert!(crop_to_frame(window, rect(107, 100, 1107, 701), small, false).is_err());
    }

    #[test]
    fn props_set_the_source_rectangle_only_when_cropping() {
        let destination = rect(1, 2, 3, 4);
        let plain = thumbnail_props(destination, None);
        let flags = plain.dwFlags;
        assert_eq!(flags & DWM_TNP_RECTSOURCE, 0);
        assert_ne!(flags & DWM_TNP_VISIBLE, 0);
        let cropped = thumbnail_props(destination, Some(rect(7, 0, 100, 50)));
        let flags = cropped.dwFlags;
        let source = cropped.rcSource;
        assert_ne!(flags & DWM_TNP_RECTSOURCE, 0);
        assert_eq!(source, rect(7, 0, 100, 50));
    }

    fn inside(inner: RECT, outer: RECT) -> bool {
        inner.left >= outer.left
            && inner.top >= outer.top
            && inner.right <= outer.right
            && inner.bottom <= outer.bottom
    }

    fn overlap(a: RECT, b: RECT) -> bool {
        a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
    }

    fn check_layout(scale: f64, work: SIZE) {
        let grid = layout(scale, work, PLANS.len());
        assert_eq!(grid.slots.len(), PLANS.len());
        assert!(grid.client.cx <= work.cx && grid.client.cy <= work.cy);
        let client = rect(0, 0, grid.client.cx, grid.client.cy);
        for (index, slot) in grid.slots.iter().enumerate() {
            assert!(inside(slot.label, client) && inside(slot.image, client));
            assert!(slot.label.bottom <= slot.image.top);
            assert_eq!(width(slot.image) * 10 / 16, height(slot.image));
            for other in &grid.slots[index + 1..] {
                assert!(!overlap(slot.image, other.image));
                assert!(!overlap(slot.label, other.image));
            }
        }
    }

    #[test]
    fn layout_fits_common_work_areas() {
        check_layout(1.0, SIZE { cx: 1920, cy: 1040 });
        check_layout(1.25, SIZE { cx: 1920, cy: 1032 });
        check_layout(1.5, SIZE { cx: 1920, cy: 1032 });
        check_layout(2.0, SIZE { cx: 3840, cy: 2064 });
        check_layout(1.0, SIZE { cx: 1366, cy: 728 });
        let grid = layout(1.0, SIZE { cx: 1920, cy: 1040 }, PLANS.len());
        assert_eq!(grid.client, SIZE { cx: 1512, cy: 512 });
    }

    #[test]
    fn table_aligns_columns() {
        let rows = vec![
            vec!["1".to_owned(), "normal".to_owned()],
            vec!["10".to_owned(), "x".to_owned()],
        ];
        assert_eq!(
            table(&["#", "source"], &rows),
            vec![
                "#  | source".to_owned(),
                "---+-------".to_owned(),
                "1  | normal".to_owned(),
                "10 | x".to_owned(),
            ]
        );
    }

    #[test]
    fn cloak_values_are_named() {
        assert_eq!(cloak_text(None), "cloak ?");
        assert_eq!(cloak_text(Some(0)), "not cloaked");
        assert_eq!(cloak_text(Some(1)), "cloaked 0x1 (app)");
        assert_eq!(cloak_text(Some(6)), "cloaked 0x6 (shell+inherited)");
    }

    fn window(id: u64, class: &str, title: &str) -> RawWindow {
        RawWindow {
            id: WindowId(id),
            title: title.to_owned(),
            class_name: class.to_owned(),
            process_id: 100 + id as u32,
            exe_path: None,
            app_user_model_id: None,
            is_visible: true,
            is_minimized: false,
            is_cloaked: false,
            on_current_desktop: Some(true),
            monitor: None,
            rect: None,
            icon_key: None,
        }
    }

    #[test]
    fn picks_task_manager_on_this_desktop_first() {
        let mut elsewhere = window(1, TASK_MANAGER_CLASS, "Task Manager");
        elsewhere.on_current_desktop = Some(false);
        let here = window(2, TASK_MANAGER_CLASS, "Task Manager");
        let found = pick_external(&[elsewhere, here]);
        assert_eq!(found.elevated.map(|w| w.id), Some(WindowId(2)));
        assert_eq!(found.other_desktop.map(|w| w.id), Some(WindowId(1)));
        assert!(found.note.is_none());
    }

    #[test]
    fn picks_calculator_before_other_store_frames() {
        let mut frame = window(1, STORE_FRAME_CLASS, "Settings");
        frame.is_cloaked = true;
        let mut calculator = window(2, STORE_FRAME_CLASS, "Rechner");
        calculator.app_user_model_id = Some("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App".into());
        calculator.is_minimized = true;
        let found = pick_external(&[frame.clone(), calculator]);
        assert_eq!(found.store_app.map(|w| w.id), Some(WindowId(2)));
        let found = pick_external(&[frame, window(3, STORE_FRAME_CLASS, "Photos")]);
        assert_eq!(found.store_app.map(|w| w.id), Some(WindowId(1)));
    }

    #[test]
    fn picks_notepad_on_another_desktop_first() {
        let mut terminal = window(1, "CASCADIA_HOSTING_WINDOW_CLASS", "Terminal");
        terminal.on_current_desktop = Some(false);
        let mut notepad = window(2, "Notepad", "Untitled - Notepad");
        notepad.on_current_desktop = Some(false);
        notepad.exe_path = Some(PathBuf::from(r"C:\Windows\System32\notepad.exe"));
        let found = pick_external(&[terminal, notepad, window(3, "Notepad", "here")]);
        assert_eq!(found.other_desktop.map(|w| w.id), Some(WindowId(2)));
        assert!(found.elevated.is_none() && found.store_app.is_none());
    }

    #[test]
    fn says_when_the_shell_gave_no_desktop_facts() {
        let mut unknown = window(1, "Notepad", "a");
        unknown.on_current_desktop = None;
        let found = pick_external(&[unknown]);
        assert!(found.other_desktop.is_none());
        assert!(found.note.is_some());
    }

    #[test]
    fn timing_lines_flag_slow_calls() {
        let call = |api, micros| Call {
            api,
            hr: HRESULT(0),
            micros,
        };
        let lines = timing_lines(&[
            call("DwmRegisterThumbnail", 100.0),
            call("DwmRegisterThumbnail", 20_000.0),
            call("DwmUnregisterThumbnail", 10.0),
        ]);
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("2 calls") && lines[0].contains("slowest 20000 us"));
        assert_eq!(lines[2], "A6: 1 of 3 DWM calls took longer than 16 ms");
    }

    #[test]
    fn worker_result_names_the_step_that_did_not_return() {
        let returned = vec!["worker: DwmUpdateThumbnailProperties(slot 1) ok in 30 us".to_owned()];
        let waited = Duration::from_millis(5_004);
        assert_eq!(
            worker_result(WorkerEnd::Finished, &returned, None, waited),
            returned[0]
        );
        let stuck = worker_result(
            WorkerEnd::TimedOut,
            &returned,
            Some("DwmUnregisterThumbnail(slot 2)"),
            waited,
        );
        assert!(
            stuck.starts_with(
                "did not finish within 5004 ms: still in DwmUnregisterThumbnail(slot 2)"
            )
        );
        assert!(stuck.ends_with(
            "returned before: worker: DwmUpdateThumbnailProperties(slot 1) ok in 30 us"
        ));
        let stopped = worker_result(
            WorkerEnd::Stopped,
            &[],
            Some("slot 2's registration"),
            waited,
        );
        assert_eq!(
            stopped,
            "stopped waiting after 5004 ms: still in slot 2's registration; returned before: nothing"
        );
        let vanished = worker_result(WorkerEnd::Vanished, &[], None, waited);
        assert!(vanished.contains("panicked) in its last message"));
    }

    #[test]
    fn early_input_is_dropped_but_q_and_the_end_still_count() {
        let (sender, input) = mpsc::channel();
        assert_eq!(drain_early(&input), Drained::default());

        for line in ["", "a note"] {
            let _ = sender.send(Some(line.to_owned()));
        }
        assert_eq!(
            drain_early(&input),
            Drained {
                ignored: vec![String::new(), "a note".to_owned()],
                stop: None,
            }
        );

        for line in ["", "Q", "after q"] {
            let _ = sender.send(Some(line.to_owned()));
        }
        let drained = drain_early(&input);
        assert_eq!(drained.ignored, vec![String::new()]);
        assert_eq!(drained.stop, Some(Quit::Typed));

        let _ = sender.send(Some("after q".to_owned()));
        let _ = sender.send(None);
        drop(sender);
        let drained = drain_early(&input);
        assert_eq!(drained.stop, Some(Quit::StdinClosed));
        assert_eq!(drain_early(&input).stop, Some(Quit::StdinClosed));
    }
}
