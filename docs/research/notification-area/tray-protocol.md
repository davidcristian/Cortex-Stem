# How an icon reaches the tray

This page is part of the [notification-area study](index.md): how an app's
icon gets into the notification area (the tray) and how its events get back
to the app. Claims marked **[A1]** and so on are listed in
[Assumptions](assumptions.md).

- An app calls `Shell_NotifyIconW` with a `NOTIFYICONDATAW`: the window
  that receives the icon's events (`hWnd`), an id (`uID`, or `guidItem` on
  Windows 7 and later), an `HICON`, a tooltip and a callback message
  (`uCallbackMessage`).
- shell32 looks up the top-level window of class `Shell_TrayWnd` with
  `FindWindow` and sends it the data as `WM_COPYDATA`. The call returns
  what that window's procedure returns. shell32 waits for that result, 4 s
  in most versions and 7 s in Windows Vista's, and stops waiting early if
  the target appears hung. This is from Geoff Chappell's study of shell32,
  not from Microsoft.
- What is inside the `WM_COPYDATA` is not documented. Open-source shells
  read `dwData` 1 as a notify-icon message, 0 as an `SHAppBarMessage` call
  and 3 as a `Shell_NotifyIconGetRect` query **[A1]**. So the same window
  also handles every app bar's messages in the session, including Stem's
  own: the taskbar auto-hide state (`windows/taskbar.rs`) and the dock's
  app bar while *Reserve screen space* is on (`windows/appbar/win32_shell.rs`).
- Events go back the documented way: the tray sends `uCallbackMessage` to
  `hWnd`. With `NOTIFYICON_VERSION_4`, `LOWORD(lParam)` is the event
  (`NIN_SELECT`, `NIN_KEYSELECT`, `WM_CONTEXTMENU`, mouse messages),
  `HIWORD(lParam)` is the icon id and `wParam` is the anchor point.
- An app that shows a menu for its icon must be the foreground window
  before `TrackPopupMenu`, or the menu does not close on an outside click.
  A tray other than Explorer must give the app that right with
  `AllowSetForegroundWindow`. It can do so only while it may set the
  foreground itself, for example right after the user's click.
- When the taskbar is created it broadcasts the registered message
  `TaskbarCreated`, and apps add their icons again. On Windows 10 it is
  also broadcast when the primary display's DPI changes.
- Balloons (`NIF_INFO`) appear on Windows 10 and 11 as banner
  notifications.
- A new icon starts in the overflow, and only the user can move it to the
  visible area (promote it). On Windows 11 that setting is in *Taskbar
  settings → Other system tray icons*.
