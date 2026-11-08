# Manual test plan for the notification area (Windows)

This page is part of the [notification-area study](index.md). Claims marked
**[A3]** and so on are listed in [Assumptions](assumptions.md).

These steps check the facts the [recommendation](recommendation.md) depends
on and settle the assumptions a person can settle. Write down the Windows
build first. The snippets are in the
[appendix](#appendix-inspection-snippets) below; run each in a new
PowerShell window. Their output includes tooltip text, which can contain
personal details: summarise it, do not paste it into an issue.

1. Run parts 1 and 2. Expected: the first line reads `TrayNotifyWnd has a
   child window: False`. The Windows 10 toolbar does not exist.
2. Look at the lines part 2 printed. Expected: one `NotifyItemIcon` line
   per visible app icon and `SystemTrayIcon` lines for the system items,
   all with Explorer's pid, `hwnd=0` and only `Invoke,ScrollItem`; the
   quoted text is each icon's tooltip.
3. Click the *Show hidden icons* chevron and leave the flyout open. In a
   new window, run parts 1 and 2 with the `$root` line replaced as the
   comment says. Expected: the hidden icons are listed. Close the flyout
   and run it again: nothing is listed.
4. Turn on *Hide the Windows taskbar* in Stem's settings, wait for the
   taskbar to slide away and run parts 1 and 2. Expected: the same lines,
   with rectangles that start a pixel or two above the bottom edge of the
   screen.
5. In Accessibility Insights for Windows or Inspect (Windows SDK), select
   Stem's own tray icon and run its Invoke pattern. Expected: the dock
   hides or shows, as on a left click on the tray icon **[A4]**.
6. With the taskbar auto-hidden, press *Win+B*. Expected: the taskbar
   slides up with the focus on the first notification icon; *Enter* acts
   like a click; *Esc* lets the taskbar hide again **[A3]**.
7. With the taskbar auto-hidden, press *Win+A*, then *Esc*, then *Win+N*.
   Expected: quick settings opens, then the notification centre with the
   calendar, both at the bottom-right corner.
8. With Stem running, restart Explorer (Task Manager → *Windows Explorer*
   → *Restart*). Expected: Stem's tray icon comes back within a few
   seconds. Note any app whose icon does not come back.
9. If you use an app that shows a tray icon while it runs as
   administrator, start it that way and restart Explorer as in step 8.
   Expected (A11): its icon comes back only if the app allows
   `TaskbarCreated` through UIPI. Record the app and the result.
10. With the taskbar auto-hidden, run part 3. Expected: Windows shows the
    balloon as a notification banner while the taskbar stays hidden.

Record anything that differs, in particular a `TrayNotifyWnd` with
children (step 1), icons that report a window handle (step 2) or hidden
icons listed with the flyout closed (step 3): each would weaken a reason
given in [Options](options.md).

## Appendix: inspection snippets

Parts 1 and 2 only read; they click nothing.

```powershell
# Parts 1 and 2: run together in a new PowerShell window.
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type -Namespace StemProbe -Name W -MemberDefinition @'
[DllImport("user32.dll", CharSet = CharSet.Unicode)]
public static extern IntPtr FindWindowExW(IntPtr parent, IntPtr after,
    string cls, IntPtr title);
'@
$Z = [IntPtr]::Zero

# Part 1: does the Windows 10 tray toolbar still exist?
$tray = [StemProbe.W]::FindWindowExW($Z, $Z, 'Shell_TrayWnd', $Z)
$notify = [StemProbe.W]::FindWindowExW($tray, $Z, 'TrayNotifyWnd', $Z)
$child = [StemProbe.W]::FindWindowExW($notify, $Z, [NullString]::Value, $Z)
"TrayNotifyWnd has a child window: $($child -ne $Z)"

# Part 2: the tray's UI Automation elements. For step 3, replace the
# $root line with:
#   $root = [StemProbe.W]::FindWindowExW($Z, $Z,
#       'TopLevelWindowForOverflowXamlIsland', $Z)
$root = [StemProbe.W]::FindWindowExW($tray, $Z,
    'Windows.UI.Composition.DesktopWindowContentBridge', $Z)
$all = [System.Windows.Automation.AutomationElement]::FromHandle($root).FindAll(
    'Descendants', [System.Windows.Automation.Condition]::TrueCondition)
foreach ($e in $all) {
  $c = $e.Current
  if ($c.AutomationId -in 'NotifyItemIcon', 'SystemTrayIcon') {
    $p = ($e.GetSupportedPatterns() | ForEach-Object {
      $_.ProgrammaticName -replace 'PatternIdentifiers.Pattern' }) -join ','
    '{0,-15} {1,-28} pid={2} hwnd={3} [{4}] {5} "{6}"' -f $c.AutomationId,
      $c.ClassName, $c.ProcessId, $c.NativeWindowHandle, $p,
      $c.BoundingRectangle, ($c.Name -replace '\s+', ' ')
  }
}
```

`[NullString]::Value` is there because PowerShell turns `$null` into an
empty string for a `string` parameter, and an empty class name matches no
window.

```powershell
# Part 3: a test balloon from a throwaway notify icon.
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$n = New-Object System.Windows.Forms.NotifyIcon
$n.Icon = [System.Drawing.SystemIcons]::Information
$n.Text = 'Stem tray test'
$n.Visible = $true
$n.ShowBalloonTip(5000, 'Stem tray', 'Test balloon', 'Info')
Start-Sleep -Seconds 8
$n.Dispose()
```
