//! How a platform event changes what the engine stores.

use std::collections::BTreeMap;

use super::DockEngine;
use super::windows::Tracked;
use crate::events::PlatformEvent;
use crate::model::{RawWindow, Rect};

impl DockEngine {
    pub(super) fn apply_platform(&mut self, ev: PlatformEvent) {
        match ev {
            PlatformEvent::WindowsSnapshot(list) => {
                let mut next = BTreeMap::new();
                for raw in list {
                    let id = raw.id;
                    let (seq, attention) = match self.windows.get(&id) {
                        Some(t) => (t.seq, t.attention),
                        None => (self.alloc_seq(), false),
                    };
                    next.insert(
                        id,
                        Tracked {
                            raw,
                            seq,
                            attention,
                        },
                    );
                }
                self.windows = next;
                // A resync can reveal that the foreground window is gone.
                self.foreground = self.foreground.filter(|id| self.windows.contains_key(id));
                self.overlap_dirty = true;
            }
            PlatformEvent::WindowAdded(raw) | PlatformEvent::WindowUpdated(raw) => self.upsert(raw),
            PlatformEvent::WindowRemoved(id) => {
                if self.windows.remove(&id).is_some() {
                    self.overlap_dirty = true;
                }
                if self.foreground == Some(id) {
                    self.foreground = None;
                }
            }
            PlatformEvent::ForegroundChanged(id) => {
                self.foreground = id.filter(|id| self.windows.contains_key(id));
                if let Some(t) = self.foreground.and_then(|id| self.windows.get_mut(&id)) {
                    // Activating a window clears its attention request.
                    t.attention = false;
                }
            }
            PlatformEvent::WindowAttention { id, flashing } => {
                // The foreground window already has the user's attention, and
                // only a foreground change clears the flag, so a flash of the
                // window in front would keep its icon wiggling. It is never
                // flagged; together with `ForegroundChanged` above, the
                // foreground window never needs attention.
                let in_front = self.foreground == Some(id);
                if let Some(t) = self.windows.get_mut(&id) {
                    t.attention = flashing && !in_front;
                }
            }
            PlatformEvent::MonitorsChanged(monitors) => self.monitors = monitors,
            PlatformEvent::ThemeChanged(theme) => self.theme = theme,
            PlatformEvent::RecycleBinChanged(state) => self.recycle_bin = state,
            // Part of the snapshot (`DockState::power`, and the battery
            // item's presence and title), so `apply` bumps the revision
            // when the reading differs from the one kept and the battery
            // item is shown, or comes or goes (`renders_differently`).
            PlatformEvent::PowerChanged(power) => self.power = power,
            // Part of the snapshot (`DockState::volume`, and the volume
            // item's title), so `apply` bumps the revision when the reading
            // differs from the one kept and the volume item is shown.
            PlatformEvent::VolumeChanged(volume) => self.volume = volume,
            // Part of the snapshot (`DockState::clock_format`) and nothing
            // else, so `apply` bumps the revision when the reading differs
            // from the one kept and the clock item is shown.
            PlatformEvent::ClockFormatChanged(format) => self.clock_format = format,
            // Part of the snapshot (through the docks' facts), so `apply`
            // bumps the revision only when some dock's fact changes.
            PlatformEvent::FullscreenChanged { active, monitor } => {
                self.fullscreen = active;
                self.fullscreen_monitor = if active { monitor } else { None };
            }
        }
    }

    fn upsert(&mut self, raw: RawWindow) {
        let id = raw.id;
        match self.windows.get_mut(&id) {
            Some(t) => {
                if overlap_facts(&t.raw) != overlap_facts(&raw) {
                    self.overlap_dirty = true;
                }
                t.raw = raw;
            }
            None => {
                let seq = self.alloc_seq();
                self.windows.insert(
                    id,
                    Tracked {
                        raw,
                        seq,
                        attention: false,
                    },
                );
                self.overlap_dirty = true;
            }
        }
    }

    fn alloc_seq(&mut self) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        seq
    }
}

/// The facts of a window the overlap rule reads (`overlap::covers`): when an
/// update leaves them as they were, `overlapped` cannot have changed.
fn overlap_facts(raw: &RawWindow) -> (bool, bool, bool, u32, Option<Rect>) {
    (
        raw.is_visible,
        raw.is_minimized,
        raw.is_cloaked,
        raw.process_id,
        raw.rect,
    )
}
