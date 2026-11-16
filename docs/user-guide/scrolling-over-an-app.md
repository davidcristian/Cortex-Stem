# Scrolling over an app

Set Settings → [Behaviour](settings-window.md#behaviour) → _Scrolling over an
app_ to _Cycles its windows_, and the mouse wheel over an app's icon goes
through the app's open windows:

- Each notch towards you brings the app's next window to the front. Each
  notch away from you brings its previous window.
- After the last window comes the first again. A minimised window is
  restored.
- When none of the app's windows is in front, the first notch towards you
  brings its first window, and the first notch away from you brings its last
  window.
- A two-finger scroll on a touchpad does the same. The direction that would
  scroll a page down brings the next window. Scrolling about as far as one
  wheel notch would scroll brings one window.
- A pinch on the touchpad, and the wheel with Ctrl held, do nothing over the
  dock.

The wheel goes through the windows in the order the icon's menu lists them.
Only the windows that this dock shows take part, as for a click. With a dock
on every monitor and _Show only windows on the dock's monitor_ on, those are
the windows on the dock's monitor.

A fast spin of the wheel moves on at most one window every 150 ms, not one
per notch, so each window has time to come to the front. Sometimes Windows
does not let Stem bring a window forward (an app running as administrator
can cause this). A notice then appears next to the dock, and the next notch
goes on to the window after it.

Scrolling over a fixed item other than the volume item, or over a pinned app
without windows, does nothing. Scrolling never starts an app. With the
default, _Does nothing_, the wheel does nothing anywhere on the dock. The one
exception is the volume item: the wheel over it always changes the volume,
whatever this setting is (see [The volume item](fixed-items.md#the-volume-item)).

Windows sends the wheel to the window under the pointer, and so to the dock,
only while Settings → Bluetooth & devices → Mouse → _Scroll inactive windows
when hovering over them_ is on. It is on unless you turned it off. With it
off, the wheel scrolls the app you are working in instead.
