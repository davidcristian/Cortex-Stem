# Launching a jump-list entry

This page is part of the [jump-list study](index.md): how Stem would open an
entry the user chooses from an app's jump list. Claims marked **A6** and so
on are listed in [Assumptions](assumptions.md).

- **An `IShellItem` destination.** The stock list opens it "much like …
  Open With": with the app the list belongs to.
  - The simplest route is the default verb: `ShellExecuteExW` with
    `SEE_MASK_INVOKEIDLIST` and the item's ID list from
    `SHGetIDListFromObject`. Open-Shell does this (with `SEE_MASK_IDLIST`).
    It runs the file type's default handler, which may be a different app
    (**A12**).
  - Opening with the owner, when the owner is an exe: `SHAssocEnumHandlers`
    for the extension, then the handler whose `IAssocHandler::GetName` (the
    handler's full exe path) equals the owner's exe, then
    `IAssocHandler::Invoke`. When the owner is packaged:
    `IApplicationActivationManager::ActivateForFile` (**A6**).
- **An `IShellLink`** (a task, a custom item, some recent items). Read
  `IShellLinkW::GetPath` (expanded, not `SLGP_RAWPATH`), `GetArguments` and
  `GetWorkingDirectory`. Launch that as a `LaunchTarget::Exe` through the
  existing `launch::shell_execute`, which is what the link would run. Never
  call `IShellLinkW::Resolve`: it searches for moved targets and can access
  the network.
  - A link with no file-system path (an ID list only) needs `GetIDList` and
    `SEE_MASK_IDLIST`.
  - A packaged app's task can have a `System.AppUserModel.ID` and an
    activation context. That needs `IApplicationActivationManager::
    ActivateApplication`, with the context as its arguments (Open-Shell's
    route). This is a later stage ([work item 9](work-items.md)).
- Every launch runs on Tauri's blocking pool, like the menu's other actions
  (`menus/actions.rs` `perform`). The app's icon bounces, as for *New window*
  (`ServiceEvent::Bounce`).
