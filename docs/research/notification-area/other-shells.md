# What other shells do with the notification area

This page is part of the [notification-area study](index.md): how three
open-source shells get the notification icons that apps add.

| Project | Licence | How it gets the icons |
|---|---|---|
| ManagedShell (the library under Cairo Desktop and RetroBar) | Apache-2.0 | Its own `Shell_TrayWnd`, `WS_EX_TOPMOST`, raised again by a 100 ms timer whenever `FindWindow` returns another window; Explorer's tray pushed to `HWND_BOTTOM`. Parses `WM_COPYDATA`, forwards all of it to Explorer's tray, broadcasts `TaskbarCreated` at start and again at exit when it is not the shell. Icons that already exist: `TB_GETBUTTON` and `ReadProcessMemory` on Explorer's toolbar (Windows 10), and the undocumented `ITrayNotify` COM interface. |
| systray-util (the crate under Zebar's tray) | MIT (the repository is GPL-3.0) | The same forwarding window in Rust, 100 ms timer included. Its own notes say the toolbar read works on Windows 10 only. |
| Seelen UI (a Tauri desktop shell) | AGPL-3.0 | A DLL loaded into Explorer with `SetWindowsHookExW(WH_CALLWNDPROC)` on `Shell_TrayWnd`'s thread, so it receives the messages without a second window. Broadcasts `TaskbarCreated`. Clicks: `AllowSetForegroundWindow`, then `SendNotifyMessageW` with the callback message. |

None of them has a documented way to list the icons that existed before it
started, other than making every app add them again.
