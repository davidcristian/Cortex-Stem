# Auto-hide

_Auto-hide_ (Settings → [Behaviour](settings-window.md#behaviour)) has three
modes:

- **Off**: the dock stays on screen.
- **Always**: the dock slides away towards its screen edge when the pointer
  has been away from it for the _Auto-hide delay_ (250 ms by default).
- **When a window overlaps**: while another app's window covers the panel,
  the dock slides away once the pointer has been away from it for the delay.
  It comes back as soon as no window covers it. A maximised window covers
  the dock too, so a maximised window hides the dock.

To bring a hidden dock back, move the pointer to the screen edge the dock is
on, where the dock was. The reveal strip there is 2 pixels thick and as long
as the panel. It is on that edge of the work area. That is the screen's edge,
unless the stock taskbar or another app bar (a program that reserves an edge
of the screen, as the taskbar does) is on the same side and is not set to
auto-hide. Then the strip is just inside the taskbar. The toggle shortcut
and a left click on the tray icon also bring the dock back in one press.
Touch and pen cannot reveal the dock, so use the shortcut or the tray.

The dock slides in about 200 ms. With Windows' _Animation effects_ off, it
fades instead.

The dock does not hide while you drag one of its icons, while one of its
menus is open, while an icon bounces for an app you started from the dock,
while a file is dragged over it, or while one of its icons has the keyboard
focus. An app that requests attention brings a hidden dock back and keeps it
on screen until you switch to that app.

A fullscreen app (a game, a video in fullscreen, a browser after F11, a
slideshow) hides the dock on its monitor in every mode. The dock hides at
once, or, with the pointer on the dock, as soon as the pointer leaves. The
reveal strip still works over a fullscreen app, unless the stock taskbar (or
another app bar) is on the dock's edge and is not set to auto-hide. In that
case the toggle shortcut or the tray brings the dock back. A maximised window
is not fullscreen.

Auto-hide works best with _Click through outside the dock_ on. With it off,
the empty space the hidden dock leaves still takes clicks.
