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
- **Volume** shows the volume of your speakers or headphones (the default
  output device in Windows' sound settings). See
  [The volume item](#the-volume-item).
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

Quick Settings, Notification Center, Volume, Battery and Clock come just
before the Recycle Bin, in that order. For a click on any of the five, Stem
sends Windows the keyboard shortcut (Win+A or Win+N). Windows then opens its
panel where it always does, in the corner of the screen, not beside the
dock. While a window that runs as administrator is in front, Windows may
ignore the shortcut, as it ignores keys that Stem sends to such a window.
Click these items without holding Ctrl, Shift or Alt. Windows may add a key
you hold to the shortcut: Win+Ctrl+N, for example, opens Narrator's settings
instead.

## The volume item

- The icon is a speaker with no wave at 0 %, one wave up to 33 %, two waves
  up to 66 % and three waves above. The level is written below it.
- While the sound is muted, a cross replaces the waves, and the level is
  kept.
- While Windows has no output device, the icon is a struck-out speaker with
  a dash.
- Its name says the same in words: `Volume 45 %`, `Volume muted` or
  `No audio output`.
- It updates within moments when the volume keys or the Quick Settings
  slider change the volume, and when the default device changes
  (headphones plugged in or taken out).
- The mouse wheel over it changes the volume by 2 for each notch, from 0 to
  100, as the volume keys do. A notch up also unmutes. This works whatever
  _Scrolling over an app_ is set to. There is no pause between notches as
  there is for an app's windows: each notch counts, but one very fast flick
  of the wheel counts as ten notches at most.
- A click opens quick settings, as Win+A does, where the volume slider is.
- Its menu has _Mute_ (or _Unmute_ while muted; greyed out without an output
  device), _Sound settings_ and _Volume mixer_. The last two open those pages
  of the Windows Settings app.

## The battery item

- The battery in the icon fills to the charge, and the percent is written
  below it.
- A bolt shows while the battery charges, and a plug while the PC is plugged
  in without charging.
- The fill turns amber while battery saver (energy saver) is on. It turns red
  below 20 % while the battery is not charging and saver is off. However low
  the charge, a thin strip of fill stays visible.
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
- All three use your _Regional format_ (Settings → Time & language →
  Language & region → _Regional format_), as the taskbar's clock does,
  whatever your display language. English (United Kingdom) shows `14:05`,
  English (United States) shows `2:05 PM`.
- A 12-hour or 24-hour _Short time_ that you chose (_Regional format_ →
  _Change formats_, or the Control Panel's Region settings) is used too.
- The dock takes only the 12-hour or 24-hour clock and the region's way of
  writing the date, not the format pattern itself. It does not show literal
  text or seconds from a custom time format: a custom `HH'h'mm` gives `14:05`
  on the dock. It does not use the pattern's hour digits or Windows' AM and
  PM symbols either. A 12-hour `h:mm tt` under English (United Kingdom)
  shows `2:05 pm` on the dock and `02:05 pm` in its pill, where the taskbar
  uses Windows' symbols (such as `2:05 PM`).
- A change of format applies within a second, without restarting Stem.
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
