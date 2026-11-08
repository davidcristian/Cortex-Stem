# Recommendation for the notification area

This page is part of the [notification-area study](index.md): the decision,
the smallest useful step, and their consequences. The letters A to F name
the [options](options.md).

**Against.** Stem neither replaces nor mirrors the notification area. The
non-goal in `docs/PLAN.md` stays, for these reasons: replacing it (F)
means writing a shell; the hook (E) puts Stem code inside Explorer; the
spy window (D) puts Stem in the path of every app's tray call and depends
on undocumented behaviour; UI Automation (C) gives icons without images or
menus.

**Smallest useful step, on public means only: B.** Optional items for the
notification area (*Win+B*), quick settings (*Win+A*) and the notification
centre (*Win+N*), and a clock widget that opens the notification centre on
click. They bring the hidden taskbar's corner within one click of the dock
without touching the tray.

Revisit if Microsoft documents a way to host notification icons, if the
stock taskbar can no longer auto-hide, or if users ask for tray icons in
the dock. The only route then worth a prototype is D, forwarding and
log-only first ([work item 5](work-items.md)).

## Consequences

- The study adds no Win32 code.
- The stock taskbar keeps the tray, the clock, quick settings and the
  input indicator. *Hide the Windows taskbar* stays the way to hide it.
- Stem's own tray icon stays an ordinary notify icon in Explorer's tray.
- The non-goals in `docs/PLAN.md` and the user guide's
  [FAQ](../../user-guide/faq.md) can link to this study.
