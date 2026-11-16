//! Phase 2 (A6): the DWM calls made on a worker thread, and the wait for
//! it.

use std::ffi::c_void;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::DwmUpdateThumbnailProperties;

use super::dwm::{Placement, register_into, unregister};
use super::geometry::thumbnail_props;
use super::own_windows::pump_pending;
use super::probe::Probe;
use super::quit::{Quit, quit_text, quitting, request_quit};
use super::timing::Call;
use super::{WORKER_DEADLINE, WORKER_SLICE, emit};

/// What the phase 2 worker sends back: one message per finished step, so a
/// call that never returns shows as the step that is missing.
pub(super) enum WorkerEvent {
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
pub(super) enum WorkerEnd {
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
pub(super) fn worker_result(
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

impl Probe {
    /// Phase 2 (A6): a worker thread updates slot 1, unregisters slot 2 and
    /// registers it again. This thread waits at most `WORKER_DEADLINE` for
    /// it, in short slices, and keeps pumping its windows' messages
    /// meanwhile (as the dock's UI thread would), so neither a worker call
    /// that needs the destination's thread nor one that blocks can hang the
    /// probe. Returns false when the worker did not finish: the probe then
    /// stops, since later phases call DWM again.
    pub(super) fn phase_worker_thread(&mut self) -> bool {
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
    pub(super) fn collect_late_worker(&mut self) {
        let Some(events) = self.late_worker.take() else {
            return;
        };
        while let Ok(event) = events.try_recv() {
            for line in self.take_worker_event(event) {
                say!("late from the worker thread: {line}");
            }
        }
    }
}
