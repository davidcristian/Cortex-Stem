//! What one wheel notch over a dock item does: activate the next or the
//! previous window of an app, or step the volume.

use std::time::{Duration, Instant};

use super::DockEngine;
use crate::model::{DockItemKind, ScrollDirection, VolumeStatus, WindowId};
use crate::settings::ScrollAction;

/// How long a dock window remembers the window its last admitted wheel
/// notch tried to activate ([`DockEngine::note_scroll_activation`];
/// the foreground changing ends it sooner). 2 s: far longer than a late
/// foreground report (tens of milliseconds) or a pause within one spin of
/// the wheel, and short enough that a notch turned later steps from the
/// window in front again, so a window Windows refused is tried once more.
pub const SCROLL_MEMORY: Duration = Duration::from_secs(2);

/// The effect of one wheel notch over a dock item
/// (`docs/ipc/scrolling-over-an-item.md`): what
/// [`DockEngine::scroll_action`] decides. How often it may happen is the
/// caller's business (the service's 150 ms gate per dock window).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollEffect {
    /// Bring this window to the front (`WindowAction::Activate`, which also
    /// restores a minimised one).
    Activate(WindowId),
    /// Step the default output device's volume by one notch: the
    /// item is the volume item, which the wheel drives whatever
    /// `behavior.scrollAction` says. The caller reads the volume as it is
    /// now and sets what [`volume_step`] makes of it; the 150 ms gate does
    /// not apply, every notch counts.
    StepVolume(ScrollDirection),
    /// Nothing to do: `behavior.scrollAction` is `none`, the item is not an
    /// app (a fixed item other than the volume, a separator), or the dock
    /// lists none of its windows.
    Nothing,
    /// The key names no item of the snapshot: a key the page made up, an
    /// item that has gone, or a hidden app's. The caller rejects it
    /// as `primary_click` rejects such a key.
    UnknownItem,
}

impl DockEngine {
    /// Decide what one wheel notch `direction` over `item_key` in the dock
    /// window `dock` does (`docs/ipc/scrolling-over-an-item.md`).
    ///
    /// - a key the snapshot has no item for is [`ScrollEffect::UnknownItem`],
    ///   whatever the setting;
    /// - the volume item is [`ScrollEffect::StepVolume`] in that
    ///   direction, whatever the setting and whatever the engine's last
    ///   reading (the caller reads the volume now; without an output device
    ///   it does nothing);
    /// - with `behavior.scrollAction = cycleWindows`, an app item whose app
    ///   has windows that dock lists ([`Self::windows_on_dock`], first-seen
    ///   order, as its menu lists them) activates, for
    ///   [`ScrollDirection::Next`], the window after the one it steps from
    ///   and, for [`ScrollDirection::Previous`], the one before it,
    ///   wrapping around at either end. It steps from the window that
    ///   dock's last notch tried ([`Self::note_scroll_activation`]) while
    ///   the foreground has not changed since, that notch is less than
    ///   [`SCROLL_MEMORY`] before `now` and that window is still
    ///   one of them, else from the one in the foreground; when neither is
    ///   one of them, `Next` activates the first and `Previous` the last.
    ///   A lone window in front is its own neighbour: activated again,
    ///   which changes nothing;
    /// - anything else ([`ScrollAction::None`], another fixed item, a
    ///   separator, an app item with no window on that dock) is
    ///   [`ScrollEffect::Nothing`].
    pub fn scroll_action(
        &self,
        item_key: &str,
        dock: &str,
        direction: ScrollDirection,
        now: Instant,
    ) -> ScrollEffect {
        let Some(item) = self
            .snapshot()
            .items
            .into_iter()
            .find(|i| i.key == item_key)
        else {
            return ScrollEffect::UnknownItem;
        };
        if item.kind == DockItemKind::Volume {
            return ScrollEffect::StepVolume(direction);
        }
        if self.settings.behavior.scroll_action == ScrollAction::None {
            return ScrollEffect::Nothing;
        }
        let Some(app_id) = item.app_id.filter(|_| item.kind == DockItemKind::App) else {
            return ScrollEffect::Nothing;
        };
        let windows = self.windows_on_dock(&app_id, dock);
        let count = windows.len();
        if count == 0 {
            return ScrollEffect::Nothing;
        }
        let position = |id: WindowId| windows.iter().position(|w| *w == id);
        let from = self
            .scroll_tried
            .get(dock)
            .filter(|(_, at)| now.saturating_duration_since(*at) < SCROLL_MEMORY)
            .and_then(|(tried, _)| position(*tried))
            .or_else(|| self.foreground.and_then(position));
        let index = match (from, direction) {
            (Some(pos), ScrollDirection::Next) => (pos + 1) % count,
            (Some(pos), ScrollDirection::Previous) => (pos + count - 1) % count,
            (None, ScrollDirection::Next) => 0,
            (None, ScrollDirection::Previous) => count - 1,
        };
        ScrollEffect::Activate(windows[index])
    }

    /// The caller let a wheel notch in the dock window `dock` through at
    /// `now` and asks the platform to activate `window`. Until the
    /// foreground changes, and for at most [`SCROLL_MEMORY`] after `now`,
    /// that dock's next notch steps from `window` rather than
    /// from the window in front ([`Self::scroll_action`]): an activation
    /// Windows refuses (an elevated window, the foreground lock, a hung
    /// window) leaves the foreground where it was, and stepping from it
    /// would choose the refused window again at every notch; an
    /// activation whose foreground report has not arrived yet would be
    /// chosen twice. A notch after the limit steps from the foreground
    /// again, so it tries a refused window once more. Call it before the
    /// platform call, so the report of this very activation, or any other
    /// foreground change, ends it. Not part of the snapshot.
    pub fn note_scroll_activation(&mut self, dock: &str, window: WindowId, now: Instant) {
        self.scroll_tried.insert(dock.to_owned(), (window, now));
    }
}

/// How far one wheel notch over the volume item moves the volume, in
/// percentage points: Windows' own step for its volume keys.
pub const VOLUME_STEP: u8 = 2;

/// What one wheel notch `direction` over the volume item makes of the
/// volume `current` (docs/ipc/widget-items.md "The volume item"):
/// [`ScrollDirection::Previous`] (the wheel turned away from the user, up)
/// raises it by [`VOLUME_STEP`], up to 100, and unmutes, as Windows'
/// volume-up key does, at 100 too; [`ScrollDirection::Next`] (towards the
/// user, down) lowers it by [`VOLUME_STEP`], down to 0, and leaves the
/// mute as it is. The level moves by the step from where it is, without
/// snapping to even numbers; a level above 100 (never sent) counts as 100.
pub fn volume_step(current: VolumeStatus, direction: ScrollDirection) -> VolumeStatus {
    let percent = current.percent.min(100);
    match direction {
        ScrollDirection::Previous => VolumeStatus {
            percent: percent.saturating_add(VOLUME_STEP).min(100),
            muted: false,
        },
        ScrollDirection::Next => VolumeStatus {
            percent: percent.saturating_sub(VOLUME_STEP),
            muted: current.muted,
        },
    }
}
