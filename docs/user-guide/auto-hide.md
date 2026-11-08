# Auto-hide

_Auto-hide_ (Settings → [Behaviour](settings-window.md#behaviour)) has three
modes:

- **Off**: the dock stays on screen.
- **Always**: the dock slides away when the pointer has been away from it for
  the _Auto-hide delay_ (250 ms by default).
- **When a window overlaps**: while another app's window covers the panel,
  the dock slides away once the pointer has been away from it for the delay.
  It comes back as soon as no window covers it. A maximised window covers
  the dock too, so a maximised window hides the dock.

To bring a hidden dock back, move the pointer to the bottom edge of the
screen, under where the dock was. The reveal strip there is 2 pixels tall and
as wide as the panel. It is at the bottom of the work area (the part of the
screen that maximised windows fill). That is the screen's edge while the
stock taskbar is hidden or set to auto-hide. Otherwise the strip is just
above the taskbar. The toggle shortcut and a left click on the tray icon also
bring the dock back in one press. Touch and pen cannot reveal the dock, so
use the shortcut or the tray.

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
reveal strip still works over a fullscreen app. A maximised window is not
fullscreen.

Auto-hide works best with _Click through outside the dock_ on. With it off,
the empty space the hidden dock leaves still takes clicks.
