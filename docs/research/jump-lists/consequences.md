# Consequences of the jump-list design

This page is part of the [jump-list study](index.md): what the
[recommended design](recommendation.md) changes for users and for the code.
Claims marked **A8** and so on are listed in [Assumptions](assumptions.md).

- An app's menu can open up to 150 ms later than it does now, on every open,
  because each open fetches a fresh list. The budget is an estimate until
  step 12 of the [test plan](manual-tests.md) measures it.
- One new thread. It blocks on its channel when idle, so idle CPU use does
  not change and nothing polls
  ([Performance budgets](../../architecture/performance.md)).
- In the first stage a recent item opens with its default handler. When
  that handler is another app, the result differs from the stock jump list,
  which opens the item with the owner. [Work item 3](work-items.md) fixes
  this.
- The first stage shows no pinned items and no tasks, and nothing for
  `exe:` apps. Browser menus stay as they are.
- No IPC payload change and no new setting. Windows' own privacy settings
  control whether Recent is shown (A8, A9).
- **Security.** Entries are data that processes running as the user wrote.
  Stem reads them only on a right-click, never runs anything while reading,
  and launches only on an explicit choice. That is the trust level of the
  stock jump list, which launches the same entries, and of a desktop
  shortcut ([Security](../../architecture/security.md)). Later parsers are bounded,
  fail closed and are tested against fixtures. The link bytes go to the
  shell's own loader, as they do for Explorer.
- **Privacy.** Document names appear in Stem's menu, as they do in the stock
  list. Logs record counts at debug and titles only at trace, because file
  logging writes to disk.
- Later stages depend on undocumented formats, so a Windows update can
  empty a section. The failure mode is "no entries", never a crash or a
  wrong launch.
