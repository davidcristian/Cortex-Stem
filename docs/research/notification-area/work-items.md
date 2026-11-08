# Work items for the notification area

This page is part of the [notification-area study](index.md): the changes
the study proposes. The other pages refer to them by number.

1. **Notification area and quick settings items.** Optional fixed items
   that send *Win+B*, *Win+A* and *Win+N* through `SendInput`, as the
   Start item does, with settings switches.
2. **The clock opens the notification centre.** A click on the clock
   widget sends *Win+N*. An input-language label from `GetKeyboardLayout`
   is a candidate widget.
3. **An ADR (the next free number): the notification area stays
   Explorer's.** Record the decision in [Recommendation](recommendation.md)
   and its revisit conditions.
4. **Point the non-goal here.** Link this study from the non-goals in
   `docs/PLAN.md` and from the user guide's FAQ.
5. **Tray spy prototype.** Only if the decision is revisited: a forwarding
   `Shell_TrayWnd` on its own platform thread, behind a developer-only
   switch, that logs icons and does nothing else, to measure completeness,
   Explorer restarts and elevated apps on Windows.
