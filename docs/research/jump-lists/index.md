# Research: jump lists and recent documents in the dock menu

This study is a proposal: Stem has no jump-list code. It is the input for an
ADR ([work item 1](work-items.md)). API documentation links are collected in
[Sources](sources.md). Claims that no documentation or check confirmed are
marked **A1**, **A3** and so on, and listed in [Assumptions](assumptions.md).
Claims marked *measured* were checked on one Windows 11 PC (build 26300) by
listing the names of its jump-list files. No file contents were read, except
three header-only files (12 and 24 bytes) and the first 48 bytes of one task
list.

## Context

The stock taskbar shows an app's **jump list** when the user right-clicks its
button. Stem's right-click menu does not. `src-tauri/src/menus.rs` builds that
menu again on every open, from the service's snapshot and settings. For an
app item it lists the app's windows, *New window*, *Pin to Stem* or *Unpin
from Stem*, *Hide from Stem* (unpinned apps only) and *Close all windows*,
then a separator, *Stem settings…* and *Quit Stem*
([Context menus](../../ipc/context-menus.md)). The macOS dock shows an app's
recent documents in the same place.

A jump list has three kinds of content (Taskbar Extensions):

- **Destinations** are things: files, folders, URLs. The shell builds the
  *Recent* and *Frequent* lists from `SHAddToRecentDocs` calls. Explorer and
  the common file dialog make those calls for the app. The user can pin a
  destination, which moves it to *Pinned*.
- **Custom categories** are destinations the app picks itself
  (`ICustomDestinationList::AppendCategory`), such as a browser's *Most
  visited*.
- **Tasks** are actions. They are almost always `IShellLink`s with
  arguments (`ICustomDestinationList::AddUserTasks`), such as *New InPrivate
  window*. They show even when the app is not running.

Each list belongs to an **AppUserModelID** (AppID). Stem already reads the
AppID of many apps. An `aumid:` id comes from the window's
`System.AppUserModel.ID` property, the process's package identity, or the
same property on a pinned shortcut
([Identity and grouping](../../architecture/identity.md)). An `exe:` id
means Stem found no explicit AppID. The shell then uses an implicit AppID,
which "applications cannot retrieve" (AppUserModelIDs).

Constraints:

- Menus are native and built in Rust. The page never receives the entries.
- `show_item_menu` is a synchronous Tauri command, so it runs on the main
  thread. That thread also runs the dock window's message loop. Nothing that
  can block may run there (the
  [Windows engineering rules](../../development/windows-rules.md);
  [Threads](../../architecture/threads.md)).
- Shell COM needs an apartment: `util::ComApartment::sta()`, once per thread.
  Every query is bounded in time and size.
- Win32 and `unsafe` stay in `crates/stem-platform/src/windows/`.

## Files in this study

- [data-sources.md](data-sources.md): where Windows stores jump lists, the
  file-name hash, and which apps fill which source.
- [launching.md](launching.md): how Stem would open an entry the user
  chooses.
- [options.md](options.md): the four ways to read jump lists, compared.
- [recommendation.md](recommendation.md): the chosen option and the scope of
  its first stage.
- [code-layout.md](code-layout.md): where the first stage's code would live,
  and how it meets the Windows engineering rules.
- [consequences.md](consequences.md): what the design changes for users and
  for the code.
- [assumptions.md](assumptions.md): the claims marked **A1** and so on.
- [manual-tests.md](manual-tests.md): the manual test plan for the first
  stage on Windows.
- [work-items.md](work-items.md): the changes the study proposes, by number.
- [sources.md](sources.md): Microsoft documentation and other sources.
