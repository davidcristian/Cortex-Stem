# The dock window

This file describes the dock's native window: its flags and focus, click-through outside the
panel, and how it stays topmost. The other parts of the dock window are described in:

- [dock-placement.md](dock-placement.md): position and size, and one window per monitor;
- [auto-hide.md](auto-hide.md): auto-hide.

## Window flags and focus

The dock is a Tauri `WebviewWindow` labelled `dock`. It is undecorated,
transparent, always on top, not resizable, `skip_taskbar`, without a
system shadow, and **not focusable**. tao derives `WS_EX_NOACTIVATE` from
that flag, so its own style rewrites keep the bit. The window is created
hidden and shown after it is positioned.

`Platform::prepare_dock_window` runs once, on the main thread, right after
creation. It:

- adds `WS_EX_TOOLWINDOW` and `WS_EX_NOACTIVATE`;
- turns off DWM corner rounding. Tauri's `shadow(false)` already removes
  the shadow. DWM non-client rendering stays *on*; otherwise Windows paints
  the legacy caption of the `WS_CAPTION` window that tao keeps for
  undecorated windows;
- subclasses the window (`SetWindowSubclass`). The subclass sets the
  extended styles again on every `WM_STYLECHANGING`, returns
  `MA_NOACTIVATE` for `WM_MOUSEACTIVATE` and applies the pass-through
  transitions;
- puts `WS_EX_NOACTIVATE` on the WebView2 child windows (Chromium applies
  it on windows that are not top-level).

Clicking the dock must not take the focus from the active app. The events
thread ignores foreground reports for windows of Stem's own process, so the
clicked app stays the active one for the engine.

## Click-through outside the panel

The UI reports the panel and the envelope, the area that magnified icons
can reach (`set_dock_hit_region`). The platform tracks the cursor on its own
thread and toggles `WS_EX_LAYERED | WS_EX_TRANSPARENT` with hysteresis
([`docs/ipc/hit-region.md`](../ipc/hit-region.md)). The platform's window
subclass keeps Stem's extended styles across tao's own style rewrites.
`behavior.clickThrough` (a settings row and a tray check item, on by
default) switches this off and on at runtime. When it goes off, the Tauri
layer passes `None` to the platform. When it comes back on, the layer sends
the UI's latest region again.

## Topmost

On Windows 11 the dock can drop out of the topmost band while it keeps
`WS_EX_TOPMOST` (seen after a lock and unlock with a maximised browser in
front), which leaves it behind a maximised window. So the events thread
posts `MSG_ASSERT_TOPMOST` to every prepared dock window on each foreground
change, and the subclass also checks after each pass-through change. The
check is a bounded `GetWindow(GW_HWNDPREV)` walk.
`SetWindowPos(HWND_TOPMOST)` runs only when a visible window that is not
topmost is above the dock (logged at debug). So the common case is one
read, and the dock is never moved above menus or flyouts when it does not
need to be.
