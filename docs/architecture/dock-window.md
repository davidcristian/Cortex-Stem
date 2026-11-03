# The dock window

This file describes the dock's native window: its flags and focus, and click-through outside
the panel. The other parts of the dock window are described in:

- [dock-placement.md](dock-placement.md): position and size, and one window per monitor.

## Window flags and focus

The dock is a Tauri `WebviewWindow` labelled `dock`. It is undecorated,
transparent, always on top, not resizable, `skip_taskbar` and without a
system shadow. The window is created hidden and shown after it is
positioned.

`Platform::prepare_dock_window` adds `WS_EX_TOOLWINDOW` and
`WS_EX_NOACTIVATE`, and turns off DWM corner rounding and the shadow.

Clicking the dock must not take the focus from the active app.

## Click-through outside the panel

The UI reports the panel and the envelope, the area that magnified icons
can reach (`set_dock_hit_region`). The platform tracks the cursor on its own
thread and toggles `WS_EX_LAYERED | WS_EX_TRANSPARENT` with hysteresis
([`docs/ipc/hit-region.md`](../ipc/hit-region.md)). The platform's window
subclass keeps Stem's extended styles across tao's own style rewrites.
