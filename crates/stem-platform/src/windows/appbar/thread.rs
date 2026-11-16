//! The `stem-appbar` thread: it takes the queued commands in batches and
//! drives `rules::appbar::Bars` over the real shell.

use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, TryRecvError};
use windows::Win32::UI::Shell::{
    ABN_FULLSCREENAPP, ABN_POSCHANGED, ABN_STATECHANGE, ABN_WINDOWARRANGE,
};

use super::Command;
use super::win32_shell::Win32Shell;
use crate::rules::appbar::{Bars, Notification};

/// The thread's state: the bookkeeping (`rules::appbar::Bars`) and the
/// clock it runs on.
struct Worker {
    bars: Bars,
    started: Instant,
}

pub(super) fn thread_main(receiver: &Receiver<Command>) {
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
