# The state snapshot

This file describes a part of `DockState`, the snapshot that `get_dock_state` returns and the
[`dock://state`](events.md) event sends: the fullscreen fact in `DockState.fullscreen`.

## `DockState.fullscreen`

`DockState.fullscreen` is `true` when the platform reports that the foreground window covers the
whole screen (a game, a browser after F11, a video in fullscreen, a slideshow). The platform
computes it from `SHQueryUserNotificationState` and from the window's frame against its monitor's
bounds (`rules::fullscreen`). It computes it again when the foreground changes and when the
foreground window moves, and sends it only when it changes. While the fact is set, the UI hides
the dock whatever `autoHide.mode` says. It shows the dock again when the fact clears, unless the
mode is `always` and the dock was hidden anyway. The reveal strip works in the meantime
([Hit region](hit-region.md)). There is no `dock://visibility` event (v1.6): the fact is sent in
the snapshot, as the foreground is. There is one dock, so the field describes the foreground
window's own monitor.
