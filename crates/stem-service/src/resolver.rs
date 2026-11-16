//! The identity thread (`dock-identity`): it asks the platform who an app
//! is, retries a failure once, and re-identifies stale pins at start.

use std::collections::VecDeque;
use std::sync::Weak;
use std::sync::atomic::Ordering;
use std::time::Instant;

use crossbeam_channel::{Receiver, RecvTimeoutError};
use stem_core::{AppId, AppIdentity, EngineInput};

use crate::{IDENTITY_RETRY_DELAY, Inner, ResolveMsg, WorkerMsg, reidentify};

impl Inner {
    /// Give every stale `exe:` pin of a packaged app (settings written by
    /// older builds) the packaged app's `aumid:` id and a `StoreApp` target,
    /// in place, or drop it when that id is already pinned. Runs once per
    /// start, first thing on the identity thread (never on the UI thread and
    /// never on the worker, which does not wait on the OS). Each candidate is
    /// asked about without any lock held; everything that changed is saved
    /// in one `mutate_settings`, with one info line per pin. A pin the
    /// platform cannot identify, or an error, leaves the pin as it is.
    fn reidentify_stale_pins(&self) {
        let candidates: Vec<stem_core::settings::PinnedApp> = self
            .settings
            .lock()
            .pinned
            .iter()
            .filter(|p| reidentify::is_candidate(p))
            .cloned()
            .collect();
        let mut found = Vec::new();
        let mut identities = Vec::new();
        for pin in &candidates {
            if self.stopping.load(Ordering::SeqCst) {
                return;
            }
            match self.platform.identify_target(&pin.launch) {
                Ok(identity) => match reidentify::reidentified(pin, &identity) {
                    Some(entry) => {
                        identities.push((entry.new_id(), identity));
                        found.push(entry);
                    }
                    None => log::debug!(
                        "pin {} is not a packaged app's ({}); left alone",
                        pin.id.as_str(),
                        identity.id.as_str()
                    ),
                },
                Err(e) => log::debug!(
                    "re-identifying pin {}: identify_target failed: {e}; left alone",
                    pin.id.as_str()
                ),
            }
        }
        if found.is_empty() || self.stopping.load(Ordering::SeqCst) {
            return;
        }

        let mut rewrites = Vec::new();
        let saved = self.mutate_settings(|settings| {
            rewrites = reidentify::rewrite_pins(&mut settings.pinned, &found);
            // Make the new pins' identities known before they render, as
            // `pin_target` does: queued ahead of the `SettingsChanged` that
            // `mutate_settings` sends.
            for rewrite in &rewrites {
                if let reidentify::PinRewrite::Replaced { new, .. } = rewrite
                    && let Some((_, identity)) = identities.iter().find(|(id, _)| id == new)
                {
                    let _ = self
                        .worker_tx
                        .send(WorkerMsg::Input(EngineInput::IdentityResolved {
                            app_id: new.clone(),
                            identity: identity.clone(),
                        }));
                }
            }
            !rewrites.is_empty()
        });
        match saved {
            Ok(_) => {
                for rewrite in &rewrites {
                    log::info!("{rewrite}");
                }
            }
            Err(e) => log::warn!(
                "re-identified {} stale pin(s) but could not save the settings: {e}; they stay as they were",
                rewrites.len()
            ),
        }
    }

    /// Ask the platform who `app_id` is and feed the answer to the engine.
    ///
    /// Returns `Some(due)` instead when every attempted platform call failed
    /// and `may_retry` is set: the caller schedules a retry for `due`. On the
    /// retry a fallback identity built from what the engine already knows is
    /// sent, so the engine stops asking. When there is nothing to ask (the
    /// app has neither a window nor a pin any more) the engine is told
    /// `IdentityUnavailable` instead and will ask again if the app returns.
    fn resolve_identity(&self, app_id: &AppId, may_retry: bool) -> Option<Instant> {
        let pinned = self
            .settings
            .lock()
            .pinned
            .iter()
            .find(|p| &p.id == app_id)
            .cloned();
        let window = self.engine.lock().windows_of(app_id).first().copied();

        let mut tried = false;
        let mut resolved: Option<AppIdentity> = None;
        if let Some(id) = window {
            tried = true;
            match self.platform.identify_window(id) {
                // The platform reads the window's facts again and derives
                // the id the engine would (`app_id_for`): another id means
                // the facts changed under the request (an AppUserModelID
                // that showed up late), so this identity is another app's
                // and must not be stored under `app_id`. The engine regroups
                // the window once it sees the new facts; until then this
                // counts as unresolved (the pin, the retry, the fallback).
                Ok(identity) if identity.id != *app_id => log::debug!(
                    "window {id:?} of {app_id:?} now identifies as {:?}; not stored under {app_id:?}",
                    identity.id
                ),
                Ok(identity) => resolved = Some(identity),
                Err(e) => log::debug!("identify_window({id:?}) for {app_id:?} failed: {e}"),
            }
        }
        if resolved.is_none()
            && let Some(p) = &pinned
        {
            tried = true;
            match self.platform.identify_target(&p.launch) {
                Ok(identity) => resolved = Some(identity),
                Err(e) => {
                    log::debug!("identify_target({:?}) for {app_id:?} failed: {e}", p.launch);
                }
            }
        }
        if !tried {
            log::debug!(
                "identity of {app_id:?} not needed: the app has no window and is not pinned"
            );
            let _ = self
                .worker_tx
                .send(WorkerMsg::Input(EngineInput::IdentityUnavailable {
                    app_id: app_id.clone(),
                }));
            return None;
        }
        if resolved.is_none() && may_retry {
            log::debug!("identity of {app_id:?} unresolved; retrying in {IDENTITY_RETRY_DELAY:?}");
            return Some(Instant::now() + IDENTITY_RETRY_DELAY);
        }

        let mut identity = resolved.unwrap_or_else(|| {
            log::warn!("identity of {app_id:?} could not be resolved; using a fallback name");
            AppIdentity {
                id: app_id.clone(),
                display_name: pinned
                    .as_ref()
                    .and_then(|p| p.label.clone())
                    .unwrap_or_else(|| fallback_display_name(app_id)),
                icon_key: None,
                launch: pinned.as_ref().map(|p| p.launch.clone()),
            }
        });
        // The engine groups by the id *it* derived; keep that as the key even
        // if the platform preferred a different canonical form for the pin's
        // target (a window's identity with another id never gets here).
        identity.id = app_id.clone();
        if identity.launch.is_none() {
            identity.launch = pinned.map(|p| p.launch);
        }
        let _ = self
            .worker_tx
            .send(WorkerMsg::Input(EngineInput::IdentityResolved {
                app_id: app_id.clone(),
                identity,
            }));
        None
    }
}

pub(crate) fn fallback_display_name(app_id: &AppId) -> String {
    let s = app_id.as_str();
    let tail = s.rsplit(['/', '\\', ':']).next().unwrap_or(s);
    let tail = tail.strip_suffix(".exe").unwrap_or(tail);
    if tail.is_empty() {
        s.to_owned()
    } else {
        tail.to_owned()
    }
}

/// The identity thread: re-identify stale pins once, then resolve
/// requests as they come, keep failed ones in a queue of timed retries,
/// stop on `Shutdown` or when the service stops.
pub(crate) fn resolver_loop(rx: &Receiver<ResolveMsg>, inner: &Weak<Inner>) {
    if let Some(inner) = inner.upgrade() {
        inner.reidentify_stale_pins();
    }
    let mut retries: VecDeque<(Instant, AppId)> = VecDeque::new();

    'run: loop {
        let msg = match retries.front() {
            Some((due, _)) => match rx.recv_deadline(*due) {
                Ok(msg) => Some(msg),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match rx.recv() {
                Ok(msg) => Some(msg),
                Err(_) => break,
            },
        };
        let Some(inner) = inner.upgrade() else { break };
        if inner.stopping.load(Ordering::SeqCst) {
            break;
        }
        match msg {
            Some(ResolveMsg::Shutdown) => break,
            Some(ResolveMsg::Resolve(app_id)) => {
                if let Some(due) = inner.resolve_identity(&app_id, true) {
                    retries.push_back((due, app_id));
                }
            }
            None => {
                let now = Instant::now();
                while retries.front().is_some_and(|(due, _)| *due <= now) {
                    if inner.stopping.load(Ordering::SeqCst) {
                        break 'run;
                    }
                    if let Some((_, app_id)) = retries.pop_front() {
                        inner.resolve_identity(&app_id, false);
                    }
                }
            }
        }
    }
    log::debug!("dock-identity resolver stopped");
}
