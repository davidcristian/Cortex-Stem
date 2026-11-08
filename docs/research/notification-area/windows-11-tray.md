# What Windows 11 has in the notification area

This page is part of the [notification-area study](index.md): what the
Windows 11 taskbar contains where the notification icons are. Claims marked
**[A2]** and so on are listed in [Assumptions](assumptions.md).

Read-only checks on one Windows 11 PC (26H2, build 26300), from PowerShell:
`EnumChildWindows`, `FindWindowExW`, UI Automation and the registry. Nothing
was clicked or invoked.

- The taskbar's content is a XAML island inside `Shell_TrayWnd` (child
  window class `Windows.UI.Composition.DesktopWindowContentBridge`). The
  Win32 `TrayNotifyWnd` still exists but has **no child windows**: the
  Windows 10 toolbar (`SysPager` → `ToolbarWindow32`) that older tray tools
  read with `TB_GETBUTTON` does not exist.
- In the UI Automation tree each app icon is a `Button` of class
  `SystemTray.NormalButton` with AutomationId `NotifyItemIcon`. Its name is
  the tooltip text. The system items (*Show Hidden Icons*, the input
  indicator, network, volume, battery, the clock, notifications, *Show
  Desktop*) are buttons with AutomationId `SystemTrayIcon` and classes of
  their own. They are the taskbar's own controls, not notify icons
  **[A2]**.
- All of them support only the Invoke and ScrollItem patterns, report
  Explorer's process id and have no window handle. Nothing exposes the
  owner window, the executable, the icon id or the image.
- The overflow flyout is a top-level window of its own, class
  `TopLevelWindowForOverflowXamlIsland`. While it is closed its tree is
  empty.
- With the taskbar auto-hidden (what *Hide the Windows taskbar* sets), the
  elements stay in the tree with rectangles where the hidden taskbar is:
  2 px on screen, the rest below the bottom edge.
- `HKCU\Control Panel\NotifyIconSettings` has one subkey per icon Windows
  has seen, with `ExecutablePath`, `UID` or `IconGuid`, `InitialTooltip`,
  `IsPromoted`, `Publisher` and `IconSnapshot` (PNG bytes). None of it is
  documented.

Next to the app icons, the corner contains Windows' own surfaces: the
clock, the calendar and the notification centre (*Win+N*), quick settings
with network, volume and battery (*Win+A*), the input indicator
(*Win+Space* switches layouts) and *Show Desktop*. Microsoft documents the
shortcuts, not a way to host these surfaces anywhere else.
