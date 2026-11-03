# The dock window

This file describes the dock's native window: its flags and focus. The other parts of the
dock window are described in:

- [dock-placement.md](dock-placement.md): position and size, and one window per monitor.

## Window flags and focus

The dock is a Tauri `WebviewWindow` labelled `dock`. It is undecorated,
transparent, always on top, not resizable, `skip_taskbar` and without a
system shadow. The window is created hidden and shown after it is
positioned.

`Platform::prepare_dock_window` adds `WS_EX_TOOLWINDOW` and
`WS_EX_NOACTIVATE`, and turns off DWM corner rounding and the shadow.

Clicking the dock must not take the focus from the active app.

The whole window takes the pointer, including the transparent headroom
above the panel, so clicks there do not reach the windows below.
