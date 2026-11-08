# Sources for the notification-area study

This page is part of the [notification-area study](index.md): the APIs,
documentation and other sources it relies on.

Win32 and WinRT APIs this study relies on:

- `Shell_NotifyIconW`: https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shell_notifyiconw
- `NOTIFYICONDATAW`: https://learn.microsoft.com/en-us/windows/win32/api/shellapi/ns-shellapi-notifyicondataw
- `Shell_NotifyIconGetRect`: https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shell_notifyicongetrect
- `NOTIFYICONIDENTIFIER`: https://learn.microsoft.com/en-us/windows/win32/api/shellapi/ns-shellapi-notifyiconidentifier
- `SHAppBarMessage`: https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shappbarmessage
- `FindWindowW`: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-findwindoww
- `FindWindowExW`: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-findwindowexw
- `EnumChildWindows`: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-enumchildwindows
- `WM_COPYDATA`: https://learn.microsoft.com/en-us/windows/win32/dataxchg/wm-copydata
- `COPYDATASTRUCT`: https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-copydatastruct
- `RegisterWindowMessageW`: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerwindowmessagew
- `SendNotifyMessageW`: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendnotifymessagew
- `SendMessageTimeoutW`: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendmessagetimeoutw
- `SetWindowPos`: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos
- `ChangeWindowMessageFilterEx`: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-changewindowmessagefilterex
- `GetWindowThreadProcessId`: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowthreadprocessid
- `AllowSetForegroundWindow`: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-allowsetforegroundwindow
- `SetForegroundWindow`: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setforegroundwindow
- `TrackPopupMenu`: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-trackpopupmenu
- `GetIconInfo`: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-geticoninfo
- `GetDIBits`: https://learn.microsoft.com/en-us/windows/win32/api/wingdi/nf-wingdi-getdibits
- `SetWindowsHookExW`: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowshookexw
- `SendInput`: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput
- `GetKeyboardLayout`: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getkeyboardlayout
- `IUIAutomation`: https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nn-uiautomationclient-iuiautomation
- `IUIAutomation::ElementFromHandle`: https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomation-elementfromhandle
- `IUIAutomation::AddStructureChangedEventHandler`: https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomation-addstructurechangedeventhandler
- `IUIAutomationInvokePattern`: https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nn-uiautomationclient-iuiautomationinvokepattern
- `UserNotificationListener`: https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.management.usernotificationlistener

Other Microsoft documentation:

- The Taskbar (taskbar creation notification): https://learn.microsoft.com/en-us/windows/win32/shell/taskbar
- Notifications and the notification area: https://learn.microsoft.com/en-us/windows/win32/shell/notification-area
- User objects (their handles are public to all processes): https://learn.microsoft.com/en-us/windows/win32/sysinfo/user-objects
- Interprocess communication on 64-bit Windows (32-bit handles): https://learn.microsoft.com/en-us/windows/win32/winprog64/interprocess-communication
- UI Automation: https://learn.microsoft.com/en-us/windows/win32/winauto/entry-uiauto-win32
- UI Automation threading issues: https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-threading
- Notification listener: https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/notification-listener
- Keyboard shortcuts in Windows: https://support.microsoft.com/en-us/windows/keyboard-shortcuts-in-windows-dcc61a57-8ff0-cffe-9796-cb9706c75eec
- Customize the taskbar notification area: https://support.microsoft.com/en-us/windows/customize-the-taskbar-notification-area-e159e8d2-9ac5-b2bd-61c5-bb63c1d437c3
- Microsoft software licence terms: https://www.microsoft.com/en-us/useterms
- Raymond Chen, *Enumerating notification icons via UI Automation*
  (2014): https://devblogs.microsoft.com/oldnewthing/20141013-00/?p=43863
- Raymond Chen, *How can I enumerate the overflow icons in the
  Notification Area without showing them?* (2025):
  https://devblogs.microsoft.com/oldnewthing/20250929-00/?p=111637

Other sources, read for facts (no code copied):

- Geoff Chappell, *Shell_NotifyIcon*: https://www.geoffchappell.com/studies/windows/shell/shell32/api/shlnot/notifyicon.htm
- ManagedShell, `TrayService.cs`, `ExplorerTrayService.cs`,
  `TrayNotify.cs`: https://github.com/cairoshell/ManagedShell/tree/master/src/ManagedShell.WindowsTray
- RetroBar (built on ManagedShell): https://github.com/dremin/RetroBar
- systray-util, `tray_spy.rs`: https://github.com/glzr-io/zebar/tree/main/crates/systray-util
- Seelen UI, tray module: https://github.com/eythaann/Seelen-UI/tree/master/src/background/system_modules/system_tray/application/windows
- `tray-icon` 0.25.1, the crate under Stem's tray icon (through Tauri),
  which allows `TaskbarCreated` through UIPI: https://docs.rs/crate/tray-icon/0.25.1/source/src/platform_impl/windows/mod.rs
