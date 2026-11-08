# Assumptions of the notification-area study

This page is part of the [notification-area study](index.md): the claims
that the other pages mark **[A1]** to **[A15]**, each with a way to settle
it. The test steps are in the [manual test plan](manual-tests.md), and the
letters A to F name the [options](options.md).

- **A1**: The `WM_COPYDATA` layouts and `dwData` values (1 tray, 0 app
  bar, 3 icon rectangle) are as ManagedShell and systray-util define them.
  Undocumented. Settle: only by building D.
- **A2**: None of Windows 11's own corner items (clock, quick settings,
  input indicator) arrives as a `Shell_NotifyIconW` icon. They were seen
  as the taskbar's own XAML buttons; not traced at the message level.
- **A3**: *Win+B* brings up an auto-hidden taskbar. Microsoft's shortcut
  list says it sets the focus and says nothing about the reveal. Settle:
  test step 6.
- **A4**: Invoke on a `NotifyItemIcon` acts as a left click. Nothing was
  invoked during the check. Settle: step 5.
- **A5**: `IconSnapshot` is the last image Windows stored, not the live
  icon. The PNG format was seen; when Windows updates it is not known.
- **A6**: Windows 11 24H2 lets the user hide the overflow button. A
  reader's report under Chen's 2025 post; not checked.
- **A7**: UI Automation structure-changed events from the XAML tray are
  reliable enough to track icons being added and removed. Not checked.
- **A8**: With two `Shell_TrayWnd` windows, `FindWindow` returns the one
  that comes first in z-order. Not documented; ManagedShell and
  systray-util rely on it.
- **A9**: A WinEvent (a window created or reordered) can report that
  another `Shell_TrayWnd` moved ahead of Stem's, instead of a poll. Not
  tried.
- **A10**: `ITrayNotify` still reports the existing icons on Windows 11.
  Undocumented COM; not tried.
- **A11**: A `TaskbarCreated` broadcast from a medium-integrity process
  does not reach an elevated app unless the app allowed the message with
  `ChangeWindowMessageFilterEx`. It follows from the UIPI rule, but is not
  documented for this message. Settle: step 9.
- **A12**: shell32 adds an icon's callback message to the calling
  process's message filter, so a medium-integrity tray can deliver clicks
  to elevated apps. From Chappell's study of the Windows Vista shell32;
  not checked on Windows 11.
- **A13**: Security software treats a DLL hooked into Explorer as
  suspicious. Not measured.
- **A14**: An unpackaged app cannot use `UserNotificationListener`. The
  documentation requires a package manifest capability; not tried.
- **A15**: The licence readings in
  [Legal and maintenance cost](legal-and-maintenance.md) (Windows' terms,
  combining licences with AGPL-3.0). Not legal advice; ask before copying
  code or shipping anything that loads into Explorer.
