//! Console input: the stdin reader thread, waiting for a line, and
//! dropping the lines typed before a pause.

use std::io::BufRead;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use windows::Win32::System::Console::{FlushConsoleInputBuffer, GetStdHandle, STD_INPUT_HANDLE};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, TranslateMessage,
};

use super::own_windows::pump_for;
use super::quit::{Quit, quitting, request_quit, wake};
use super::{PAUSE_SETTLE, emit};

pub(super) enum Step {
    Line(String),
    Stop,
}

/// Runs the message loop until a line arrives on stdin or the probe is
/// asked to stop.
pub(super) fn wait_for_line(input: &Receiver<Option<String>>) -> Step {
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
pub(super) fn spawn_stdin_reader(dest: isize) -> Receiver<Option<String>> {
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
pub(super) fn flush_console_input() {
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
pub(super) struct Drained {
    /// Lines typed before the pause asked for one; they answer nothing.
    pub(super) ignored: Vec<String>,
    /// `q` among them, or the end of input.
    pub(super) stop: Option<Quit>,
}

/// Takes every line already waiting, without blocking. `q` still stops the
/// probe and the end of input still ends it; any other line is ignored.
pub(super) fn drain_early(input: &Receiver<Option<String>>) -> Drained {
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
pub(super) fn settle_input(input: &Receiver<Option<String>>) {
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
