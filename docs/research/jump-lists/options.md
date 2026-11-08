# Options for reading jump lists

This page is part of the [jump-list study](index.md): the four ways Stem
could read jump lists, compared. The sources they read are described in
[Where the jump-list data lives](data-sources.md), and claims marked **A1**
and so on are listed in [Assumptions](assumptions.md).

**A. Documented API only (`IApplicationDocumentLists`).**
- For: documented and supported. Little code. One COM object and two calls.
- Against: Recent and Frequent only. No tasks, so browsers, Terminal,
  PowerShell and VS Code show nothing new. No pinned items, and pinned items
  in the list cannot be told apart. `exe:` apps get nothing (A1).

**B. Read both files directly.**
- For: everything the stock list shows (pinned, recent, frequent, custom
  categories, tasks), for every AppID, implicit ones included (through the
  hash). The parsers are pure Rust and can be tested on Linux with fixture
  bytes. The shell's own link loader reads the MS-SHLLINK blobs.
- Against: `DestList` and the custom container are undocumented and
  versioned, so a Windows update can break them. It is more code. Each
  parser must fail closed (return no entries on any input it does not
  recognise).

**C. Undocumented COM.** Rejected (see
[source 4](data-sources.md#4-undocumented-com-rejected)).

**D. Hybrid, in stages.** Use A for Recent of `aumid:` apps. Use B's custom
file for tasks and custom categories. Later, use `DestList` for pinned items
and for `exe:` apps.
- For: the first stage delivers a working feature on the documented API.
  Each undocumented parser is added on its own, with its own version and
  framing checks, and adds a menu section without changing the others.
- Against: two code paths for Recent (A for `aumid:`, B for `exe:`) until
  the `DestList` parser passes its manual tests on Windows and can replace A.

| | A | B | D (all stages) |
|---|---|---|---|
| Recent / Frequent | `aumid:` only | all | all |
| Pinned | no | yes (`DestList`) | yes, [work item 6](work-items.md) |
| Tasks, custom categories | no | yes | yes, [work item 4](work-items.md) |
| Undocumented formats | none | two | two, added one at a time |
| Linux-testable logic | little | parsers, hash | parsers, hash, menu shaping |
