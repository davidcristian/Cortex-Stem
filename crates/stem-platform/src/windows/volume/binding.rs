//! Following the default output device: binding it, and planning the retry
//! after a failed bind.

use super::endpoint::{BindError, DeviceWatch, bind, default_endpoint_id};
use super::thread::Follower;
use crate::rules::volume::{self as rules, BindFailure};

impl Follower<'_> {
    /// Make sure the enumerator and its client exist and that the bound
    /// endpoint is the default one; re-bind when it is not.
    pub(super) fn follow_default(&mut self, cause: &str) {
        if self.watch.is_none() {
            match DeviceWatch::new(&self.tx, &self.devices_flag) {
                Ok(watch) => self.watch = Some(watch),
                Err(e) => {
                    self.endpoint = None;
                    self.failed(&BindError::other(e), cause);
                    return;
                }
            }
        }
        let Some(watch) = &self.watch else {
            return;
        };
        match default_endpoint_id(&watch.enumerator) {
            Ok((_, id)) if self.endpoint.as_ref().is_some_and(|e| e.id == id) => {}
            Ok(_) => self.bind_default(cause),
            Err(e) => {
                self.endpoint = None;
                self.failed(&e, cause);
            }
        }
    }

    /// Drop the binding (its callback first) and bind the default endpoint.
    pub(super) fn bind_default(&mut self, cause: &str) {
        self.endpoint = None;
        let Some(watch) = &self.watch else {
            return;
        };
        match bind(&watch.enumerator, &self.tx) {
            Ok(endpoint) => {
                log::info!("volume: following the default output device ({cause})");
                log::debug!("volume: default output device {}", endpoint.id);
                self.endpoint = Some(endpoint);
                self.retry_at = None;
            }
            Err(e) => self.failed(&e, cause),
        }
    }

    /// Log a failed bind once per streak and plan the retry.
    pub(super) fn failed(&mut self, e: &BindError, cause: &str) {
        self.failures = self.failures.saturating_add(1);
        // Without the notifications nothing says when a device comes.
        let failure = if self.watch.is_some() {
            e.failure
        } else {
            BindFailure::Other
        };
        if rules::drops_watch_at_once(failure) {
            // The endpoint's callback goes before the enumerator's client.
            self.endpoint = None;
            self.watch = None;
        }
        let delay = rules::retry_after_ms(failure, self.failures);
        self.retry_at = Some(self.now_ms().saturating_add(delay));
        let next = match failure {
            BindFailure::NoDevice => {
                format!("waiting for a device, looking again in {} s", delay / 1000)
            }
            BindFailure::Other => format!("trying again in {} s", delay / 1000),
        };
        if self.failures == 1 {
            log::warn!(
                "volume: no default output device to follow ({cause}): {}; {next}",
                e.error
            );
        } else {
            log::debug!(
                "volume: still no default output device ({cause}, {} in a row): {}; {next}",
                self.failures,
                e.error
            );
        }
    }
}
