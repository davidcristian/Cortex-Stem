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
//!   which the thread drives over [`Win32Shell`], the real messages:
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
//!   windows ([`pump_sent_messages`]); [`AppBarThread::stop`] does the same
//!   and ends the thread.
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
//! (they always "succeed"). So [`Win32Shell::present`] checks that the tray
//! window exists (`FindWindowW`, which sends nothing) before a registration
//! or a reposition; `TaskbarCreated` registers again once the shell is
//! back, and the retry's delay bounds the attempts meanwhile.

use std::sync::OnceLock;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TryRecvError, bounded, unbounded};
use parking_lot::Mutex;
use stem_core::Rect;
use stem_core::settings::DockPosition;
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::UI::Shell::{
    ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_ACTIVATE, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE,
    ABM_SETPOS, ABM_WINDOWPOSCHANGED, ABN_FULLSCREENAPP, ABN_POSCHANGED, ABN_STATECHANGE,
    ABN_WINDOWARRANGE, APPBARDATA, SHAppBarMessage,
};
use windows::Win32::UI::WindowsAndMessaging::{
    ChangeWindowMessageFilterEx, FindWindowW, MSG, MSGFLT_ALLOW, PM_NOREMOVE, PM_QS_SENDMESSAGE,
    PeekMessageW, RegisterWindowMessageW, WA_INACTIVE, WM_APP,
};
use windows::core::{PCWSTR, w};

use crate::rules::appbar::{AppBarShell, Bars, Notification};
use crate::{AppBarRequest, AppBarSink, NativeWindowHandle, PlatformError, PlatformResult};

/// The message the shell sends a registered dock window (the bar's
/// `uCallbackMessage`): `wParam` = the `ABN_*` code, `lParam` its argument.
/// The dock window's subclass hands it to [`on_callback`].
pub const CALLBACK_MESSAGE: u32 = WM_APP + 0x017;

/// How often [`release_all`] looks for sent messages while it waits.
const PUMP_INTERVAL: Duration = Duration::from_millis(5);
/// How long [`AppBarThread::stop`] waits for the thread to end.
const STOP_TIMEOUT: Duration = Duration::from_millis(200);

/// The running thread's queue, for the dock windows' subclass (which runs
/// on the main thread and must never wait); `None` while no thread runs.
/// Locked only to clone or send, never across a `SHAppBarMessage`.
static SENDER: Mutex<Option<Sender<Command>>> = Mutex::new(None);

/// The `TaskbarCreated` message id, registered on first use.
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();

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

/// Handle to the running `stem-appbar` thread. Dropping it without `stop`
/// asks the thread to end but does not wait for it; prefer `stop`.
pub struct AppBarThread {
    sender: Sender<Command>,
    /// Disconnects when the thread has ended.
    ended: Receiver<()>,
    join: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for AppBarThread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppBarThread")
            .field("running", &self.join.is_some())
            .finish()
    }
}

/// A clonable handle on the thread's queue, so a caller can wait on the
/// thread without holding the platform's lock ([`release_all`]).
#[derive(Clone)]
pub struct AppBarQueue(Sender<Command>);

impl std::fmt::Debug for AppBarQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AppBarQueue")
    }
}

impl AppBarThread {
    pub fn start() -> PlatformResult<Self> {
        let (sender, receiver) = unbounded::<Command>();
        let (ended_tx, ended_rx) = bounded::<()>(0);
        let join = std::thread::Builder::new()
            .name("stem-appbar".into())
            .spawn(move || {
                // Dropped when the thread ends, which disconnects `ended`.
                let _ended = ended_tx;
                thread_main(&receiver);
            })
            .map_err(|e| PlatformError::Other(format!("could not start the AppBar thread: {e}")))?;
        let created = taskbar_created_message();
        *SENDER.lock() = Some(sender.clone());
        log::debug!("AppBar thread started (TaskbarCreated = {created:#x})");
        Ok(Self {
            sender,
            ended: ended_rx,
            join: Some(join),
        })
    }

    /// Queue the reservation `request` wants for `handle` (`None`: give the
    /// band back), to be reported through `sink`. Returns at once.
    pub fn set(
        &self,
        handle: NativeWindowHandle,
        request: Option<AppBarRequest>,
        sink: AppBarSink,
    ) {
        let command = Command::Set {
            handle: handle.0,
            request,
            sink,
        };
        if self.sender.send(command).is_err() {
            log::warn!(
                "AppBar thread has ended; the request for {:#x} is dropped",
                handle.0
            );
        }
    }

    pub fn queue(&self) -> AppBarQueue {
        AppBarQueue(self.sender.clone())
    }

    /// Give every band back and end the thread, waiting at most
    /// [`STOP_TIMEOUT`] (handling sent messages meanwhile, as
    /// [`release_all`] does); a thread held by a hung Explorer is detached.
    pub fn stop(mut self) {
        *SENDER.lock() = None;
        if self.sender.send(Command::Stop).is_err() {
            log::debug!("AppBar thread already ended");
        }
        let deadline = Instant::now() + STOP_TIMEOUT;
        loop {
            pump_sent_messages();
            let now = Instant::now();
            if now >= deadline {
                log::warn!("AppBar thread did not stop within {STOP_TIMEOUT:?}; detaching it");
                return;
            }
            match self.ended.recv_timeout((deadline - now).min(PUMP_INTERVAL)) {
                Err(RecvTimeoutError::Timeout) => {}
                Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        if let Some(join) = self.join.take()
            && join.join().is_err()
        {
            log::error!("AppBar thread panicked");
        }
        log::debug!("AppBar thread stopped");
    }
}

impl Drop for AppBarThread {
    fn drop(&mut self) {
        if self.join.is_some() {
            *SENDER.lock() = None;
            let _ = self.sender.send(Command::Stop);
        }
    }
}

/// Give back every band the thread holds and wait at most `timeout` for the
/// shell to take them, dispatching the messages other threads send to the
/// calling thread's windows meanwhile ([`pump_sent_messages`]): on the main
/// thread at the quit, the shell may send the remaining bars'
/// `ABN_POSCHANGED` to the dock windows while it removes one, and must not
/// wait on a thread that waits on it. `Timeout` when the shell did not
/// finish in time.
pub fn release_all(queue: &AppBarQueue, timeout: Duration) -> PlatformResult<()> {
    let (done_tx, done_rx) = bounded::<()>(1);
    queue
        .0
        .send(Command::ReleaseAll(done_tx))
        .map_err(|_| PlatformError::Other("the AppBar thread has ended".into()))?;
    let deadline = Instant::now() + timeout;
    loop {
        pump_sent_messages();
        let now = Instant::now();
        if now >= deadline {
            log::warn!(
                "AppBar: the shell did not take the bands back within {timeout:?} (Explorer \
                 busy or hung); they are released if it answers before the process ends"
            );
            return Err(PlatformError::Timeout);
        }
        match done_rx.recv_timeout((deadline - now).min(PUMP_INTERVAL)) {
            Ok(()) => return Ok(()),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(PlatformError::Other(
                    "the AppBar thread ended without an answer".into(),
                ));
            }
        }
    }
}

/// Dispatch the messages other threads have sent to this thread's windows,
/// and nothing else: posted messages stay queued for the thread's own loop.
fn pump_sent_messages() {
    let mut msg = MSG::default();
    // SAFETY: `msg` is a valid out-pointer for the call. `PM_NOREMOVE |
    // PM_QS_SENDMESSAGE` makes the call dispatch pending sent (nonqueued)
    // messages to this thread's window procedures and remove nothing from
    // the posted queue; on a thread without windows it does nothing.
    let _ = unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE | PM_QS_SENDMESSAGE) };
}

// ---------------------------------------------------------------------------
// From the dock windows' subclass (the main thread): queue and return
// ---------------------------------------------------------------------------

/// Let the shell's AppBar messages ([`CALLBACK_MESSAGE`] and
/// `TaskbarCreated`) through UIPI to the dock window `hwnd`, so an elevated
/// Stem still hears them from the medium-integrity Explorer (module docs).
/// `dock_window::prepare` calls it for each dock window; it sends nothing
/// and a failure is logged, never fatal (the band then does not follow the
/// shell's changes while elevated).
pub fn allow_shell_messages(hwnd: HWND) {
    let messages = [
        (CALLBACK_MESSAGE, "the AppBar callback message"),
        (taskbar_created_message(), "TaskbarCreated"),
    ];
    for (message, name) in messages {
        if message == 0 {
            continue;
        }
        // SAFETY: changes the message filter of a window of this process;
        // no out-structure is passed. A dead handle only makes it fail.
        let allowed = unsafe { ChangeWindowMessageFilterEx(hwnd, message, MSGFLT_ALLOW, None) };
        match allowed {
            Ok(()) => log::trace!(
                "dock window {:#x}: {name} ({message:#x}) allowed through UIPI",
                raw(hwnd)
            ),
            Err(e) => log::debug!(
                "dock window {:#x}: could not let {name} ({message:#x}) through UIPI: {e}; an \
                 elevated Stem would not hear it",
                raw(hwnd)
            ),
        }
    }
}

/// Queue `command` for the thread, if one runs. Never waits.
fn forward(command: Command) {
    if let Some(sender) = SENDER.lock().as_ref() {
        let _ = sender.send(command);
    }
}

fn raw(hwnd: HWND) -> isize {
    hwnd.0 as isize
}

/// [`CALLBACK_MESSAGE`] reached a dock window.
pub fn on_callback(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
    forward(Command::Notify {
        handle: raw(hwnd),
        code: wparam.0 as u32,
        arg: lparam.0,
    });
}

/// `WM_ACTIVATE` reached a dock window (`wParam`'s low word: `WA_*`).
pub fn on_activate(hwnd: HWND, wparam: WPARAM) {
    forward(Command::Activate {
        handle: raw(hwnd),
        active: (wparam.0 & 0xFFFF) as u32 != WA_INACTIVE,
    });
}

/// `WM_WINDOWPOSCHANGED` reached a dock window.
pub fn on_window_pos_changed(hwnd: HWND) {
    forward(Command::WindowPosChanged(raw(hwnd)));
}

/// Whether `msg` is `TaskbarCreated` (known once a dock window was prepared
/// or the thread started; before that no bar exists that would need it).
pub fn is_taskbar_created(msg: u32) -> bool {
    TASKBAR_CREATED
        .get()
        .is_some_and(|&id| id != 0 && id == msg)
}

/// `TaskbarCreated` reached a dock window: the shell started again.
pub fn on_taskbar_created(hwnd: HWND) {
    forward(Command::TaskbarCreated(raw(hwnd)));
}

/// A dock window is being destroyed (`WM_NCDESTROY`).
pub fn on_destroyed(hwnd: HWND) {
    forward(Command::Destroyed(raw(hwnd)));
}

/// The id of the `TaskbarCreated` message the shell broadcasts to every
/// top-level window when it starts; 0 when it could not be registered.
fn taskbar_created_message() -> u32 {
    *TASKBAR_CREATED.get_or_init(|| {
        // SAFETY: a valid NUL-terminated string; the call registers (or
        // looks up) a message name and sends nothing.
        unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) }
    })
}

// ---------------------------------------------------------------------------
// The thread
// ---------------------------------------------------------------------------

/// The thread's state: the bookkeeping (`rules::appbar::Bars`) and the
/// clock it runs on.
struct Worker {
    bars: Bars,
    started: Instant,
}

fn thread_main(receiver: &Receiver<Command>) {
    let mut worker = Worker {
        bars: Bars::default(),
        started: Instant::now(),
    };
    loop {
        let first = match worker.bars.next_due() {
            Some(due) => {
                let wait = Duration::from_millis(due.saturating_sub(worker.now_ms()));
                match receiver.recv_timeout(wait) {
                    Ok(command) => Some(command),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => {
                        worker.bars.release_all(&mut Win32Shell);
                        break;
                    }
                }
            }
            None => match receiver.recv() {
                Ok(command) => Some(command),
                Err(_) => {
                    worker.bars.release_all(&mut Win32Shell);
                    break;
                }
            },
        };
        // Everything queued meanwhile, so that a window's position changes
        // become one `ABM_WINDOWPOSCHANGED`.
        let mut batch: Vec<Command> = first.into_iter().collect();
        loop {
            match receiver.try_recv() {
                Ok(command) => batch.push(command),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    worker.bars.release_all(&mut Win32Shell);
                    return;
                }
            }
        }
        if worker.run(batch) {
            break;
        }
        let now = worker.now_ms();
        worker.bars.tick(&mut Win32Shell, now);
    }
    log::debug!("AppBar thread ends");
}

impl Worker {
    fn now_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// Handle one batch of commands in order; `true` to end the thread.
    fn run(&mut self, batch: Vec<Command>) -> bool {
        let shell = &mut Win32Shell;
        let mut moved: Vec<isize> = Vec::new();
        for command in batch {
            match command {
                Command::Set {
                    handle,
                    request,
                    sink,
                } => {
                    let now = self.now_ms();
                    self.bars.set(shell, handle, request, sink, now);
                }
                Command::Notify { handle, code, arg } => {
                    let now = self.now_ms();
                    self.bars
                        .notify(shell, handle, notification(code, arg), now);
                }
                Command::Activate { handle, active } => self.bars.activate(shell, handle, active),
                Command::WindowPosChanged(handle) => {
                    if !moved.contains(&handle) {
                        moved.push(handle);
                    }
                }
                Command::TaskbarCreated(handle) => {
                    let now = self.now_ms();
                    self.bars.shell_restarted(shell, handle, now);
                }
                Command::Destroyed(handle) => self.bars.destroyed(shell, handle),
                Command::ReleaseAll(done) => {
                    self.bars.release_all(shell);
                    let _ = done.send(());
                }
                Command::Stop => {
                    self.bars.release_all(shell);
                    return true;
                }
            }
        }
        for handle in moved {
            self.bars.window_pos_changed(shell, handle);
        }
        false
    }
}

/// The callback message's `wParam` / `lParam` as a notification.
fn notification(code: u32, arg: isize) -> Notification {
    match code {
        ABN_POSCHANGED => Notification::PosChanged,
        ABN_STATECHANGE => Notification::StateChange,
        ABN_FULLSCREENAPP => Notification::FullScreenApp(arg != 0),
        ABN_WINDOWARRANGE => Notification::WindowArrange(arg != 0),
        other => Notification::Other(other),
    }
}

/// The real shell: one `SHAppBarMessage` per call. The AppBar thread only.
#[derive(Debug, Clone, Copy)]
struct Win32Shell;

impl AppBarShell for Win32Shell {
    /// Whether the tray window exists. Reads the window list; sends nothing.
    fn present(&mut self) -> bool {
        // SAFETY: both arguments are a valid NUL-terminated string or null.
        unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }.is_ok()
    }

    fn register(&mut self, handle: isize) -> bool {
        send(ABM_NEW, &mut app_bar_data(handle)) != 0
    }

    /// `ABM_REMOVE` always "succeeds".
    fn remove(&mut self, handle: isize) {
        send(ABM_REMOVE, &mut app_bar_data(handle));
    }

    fn query(&mut self, handle: isize, edge: DockPosition, rect: Rect) -> Rect {
        let mut data = app_bar_data(handle);
        data.uEdge = abe(edge);
        data.rc = to_rect(rect);
        send(ABM_QUERYPOS, &mut data);
        from_rect(data.rc)
    }

    fn set_pos(&mut self, handle: isize, edge: DockPosition, rect: Rect) -> Rect {
        let mut data = app_bar_data(handle);
        data.uEdge = abe(edge);
        data.rc = to_rect(rect);
        send(ABM_SETPOS, &mut data);
        from_rect(data.rc)
    }

    fn activate(&mut self, handle: isize, active: bool) {
        let mut data = app_bar_data(handle);
        data.lParam = LPARAM(isize::from(active));
        send(ABM_ACTIVATE, &mut data);
    }

    fn window_pos_changed(&mut self, handle: isize) {
        send(ABM_WINDOWPOSCHANGED, &mut app_bar_data(handle));
    }
}

/// One `SHAppBarMessage`. The AppBar thread only.
fn send(message: u32, data: &mut APPBARDATA) -> usize {
    // SAFETY: `data` is a complete `APPBARDATA` with `cbSize` set, alive and
    // exclusively borrowed for the call; the shell copies what it reads,
    // writes `rc` back for ABM_QUERYPOS / ABM_SETPOS, and keeps no pointer.
    unsafe { SHAppBarMessage(message, data) }
}

/// An `APPBARDATA` for the bar of the window `handle`.
fn app_bar_data(handle: isize) -> APPBARDATA {
    APPBARDATA {
        cbSize: size_of::<APPBARDATA>() as u32,
        hWnd: HWND(handle as *mut core::ffi::c_void),
        uCallbackMessage: CALLBACK_MESSAGE,
        ..Default::default()
    }
}

fn abe(edge: DockPosition) -> u32 {
    match edge {
        DockPosition::Bottom => ABE_BOTTOM,
        DockPosition::Top => ABE_TOP,
        DockPosition::Left => ABE_LEFT,
        DockPosition::Right => ABE_RIGHT,
    }
}

fn to_rect(rect: Rect) -> RECT {
    RECT {
        left: rect.x,
        top: rect.y,
        right: rect.right(),
        bottom: rect.bottom(),
    }
}

fn from_rect(rect: RECT) -> Rect {
    Rect::new(
        rect.left,
        rect.top,
        rect.right.saturating_sub(rect.left),
        rect.bottom.saturating_sub(rect.top),
    )
}
