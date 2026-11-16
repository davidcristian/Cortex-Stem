//! The worker thread (`stem-service`): it applies the queued inputs to the
//! engine in batches and notifies the listeners.

use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Weak};
use std::time::Instant;

use crossbeam_channel::{Receiver, RecvTimeoutError};
use stem_core::{EngineInput, PlatformEvent};

use crate::{Inner, Listener, ResolveMsg, STATE_EMIT_INTERVAL, ServiceEvent, WorkerMsg};

impl Inner {
    /// Call every listener with `event`. A panicking listener is logged and
    /// skipped; the others still run.
    pub(crate) fn notify(&self, event: &ServiceEvent) {
        let listeners: Vec<Arc<Listener>> = self.listeners.lock().clone();
        for listener in listeners {
            if let Err(payload) = std::panic::catch_unwind(AssertUnwindSafe(|| listener(event))) {
                log::error!(
                    "a service listener panicked on the {} event: {}",
                    event.name(),
                    panic_message(payload.as_ref())
                );
            }
        }
    }

    /// Apply a batch of inputs, hand new identity requests to the identity
    /// thread, and notify listeners of theme / monitor / settings changes
    /// (the last value of each per batch). Returns whether the snapshot
    /// changed; the caller decides when to emit it. "Changed" is the
    /// engine's verdict per input, by snapshot comparison, so a
    /// `WindowUpdated` that only moves a window counts exactly when the move
    /// flips some dock's `overlapped`, and a `DockPanelChanged` the
    /// same way.
    fn process_batch(&self, inputs: Vec<EngineInput>) -> bool {
        let mut changed = false;
        let mut settings_changed = false;
        let mut theme = None;
        let mut monitors = None;
        let mut unresolved = Vec::new();

        for input in inputs {
            match &input {
                EngineInput::Platform(PlatformEvent::ThemeChanged(t)) => theme = Some(t.clone()),
                EngineInput::Platform(PlatformEvent::MonitorsChanged(m)) => {
                    monitors = Some(m.clone());
                }
                EngineInput::SettingsChanged(_) => settings_changed = true,
                _ => {}
            }
            // Lock per input so commands never wait for a whole batch.
            let outcome = self.engine.lock().apply(input);
            changed |= outcome.changed;
            unresolved.extend(outcome.unresolved_apps);
        }

        for app_id in unresolved {
            let _ = self.resolve_tx.send(ResolveMsg::Resolve(app_id));
        }
        if let Some(t) = theme {
            self.notify(&ServiceEvent::Theme(t));
        }
        if let Some(m) = monitors {
            self.notify(&ServiceEvent::Monitors(m));
        }
        if settings_changed {
            let settings = self.engine.lock().settings().clone();
            self.notify(&ServiceEvent::Settings(settings));
        }
        changed
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("<non-string panic payload>")
}

/// The worker: drain everything queued, apply it, emit at most one `State`
/// per [`STATE_EMIT_INTERVAL`] (always the latest), stop on `Shutdown`.
pub(crate) fn worker_loop(rx: &Receiver<WorkerMsg>, inner: &Weak<Inner>) {
    let mut last_emit: Option<Instant> = None;
    // A changed snapshot is waiting for the rate limit to let it out.
    let mut state_pending = false;

    loop {
        let first = if state_pending {
            let deadline = last_emit.map_or_else(Instant::now, |t| t + STATE_EMIT_INTERVAL);
            match rx.recv_deadline(deadline) {
                Ok(msg) => Some(msg),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(msg) => Some(msg),
                Err(_) => break,
            }
        };

        // Batch: the message we waited for plus everything already queued.
        let mut inputs = Vec::new();
        let mut shutdown = false;
        let queued = first
            .into_iter()
            .chain(std::iter::from_fn(|| rx.try_recv().ok()));
        for msg in queued {
            match msg {
                WorkerMsg::Input(input) => inputs.push(input),
                WorkerMsg::Shutdown => {
                    shutdown = true;
                    break;
                }
            }
        }

        let Some(inner) = inner.upgrade() else { break };
        state_pending |= inner.process_batch(inputs);
        if state_pending {
            let now = Instant::now();
            let may_emit = shutdown || last_emit.is_none_or(|t| now >= t + STATE_EMIT_INTERVAL);
            if may_emit {
                let snapshot = inner.engine.lock().snapshot();
                inner.notify(&ServiceEvent::State(snapshot));
                last_emit = Some(now);
                state_pending = false;
            }
        }
        if shutdown {
            break;
        }
    }
    log::debug!("stem-service worker stopped");
}
