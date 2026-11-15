# Fixed items

This page describes what each fixed item of the dock does. Turn each one on
or off under Settings → [Items](settings-window.md#items).

- **Start** opens the Start menu.
- **Task View** opens Task View (your windows and virtual desktops).
- **Show Desktop** minimises every window. A second click brings them back.
- **Quick Settings** opens Windows' quick settings (network, volume, battery
  and the other quick toggles), as Win+A does.
- **Notification Center** opens your notifications and the calendar, as
  Win+N does.
- **Recycle Bin**: see [The Recycle Bin item](#the-recycle-bin-item).

The menus of Start, Task View, Show Desktop, Quick Settings and Notification
Center have one entry, which does the same as a click: _Open Start menu_,
_Open Task View_, _Show desktop_, _Open Quick Settings_ and _Open
Notification Center_. Like every menu, they end with _Stem settings…_ and
_Quit Stem_.

Quick Settings and Notification Center come just before the Recycle Bin, in
that order. For a click on either of the two items, Stem
sends Windows the keyboard shortcut (Win+A or Win+N). Windows then opens its
panel where it always does, in the corner of the screen, not beside the
dock. While a window that runs as administrator is in front, Windows may
ignore the shortcut, as it ignores keys that Stem sends to such a window.

## The Recycle Bin item

- Its icon shows whether the bin is empty or full. The icon updates within
  about 5 seconds, and at once after a drop on it.
- A click opens the Recycle Bin in File Explorer. Its menu has _Open_ and
  _Empty Recycle Bin…_.
- Files and folders dropped on it go to the Recycle Bin. The item grows a
  little while a drag is over it. Windows shows its usual confirmation and
  progress dialogs, and the move can be undone from the bin. Windows warns
  before it permanently deletes anything that it cannot recycle. Stem does
  not recycle drive roots or paths that do not exist.
