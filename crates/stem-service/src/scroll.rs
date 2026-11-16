//! Scrolling over a dock item: the limit of one window activation per
//! [`SCROLL_INTERVAL`] ([`ScrollGate`]), and the command that applies one
//! wheel notch.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use stem_core::engine::ScrollEffect;
use stem_core::{ScrollDirection, WindowAction, WindowId};

use crate::{DockService, ServiceError, ServiceResult};

/// Shortest time between two activations the wheel over one dock window's
/// items may cause (`docs/ipc/scrolling-over-an-item.md`): a fast
/// spin moves one window per interval, not one per notch.
pub const SCROLL_INTERVAL: Duration = Duration::from_millis(150);

/// The gate of [`DockService::scroll_item`]: at most one activation
/// per dock window (by label) every [`SCROLL_INTERVAL`]. Pure: the caller
/// passes the time, so the rule is tested without waiting.
#[derive(Debug, Default)]
pub struct ScrollGate {
    /// When each dock window last let a scroll activate a window; only the
    /// ones less than [`SCROLL_INTERVAL`] ago are kept.
    pub(crate) last: HashMap<String, Instant>,
}

impl ScrollGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a scroll in the dock window `dock` may activate a window at
    /// `now`. Yes when the dock's last admitted one is at least
    /// [`SCROLL_INTERVAL`] old (or there is none), and `now` becomes its
    /// last; no when it is younger (a `now` before it counts as younger),
    /// and the gate is left as it was, so a dropped scroll does not push
    /// the next admitted one further away. Each dock window has its own
    /// gate. Entries that have run out are dropped on every call, so the
    /// labels of closed dock windows do not pile up.
    pub fn admit(&mut self, dock: &str, now: Instant) -> bool {
        self.last
            .retain(|_, at| now.saturating_duration_since(*at) < SCROLL_INTERVAL);
        if self.last.contains_key(dock) {
            return false;
        }
        self.last.insert(dock.to_owned(), now);
        true
    }
}

impl DockService {
    /// One wheel notch `direction` over `item_key` in the dock window `dock`
    /// (its label; `docs/ipc/scrolling-over-an-item.md`). With
    /// `behavior.scrollAction = cycleWindows` and an app item with windows
    /// on that dock, the engine picks the next or previous one
    /// (`DockEngine::scroll_action`) and it is activated
    /// (`WindowAction::Activate`, which restores a minimised one), unless
    /// that dock window's gate ([`ScrollGate`]) let one through less than
    /// [`SCROLL_INTERVAL`] ago: then the notch is dropped. The engine is
    /// told which window the notch tried (`note_scroll_activation`), so
    /// the dock's next notch goes on from it even when Windows refused it
    /// or its foreground report is late, if it comes within
    /// `stem_core::engine::SCROLL_MEMORY`. Returns the window
    /// activated, or `None` when the notch did nothing (the setting is
    /// `none`, a fixed item, a separator, no windows there, dropped by the
    /// gate). An unknown key, a hidden app's among them, is
    /// [`ServiceError::UnknownItem`], as for [`Self::primary_click`]; a
    /// failed activation is the platform's error, and still counts for the
    /// gate and as the window tried.
    ///
    /// Over the volume item a notch steps the volume, whatever
    /// `behavior.scrollAction` says and without the gate: every notch
    /// counts ([`Self::step_volume`]); it returns `None` (no window was
    /// activated). It waits on the platform's volume thread, bounded: call
    /// it off the UI thread and off an async runtime's workers.
    pub fn scroll_item(
        &self,
        item_key: &str,
        dock: &str,
        direction: ScrollDirection,
    ) -> ServiceResult<Option<WindowId>> {
        self.scroll_item_at(item_key, dock, direction, Instant::now())
    }

    /// [`Self::scroll_item`] at the time `now` (tests pass their own).
    pub(crate) fn scroll_item_at(
        &self,
        item_key: &str,
        dock: &str,
        direction: ScrollDirection,
        now: Instant,
    ) -> ServiceResult<Option<WindowId>> {
        let effect = self
            .inner
            .engine
            .lock()
            .scroll_action(item_key, dock, direction, now);
        let id = match effect {
            ScrollEffect::Activate(id) => id,
            ScrollEffect::StepVolume(direction) => {
                self.step_volume(direction)?;
                return Ok(None);
            }
            ScrollEffect::Nothing => {
                log::debug!("scroll {direction:?} over {item_key} in {dock:?}: nothing to do");
                return Ok(None);
            }
            ScrollEffect::UnknownItem => {
                return Err(ServiceError::UnknownItem(item_key.to_owned()));
            }
        };
        if !self.inner.scroll_gate.lock().admit(dock, now) {
            log::debug!(
                "scroll {direction:?} over {item_key} in {dock:?}: dropped, less than \
                 {SCROLL_INTERVAL:?} after the last one"
            );
            return Ok(None);
        }
        log::debug!("scroll {direction:?} over {item_key} in {dock:?}: activating {id:?}");
        // Before the platform call, so that this activation's own
        // foreground report ends it: a refused activation leaves the
        // foreground where it was, and the dock's next notch must still
        // go on from `id`.
        self.inner
            .engine
            .lock()
            .note_scroll_activation(dock, id, now);
        self.inner
            .platform
            .window_action(id, WindowAction::Activate)?;
        Ok(Some(id))
    }
}
