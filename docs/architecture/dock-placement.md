# Dock window placement

This file describes where the dock window is placed, and again after a DPI change.

## Position and size

The dock window sits at the bottom of the monitor's **work area**, so a
visible or auto-hidden stock taskbar is respected. It is flush with that
edge and as wide as the work area.

The UI centres the panel along the window and reports its content height
(`set_dock_content_size`, see
[`docs/ipc/sizing-protocol.md`](../ipc/sizing-protocol.md)).

## One window per monitor

There is one dock window, `dock`. One window per monitor, labelled
`dock-<monitorId>`, is milestone M3 of the [plan](../PLAN.md).

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
