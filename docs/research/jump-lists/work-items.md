# Work items for jump lists

This page is part of the [jump-list study](index.md): the changes the study
proposes, in order. The other pages refer to them by number, and claims
marked **A5** and so on are listed in [Assumptions](assumptions.md).

1. **An ADR (the next free number): jump lists in the dock menu.** Record
   the hybrid decision and the rejected undocumented COM route, from this
   study.
2. **Jump-list infrastructure and Recent for `aumid:` apps.** The first
   stage ([Recommendation](recommendation.md) and
   [Where the jump-list code would live](code-layout.md)): types, trait
   methods, the `stem-jump-list` thread, the service budget and the async
   `show_item_menu`.
3. **Open destinations with their own app.** `IAssocHandler::Invoke` for exe
   owners and `ActivateForFile` for packaged owners, falling back to the
   default verb.
4. **Tasks and custom categories from `customDestinations-ms`.** A pure
   framing parser and the AppID hash in `rules::jump_list` (Linux fixtures,
   the hash vectors in
   [Where the jump-list data lives](data-sources.md#the-file-name-is-a-hash-of-the-appid-measured)),
   with links loaded through `IPersistStream` on Windows.
5. **Follow the app's known-category choice.** Show *Frequent* instead of
   *Recent* when the custom file asks for `KDC_FREQUENT`, in the app's order.
6. **Pinned destinations from `DestList`.** Parse version 3 and 4 behind a
   version check, show *Pinned* first, and remove those items from *Recent*.
7. **Jump lists for `exe:` apps.** Build the implicit AppID (measure the
   known-folder rules, A5) and read the automatic file instead of the API.
8. **Explorer's AppID if its windows lack one.** A pure mapping from
   `explorer.exe` to `Microsoft.Windows.Explorer`, only if A10 fails.
9. **Packaged-app tasks with an activation context.** Launch links that have
   a `System.AppUserModel.ID` through `ActivateApplication` (for example
   Terminal's profiles).
10. **Icons on jump-list entries.** Use icon menu items with `file:` icon keys
    at menu size, within the same budget.
11. **A setting to hide jump-list entries.** Add it only if users ask: it
    needs a `Settings` field, TS bindings and a settings row.
