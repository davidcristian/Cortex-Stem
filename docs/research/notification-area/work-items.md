# Work items for the notification area

This page is part of the [notification-area study](index.md): the changes
the study proposes. The other pages refer to them by number.

1. **A notification area item.** An optional fixed item that sends
   *Win+B* through `SendInput`, as the Quick Settings and Notification
   Center items send *Win+A* and *Win+N*, with a settings switch.
2. **An input-language widget.** A label from `GetKeyboardLayout` is a
   candidate widget.
3. **An ADR (the next free number): the notification area stays
   Explorer's.** Record the decision in [Recommendation](recommendation.md)
   and its revisit conditions.
4. **Tray spy prototype.** Only if the decision is revisited: a forwarding
   `Shell_TrayWnd` on its own platform thread, behind a developer-only
   switch, that logs icons and does nothing else, to measure completeness,
   Explorer restarts and elevated apps on Windows.
