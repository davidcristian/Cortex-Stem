# Research: the notification area

This study asks what replacing or mirroring the notification area of
`Shell_TrayWnd` would take, and whether Stem should do it. In short:
replacing it means handling a private protocol and rebuilding shell surfaces
that have no public API. Mirroring it is possible only through undocumented
behaviour or an unsupported tree. The study recommends against both and
proposes one small step that uses public means only. Stem has most of that
step (see [Recommendation](recommendation.md)).

Claims marked **[An]** are assumptions: neither Microsoft's documentation
nor a read-only check on Windows confirmed them.
[Assumptions](assumptions.md) lists each one with a way to settle it.
[Sources](sources.md) lists every Win32 API this study relies on, with its
documentation.

## Context

Stem is a dock, not a tray replacement (the product definition in
`AGENTS.md`). `docs/PLAN.md` lists replacing the notification area as a
non-goal for now: the stock taskbar stays for the tray, the input indicator
and the clock. With *Hide the Windows taskbar* on, Stem turns on Windows'
own taskbar auto-hide with `ABM_SETSTATE`; nothing hides the
`Shell_TrayWnd` window.

## Files in this study

- [tray-protocol.md](tray-protocol.md): how an app's icon reaches the tray
  and how its events get back.
- [windows-11-tray.md](windows-11-tray.md): what a read-only check found in
  the Windows 11 notification area.
- [other-shells.md](other-shells.md): how three open-source shells get the
  tray icons.
- [options.md](options.md): six ways Stem could handle the notification
  area, A to F.
- [legal-and-maintenance.md](legal-and-maintenance.md): the legal and
  maintenance cost of the options that use the tray side.
- [recommendation.md](recommendation.md): the decision, the smallest useful
  step, and its consequences.
- [manual-tests.md](manual-tests.md): the manual test plan on Windows and
  the PowerShell snippets it uses.
- [work-items.md](work-items.md): the changes the study proposes, by number.
- [assumptions.md](assumptions.md): the claims marked **[A1]** to **[A15]**,
  each with a way to settle it.
- [sources.md](sources.md): the APIs, Microsoft documentation and other
  sources the study relies on.
