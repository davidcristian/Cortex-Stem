# Legal and maintenance cost

This page is part of the [notification-area study](index.md): what the
[options](options.md) that depend on the tray side of Windows would cost in
legal risk and upkeep. Claims marked **[A15]** are listed in
[Assumptions](assumptions.md).

- **Microsoft's side.** The client side (`Shell_NotifyIconW`, the callback
  message) is documented. The tray side is not: the `WM_COPYDATA` layouts,
  the `FindWindow` order and `ITrayNotify` come from other projects'
  reading of Windows, and no document says they stay the same. Windows'
  licence terms forbid working around technical limitations in the
  software. A window that handles public messages is probably not that;
  code injected into Explorer is nearer to it **[A15]**. This is not legal
  advice.
- **Other projects' code.** Stem is AGPL-3.0-only. Reading other projects
  for facts is fine. Copied code needs a compatible licence and its
  notices: MIT (systray-util) and Apache-2.0 (ManagedShell) code can be
  included with its notices, and AGPL-3.0 (Seelen UI) code as it is.
  GPL-2.0-only code (ExplorerPatcher, for example) cannot **[A15]**.
- **Maintenance.** Microsoft rewrites the taskbar from time to time:
  Windows 11 dropped the toolbar that every Windows 10 tray tool read. A
  mirror needs a test run on Windows after each feature update, and none of
  the project's automated checks would detect a break.
