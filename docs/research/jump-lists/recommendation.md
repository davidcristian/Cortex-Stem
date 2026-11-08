# Recommendation for jump lists

This page is part of the [jump-list study](index.md). It names the chosen
option from [Options](options.md) and the scope of its first stage. Claims
marked **A8** and so on are listed in [Assumptions](assumptions.md).

Take **D**. The **first stage** builds the thread, the time budget, the
async menu and the launch path, and shows *Recent* through the documented
API, for `aumid:` apps only. It tests that infrastructure on a supported API,
so every later stage only adds a source.

Scope of the first stage:

- An app item's menu gets a *Recent* section on top: a disabled heading
  item, then up to 10 entries from `GetList(ADLT_RECENT, 10)`, then a
  separator, then the current entries unchanged. The menu opens upwards from
  a bottom dock, so the app commands stay nearest the pointer, as on the
  stock taskbar.
- Choosing an entry opens it with the default verb. Opening with the owner
  is [work item 3](work-items.md).
- Apps with an `exe:`, `class:` or `uri:` id, fixed items and separators get
  no section. Their menus are unchanged.
- If `SHRestricted(REST_NORECENTDOCSHISTORY)` is non-zero, nothing is fetched
  (**A8**).

[Where the jump-list code would live](code-layout.md) describes the code of
this stage, and [Consequences](consequences.md) what it changes.
