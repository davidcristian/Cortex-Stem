# Dock window placement

This file describes where each dock window is placed: on which monitor, and again after a DPI
change. In this file, `windows::` names the module `src-tauri/src/windows.rs`.

## Position and size

The dock window sits at the bottom of the monitor's **work area**, so a
visible or auto-hidden stock taskbar is respected. It is flush with that
edge and as wide as the work area.

The UI centres the panel along the window and reports its content height
(`set_dock_content_size`, see
[`docs/ipc/sizing-protocol.md`](../ipc/sizing-protocol.md)).

## One window per monitor

With `settings.monitors = all` the app layer keeps one dock window per
monitor: `dock` on the primary monitor, and `dock-<id>` on each other one.
`<id>` is the monitor id with every character outside `[A-Za-z0-9_-]`
dropped, and that window's page is `dock.html?monitor=<id>` (with the full
id, percent-encoded). With `primary`, the default, there is only `dock`.

`windows::plan_docks` (pure) returns which windows should exist for a mode
over a monitor list. `reconcile_docks` applies it at startup, on every
monitor change and when the setting changes. It creates the missing
windows, destroys the ones whose monitor is gone (never the primary's), and
lays out the rest on their own monitor's work area at its
scale.

All per-window state is kept by label: the content size, the hit region,
the panel rect, the scale and the monitor. Every UI report is routed by the calling
window's label, and the engine gets the label and monitor with each panel
rect. So `DockState.docks` contains one `{ monitor, fullscreen, overlapped }`
per dock. The fullscreen result names its window's monitor; a result
without one goes to the primary's dock.

Each page finds its monitor in its URL (the primary's page uses the primary
of `get_monitors`) and reads its own facts. With
`behavior.showOnlyCurrentMonitor` it narrows the snapshot's items to the
windows on its monitor (`src/lib/dock/monitor.ts`, pure). Pinned and fixed
items stay on every dock, and separators are collapsed; the order and the
click semantics stay Rust's. The toggle (the tray icon, or the toggle
shortcut, `Ctrl+Alt+D` by default) decides from the primary's window and
applies to all dock windows.

## DPI changes

Nothing is pushed to the UI when the DPI changes. The page watches the
device pixel ratio itself (`src/lib/dock/dpr.ts`: a
`(resolution: <ratio>dppx)` media query on the current ratio, replaced
after each change; the settings page uses it too, for its pin icons). On a
change the page:

- computes new request sizes: the rest bitmap at exactly
  `iconSize × ratio`, the magnified one from `ICON_SIZE_LADDER` (32, 48, 64,
  96, 128, 192 or 256 px, in `src/lib/dock/icon-size.ts`);
- keeps the old bitmaps on screen until the new ones have arrived;
- measures the panel again (the snap to whole device pixels, redone for
  the new grid);
- reports its content size, hit region and panel rect again, whether or not
  their CSS numbers changed.

On tao's `ScaleFactorChanged` for a dock window, the app layer logs the old
and the new scale. At the next turn of the event loop it plans the window
rect again from the last content size, and converts the last hit region and
panel rect at the new scale, so the window is right before the UI's reports
arrive. The work waits for the next turn because inside the callback tao
has not yet applied its own resize. It goes through the async runtime
because `run_on_main_thread`, called from the main thread, runs at once.
