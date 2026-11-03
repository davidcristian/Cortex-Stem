# Dock window placement

This file describes where the dock window is placed.

## Position and size

The dock window is centred at the bottom of the monitor's **work area**, so
a visible or auto-hidden stock taskbar is respected.

The UI reports its content size (`set_dock_content_size`, see
[`docs/ipc/sizing-protocol.md`](../ipc/sizing-protocol.md)), and the window
follows it.

## One window per monitor

There is one dock window, `dock`. One window per monitor, labelled
`dock-<monitorId>`, is milestone M3 of the [plan](../PLAN.md).
