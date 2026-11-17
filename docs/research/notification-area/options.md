# Options for the notification area

This page is part of the [notification-area study](index.md): six ways Stem
could handle the notification area, from leaving it to Explorer (A) to
replacing it (F). Claims marked **[A1]** and so on are listed in
[Assumptions](assumptions.md).

## A. Leave the tray to Explorer (the current behaviour)

The stock taskbar keeps the tray, the clock and quick settings. With *Hide
the Windows taskbar* it auto-hides and slides up when the pointer touches
the bottom edge beside the dock.

- No cost, and no effect on any app's tray icon.
- To reach the tray, the user reveals the taskbar and goes to its far
  corner. The tray is not in the dock.

## B. Shortcut items that open Windows' own surfaces

Optional fixed items, or clicks on the widgets, that send documented
shortcuts with `SendInput`, as the Start item sends the Windows key:
*Win+B* puts the focus on the first notification icon, which
should also bring up an auto-hidden taskbar **[A3]**; *Win+A* opens quick
settings; *Win+N* opens the notification centre and calendar.

- Public means only. No new thread, no new interface.
- No icons in the dock, and Windows still opens its flyouts at its corner.
- Injected keys have the same limits as the Start item's
  (`windows/shell.rs`).
- Stem has the *Win+A* and *Win+N* parts: the optional Quick Settings and
  Notification Center items (`items.quickSettings`,
  `items.notificationCenter`) and the clock item (`items.clock`), whose
  click sends *Win+N*
  ([Clock and battery items](../../ipc/widget-items.md#clock-and-battery-items-v122)).
  No item sends *Win+B*.

## C. Display-only mirror through UI Automation

A platform thread walks the tray's UI Automation tree, the dock shows the
icons, and a click calls Invoke.

- It gives order, tooltip text and a rectangle. Invoke probably acts as a
  left click **[A4]**.
- It does not give the image. No pattern exposes pixels, and a screen
  capture needs the taskbar on screen, which Stem can hide. The undocumented
  `IconSnapshot` PNG could be used instead, matched by tooltip text. That
  match is unreliable, and the PNG may be older than the live icon **[A5]**.
- No right-click. There is no pattern for the icon's menu, and the owner
  window is not exposed, so Stem cannot send `WM_CONTEXTMENU` itself.
- Hidden icons are in the tree only while the overflow flyout is open
  (Raymond Chen, 2025; this matches the
  [check on Windows 11](windows-11-tray.md)). Reading them means opening
  Windows UI on screen, and a user may have hidden the overflow button
  altogether **[A6]**.
- It is not an API. Chen's 2014 sample says the accessibility tree is not
  part of the API, and the tree changed completely between Windows 10 and
  11.
- A UI Automation client needs its own MTA thread that creates no windows.
  Tracking changes would depend on structure-changed events from Explorer's
  XAML **[A7]**.

Conclusion: a copy of the tray without images or menus does not justify a
new thread and a dependency on a tree that changes between Windows
versions.

## D. A forwarding `Shell_TrayWnd` (the "spy window")

systray-util calls this design a tray spy (`tray_spy.rs`). Stem creates its
own top-level window of class `Shell_TrayWnd`, keeps it first for
`FindWindow` **[A8]**, reads each `WM_COPYDATA` **[A1]**, forwards every
message to Explorer's tray and returns Explorer's result. Explorer's tray
stays complete. Stem gets the owner window, id, callback message, version,
`HICON`, tooltip and state of every icon, and it can send clicks the
documented way.

In its favour:

- Real interaction: left, right and double clicks through the callback
  message, with `AllowSetForegroundWindow` (the owner's process from
  `GetWindowThreadProcessId`) so the app's menu can take the foreground.
- The icons can be drawn. Handles to user objects are public to every
  process in the session, and the 32-bit handle value in the message is
  enough on 64-bit Windows. `GetIconInfo` and `GetDIBits` turn them into
  pixels, as `windows/icons/bitmap.rs` already does for window icons.
- A crash leaves the tray intact. If Stem exits unexpectedly (release
  builds abort on a panic), its window is destroyed, `FindWindow` returns
  Explorer's again, and Explorer still has every icon because everything
  was forwarded.

Against it:

- **Stem is in the path of every tray and app-bar call in the session.**
  shell32 waits seconds for the result. A slow or stuck spy thread stalls
  every `Shell_NotifyIconW` and `SHAppBarMessage` call in the session. The
  [Windows engineering rules](../../development/windows-rules.md) limit
  cross-process sends to 200 ms. Forwarding with that limit fails calls
  that Explorer handles slowly; a longer limit makes every app wait behind
  one slow call.
- **Competition for z-order.** Which of two `Shell_TrayWnd` windows
  `FindWindow` returns is not documented. Both implementations in
  [What other shells do](other-shells.md) poll every 100 ms to stay first,
  which breaks the idle budget in
  [Performance budgets](../../architecture/performance.md). The events
  thread's existing WinEvent hooks might replace the poll **[A9]**.
- **Icons that already exist.** Stem has to broadcast `TaskbarCreated` so
  apps add theirs again. Apps that handle it incorrectly lose their icons or
  show them twice, as they do after an Explorer restart. The Windows 10
  read of Explorer's toolbar does not work on Windows 11, and `ITrayNotify`
  is undocumented and untested there **[A10]**.
- **Elevated apps.** UIPI (User Interface Privilege Isolation) drops
  messages from a lower integrity level, so Stem's `TaskbarCreated` likely
  misses elevated apps unless they allow it **[A11]**. Their existing icons
  stay missing until they change them. The `tray-icon` crate under Stem's
  own tray icon calls `ChangeWindowMessageFilterEx` for exactly this
  reason. Clicks probably do reach elevated apps, because shell32 allows
  the callback message through for the calling process **[A12]**.
- **Explorer restarts.** Explorer's new tray can be first for `FindWindow`
  until Stem raises its window again. Icons added in that gap go to
  Explorer only, so Stem must broadcast again and accept a second round of
  re-adds.
- **Flyouts open at the corner.** Apps place their flyouts with
  `Shell_NotifyIconGetRect` or the click anchor. Forwarding returns
  Explorer's hidden rectangle. Returning the dock's rectangle means
  handling one more undocumented message.
- **Not the whole corner.** The clock, quick settings and the input
  indicator are not notify icons on Windows 11 **[A2]**, so the forwarded
  messages do not include them. Which icons are promoted is the user's
  setting, in an undocumented key; Stem would need its own rule for which
  icons to show.
- **Its own traffic.** Stem's tray icon (`src-tauri/src/tray.rs`), its
  taskbar calls (`windows/taskbar.rs`) and its app-bar calls
  (`windows/appbar/win32_shell.rs`) would go through Stem's own window, from
  other threads of the same process.
- **Testing.** Each of these behaviours exists only at run time on
  Windows: a dozen manual tests that no check on Linux can replace, to be
  run again after every Windows feature update.

## E. A hook DLL inside Explorer

This is Seelen UI's route: a DLL loaded into Explorer receives the messages
that reach the real tray. No z-order competition, no second window.

- Stem code would run inside the shell. A crash or a hang there also stops
  the taskbar, Start and the desktop.
- It is a second binary per architecture, outside the crate boundaries that
  `AGENTS.md` sets (all Win32 code in `crates/stem-platform/src/windows/`),
  and a DLL hooked into Explorer is what security software looks for
  **[A13]**.
- It is closer to working around the software than to using its
  interfaces (see [Legal and maintenance cost](legal-and-maintenance.md)).

## F. Replace the notification area

Stem would handle the tray and app-bar messages itself, without
forwarding, and keep Explorer's tray from receiving them. Everything in D
applies, and more. Stem becomes the app-bar server (work area and edges
for every app bar in the session, the dock's own included). It must
rebuild the clock and calendar, quick settings, the notification centre,
the input switcher and *Show Desktop*, and Windows offers no way to host
those surfaces. The notification listener API requires a capability in a
package manifest, which an unpackaged app like Stem does not have
**[A14]**. Explorer still has to run for Start and search. That is a
shell, not a dock.
