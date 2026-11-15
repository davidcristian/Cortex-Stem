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
- **Battery** shows the battery's charge. See
  [The battery item](#the-battery-item).
- **Clock** shows the time and the date. See
  [The clock item](#the-clock-item).
- **Recycle Bin**: see [The Recycle Bin item](#the-recycle-bin-item).

The menus of Start, Task View, Show Desktop, Quick Settings and Notification
Center have one entry, which does the same as a click: _Open Start menu_,
_Open Task View_, _Show desktop_, _Open Quick Settings_ and _Open
Notification Center_. Like every menu, they end with _Stem settings…_ and
_Quit Stem_.

Quick Settings, Notification Center, Battery and Clock come just before
the Recycle Bin, in that order. For a click on any of the four, Stem
sends Windows the keyboard shortcut (Win+A or Win+N). Windows then opens its
panel where it always does, in the corner of the screen, not beside the
dock. While a window that runs as administrator is in front, Windows may
ignore the shortcut, as it ignores keys that Stem sends to such a window.
Click these items without holding Ctrl, Shift or Alt. Windows may add a key
you hold to the shortcut: Win+Ctrl+N, for example, opens Narrator's settings
instead.

## The battery item

- The battery in the icon fills to the charge, and the percent is written
  below it.
- A bolt shows while the battery charges, and a plug while the PC is plugged
  in without charging.
- The fill turns amber while battery saver (energy saver) is on.
- Its name says the same in words (`Battery 73 %, charging`). While Windows
  does not report the charge, the name is just `Battery`.
- It updates within moments of a change, and after the PC wakes up.
- A PC without a battery (most desktops) never shows the item, even with the
  switch on.
- A click opens quick settings, as Win+A does. Its menu has _Open Quick
  Settings_ and _Power and battery settings_, which opens that page of the
  Windows Settings app.

## The clock item

- The icon shows the time over the short weekday and the day. Its name (and
  the pill over it) is the full date and time.
- All three use the format of your Windows display language (Settings →
  Time & language → Language & region → _Windows display language_). The
  clock does not use the _Regional format_ set there, or a time format you
  changed, such as a 24-hour clock. With an English (United States) display
  language and an English (United Kingdom) region, the taskbar shows `14:05`
  and the dock shows `2:05 PM`.
- A click opens your notifications and the calendar, as Win+N does. Its menu
  has _Open Notification Center_ and _Date and time settings_, which opens
  that page of the Windows Settings app.

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
