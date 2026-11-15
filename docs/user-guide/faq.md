# FAQ

This page answers common questions about Stem and links to the pages of the
user guide that have the details.

## Why does a click on an app sometimes only make it flash?

Windows limits which app may bring a window to the front, so that a program
cannot take the keyboard focus from the window you are using. A click on the
dock normally lets Stem do so, and Stem tries the documented fallbacks when
the first attempt fails. [Clicking an app](clicking-an-app.md) says what the
dock shows when all of them fail. Clicking the window itself brings it
forward.

## Do apps that run as administrator work?

They appear on the dock with their name and icon, and a click normally brings
them to the front. Windows does not let a normal app send messages to an
elevated one, so minimising, restoring or closing an elevated window from the
dock can fail; the dock then shows a notice. Pinning an elevated app pins the
ordinary app, which starts without elevation (or through UAC if the app
always requires it). Windows of protected processes, which Windows does not
let other apps inspect, are named by their window title, may get a generic
icon, and cannot be pinned. Stem itself runs without elevation.

## How do I pin a packaged (Store) app?

Packaged apps, such as Calculator or the Windows 11 Notepad and Terminal, are
recognised by their app identity: they get their Start menu name and icon and
start the way the Start menu starts them. Their program files are in a
protected folder, so the easy way to pin one is to start it, right-click its
icon and choose _Pin to Stem_. [Pinned apps](pinned-apps.md) describes the
other ways to pin an app.

## Where are my windows from other virtual desktops?

By default the dock lists only the windows on the current virtual desktop,
and switching desktops updates it. Turn off _Show only windows on the current
desktop_ (Settings → [Behaviour](settings-window.md#behaviour)) to list every
window.

## Does the dock replace the system tray?

No. Replacing the notification area is not a goal, for the reasons in the
[notification area research](../research/notification-area/index.md). The
tray icons, the clock and the input indicators stay on the stock taskbar,
which stays reachable with [_Hide the Windows taskbar_](hiding-the-windows-taskbar.md) on.

## Which keyboard shortcuts can I use?

`Ctrl+Alt+D` shows or hides the dock, and it can be changed under Settings →
[Behaviour](settings-window.md#behaviour).
[Keyboard shortcuts](keyboard-shortcuts.md) has the syntax, the keys a
shortcut must include, and what happens to a shortcut that another app has
already registered.

## The dock vanished. How do I get it back?

- With auto-hide on, move the pointer to the edge the dock sits on, beside
  where the dock was. [Auto-hide](auto-hide.md) says where on that edge the
  dock comes back, and what to use with touch and pen.
- Press the [toggle shortcut](keyboard-shortcuts.md#the-toggle-shortcut)
  (`Ctrl+Alt+D` by default), or left-click Stem's tray icon.
- A fullscreen app hides the dock on its monitor until it leaves fullscreen.
  [Auto-hide](auto-hide.md) says when the screen edge still brings the dock
  back over it.
- Start Stem again from the Start menu: a running Stem shows its dock, and a
  Stem that is not running starts.

## Where is the log?

`%LOCALAPPDATA%\dev.davidcristian.stem\logs\stem.log`, or Settings → About →
_Open folder_. Set `STEM_LOG=debug` before starting Stem for more detail;
[The log file](crashes-and-logs.md#the-log-file) explains the levels.
