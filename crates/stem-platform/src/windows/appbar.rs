//! AppBar registration: the screen space the dock windows reserve with
//! `SHAppBarMessage` while *Reserve screen space* is on.
//!
//! Contract:
//! - [`AppBarThread`] owns a thread (`stem-appbar`) that sends **every**
//!   `SHAppBarMessage` for the dock windows' bars, in order. The platform
//!   calls only queue commands for it ([`AppBarThread::set`]) and return at
//!   once, so they are safe on any thread, the Tauri main thread included.
//! - What the thread sends for which window, and what it reports, is
//!   decided by `rules::appbar::Bars` (pure, unit-tested on every host),
//!   which the thread drives over
//!   [`Win32Shell`](win32_shell::Win32Shell), the real messages:
//!   - a request for a window waits until no newer one has come for
//!     `rules::appbar::Settle::DELAY_MS`, so a burst of reports costs one
//!     `ABM_SETPOS`; a release (`None`) is `ABM_REMOVE` at once;
//!   - a request takes `rules::appbar::steps`: `ABM_NEW` for a first one
//!     (with [`CALLBACK_MESSAGE`] as the window's callback message),
//!     `ABM_QUERYPOS` with the band on the request's edge of the monitor's
//!     **bounds**, `ABM_SETPOS` with the shell's adjustment cut back to the
//!     thickness; another edge or monitor is `ABM_REMOVE` first;
//!   - the band the shell answered goes to the window's sink after every
//!     request applied; a band that could not be taken (no shell, `ABM_NEW`
//!     refused, an answer that is not a band on the monitor) reports `None`
//!     and is asked for again after a growing delay
//!     (`rules::appbar::Retry`) for as long as the window wants it.
//! - The shell talks to a bar through its window: [`CALLBACK_MESSAGE`]
//!   (`wParam` = `ABN_*`), and the window must report `WM_ACTIVATE`
//!   (`ABM_ACTIVATE`) and `WM_WINDOWPOSCHANGED` (`ABM_WINDOWPOSCHANGED`),
//!   as Microsoft's AppBar documentation requires. The dock windows'
//!   subclass (`dock_window`, on the main thread) hands each of those to
//!   [`on_callback`], [`on_activate`] and [`on_window_pos_changed`], which
//!   only queue a command here and return. `ABN_POSCHANGED` and
//!   `ABN_STATECHANGE` ask `ABM_QUERYPOS` again and send `ABM_SETPOS` only
//!   when the answer differs from the band the bar asked for last, within
//!   the bound of `rules::appbar::RequeryGuard` (a band moved during its
//!   pause is asked for again when the pause ends); `ABN_FULLSCREENAPP` is
//!   a debug line (fullscreen is not handled here); `ABN_WINDOWARRANGE` a
//!   trace line.
//! - `TaskbarCreated` (the shell started again: an Explorer restart drops
//!   every registration) registers each bar again ([`on_taskbar_created`]);
//!   `WM_NCDESTROY` removes the window's bar ([`on_destroyed`]).
//! - [`allow_shell_messages`] lets the callback message and
//!   `TaskbarCreated` through UIPI to a dock window, for an elevated Stem
//!   (below).
//! - [`release_all`] gives every band back and waits, bounded, while
//!   handling the messages other threads send to the calling thread's
//!   windows ([`pump_sent_messages`](handle::pump_sent_messages));
//!   [`AppBarThread::stop`] does the same and ends the thread.
//!
//! # Threads and the deadlock this avoids
//!
//! `SHAppBarMessage` is a `SendMessage` to the tray window (`Shell_TrayWnd`)
//! with no timeout, and while Explorer handles `ABM_SETPOS` or
//! `ABM_REMOVE` it may *send* messages to the bars' windows (their
//! `ABN_POSCHANGED`) and resizes maximised windows. So the messages run on
//! a thread that owns no window and that nothing waits for: the main thread
//! (which owns the dock windows) never waits on this thread, except at the
//! quit, where [`release_all`] keeps dispatching sent messages while it
//! waits. A main thread blocked on a helper that waits on Explorer that
//! waits on the main thread would be a deadlock. A hung Explorer holds only
//! this thread; the commands queue behind it and the next settle merges
//! them.
//!
//! `SHAppBarMessage` takes the bar's window as a value in its payload and
//! the shell addresses its notifications to that window, whichever thread
//! sent the message; nothing in Microsoft's documentation ties the call to
//! the window's own thread.
//!
//! # An elevated Stem (UIPI)
//!
//! Explorer runs at medium integrity. When Stem runs elevated (*Run as
//! administrator*), User Interface Privilege Isolation drops the messages a
//! lower-integrity process sends or posts to its windows from `WM_USER` up,
//! registered messages included, unless the window lets them through. Both
//! the callback message (`WM_APP + 0x17`) and `TaskbarCreated` are in that
//! range, so without [`allow_shell_messages`] an elevated dock would never
//! hear `ABN_POSCHANGED` (its band would not follow the taskbar's auto-hide
//! or other bars) nor re-register after an Explorer restart. The
//! `SHAppBarMessage` calls themselves go from the higher level to the
//! lower one and are not filtered. The filter is changed for those two
//! messages only, on each dock window, at any integrity level: at medium
//! integrity it only lets a low-integrity process send them too, which
//! costs at most a bounded requery.
//!
//! # No tray
//!
//! With Explorer gone (crashed, restarting) there is no `Shell_TrayWnd`,
//! `ABM_NEW` fails and `ABM_QUERYPOS` / `ABM_SETPOS` would answer nothing
//! (they always "succeed"). So
//! [`Win32Shell::present`](win32_shell::Win32Shell#method.present) checks that
//! the tray window exists (`FindWindowW`, which sends nothing) before a
//! registration or a reposition; `TaskbarCreated` registers again once the
//! shell is back, and the retry's delay bounds the attempts meanwhile.

use crossbeam_channel::Sender;
use parking_lot::Mutex;
use windows::Win32::UI::WindowsAndMessaging::WM_APP;

use crate::{AppBarRequest, AppBarSink};

mod handle;
mod subclass;
mod thread;
mod win32_shell;

pub use handle::{AppBarQueue, AppBarThread, release_all};
pub use subclass::{
    allow_shell_messages, is_taskbar_created, on_activate, on_callback, on_destroyed,
    on_taskbar_created, on_window_pos_changed,
};

/// The message the shell sends a registered dock window (the bar's
/// `uCallbackMessage`): `wParam` = the `ABN_*` code, `lParam` its argument.
/// The dock window's subclass hands it to [`on_callback`].
pub const CALLBACK_MESSAGE: u32 = WM_APP + 0x017;

/// The running thread's queue, for the dock windows' subclass (which runs
/// on the main thread and must never wait); `None` while no thread runs.
/// Locked only to clone or send, never across a `SHAppBarMessage`.
static SENDER: Mutex<Option<Sender<Command>>> = Mutex::new(None);

/// What the thread is asked to do.
enum Command {
    /// The latest reservation a window wants ([`AppBarThread::set`]).
    Set {
        handle: isize,
        request: Option<AppBarRequest>,
        sink: AppBarSink,
    },
    /// A notification from the shell ([`CALLBACK_MESSAGE`]).
    Notify {
        handle: isize,
        code: u32,
        arg: isize,
    },
    /// `WM_ACTIVATE` on a dock window.
    Activate { handle: isize, active: bool },
    /// `WM_WINDOWPOSCHANGED` on a dock window.
    WindowPosChanged(isize),
    /// `TaskbarCreated` reached a dock window: the shell started again.
    TaskbarCreated(isize),
    /// `WM_NCDESTROY` on a dock window.
    Destroyed(isize),
    /// Give every band back, then answer.
    ReleaseAll(Sender<()>),
    /// Give every band back and end.
    Stop,
}
