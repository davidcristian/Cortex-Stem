# File drags

Files dragged from Explorer onto the dock are pinned there, or, when dropped on the Recycle Bin
item, moved to the Recycle Bin. The drag is an OLE drag: the page gets no pointer events and no
HTML5 `dragover` or `drop` for it. Tauri's drag-drop handler, on by default for webview windows
(the dock window does not turn it off), is the drop target. It emits `tauri://drag-enter`,
`tauri://drag-over`, `tauri://drag-drop` and `tauri://drag-leave` to the window's label;
`core:default` already grants the `listen` and `unlisten` that the UI needs. The drag adds no
command or event to this contract; a drop on the bin calls `delete_to_recycle_bin`.

- **Subscription.** `Backend.onFileDrag(handler): Unsubscribe` delivers `FileDragEvent`
  (`ui/src/lib/ipc/backend.ts`): `{ type: 'enter' | 'drop', paths: string[], position }`,
  `{ type: 'over', position }` and `{ type: 'leave' }`. `position` is in CSS px relative to the
  viewport, like `MouseEvent.clientX/Y`: Tauri reports physical pixels relative to the webview,
  and `TauriBackend` divides them by `window.devicePixelRatio` (once, there). `paths` are the
  shell's paths as given. `leave` means that the drag left the window or was cancelled.
- **Where it is received.** Only while the window takes the pointer
  ([Hit region](hit-region.md)): from the moment the cursor enters `panel` until it leaves
  `envelope`. With `behavior.clickThrough` off it is received anywhere in the window, but only a
  drop over the panel pins. While a file drag is in the window, the UI reports an `envelope` one
  slot longer along the main axis (half a slot at each end) to make room for the gap below. With
  auto-hide on and the dock hidden, a file drag over the reveal strip reveals the dock, and any
  file drag in the window keeps it up until `leave` or `drop`.
- **Gap.** Over the panel itself (not the transparent margin beside it), a drag that includes
  at least one `.lnk` or `.exe` path opens a one-slot gap among the pinned apps at the insertion
  index. The insertion index is the number of pinned items whose drawn centre is before the
  pointer along the main axis: left of it on a horizontal dock, above it on a vertical one. A drag
  with nothing pinnable opens no gap.
- **Over the Recycle Bin.** While the pointer is over the Recycle Bin item's slot
  (`DockItem.kind` `recycleBin`), or over the panel's padding past it, no gap opens, whatever the
  drag contains (any file type, folders). The padding counts because the bin is the last item: a
  gap opening there would push the bin under the pointer, and the two would alternate. Instead
  the item is highlighted as hover shows an item: its icon grows a little (a transform, so
  nothing reflows) and its title pill shows (when `appearance.tooltips` is on). Elsewhere on the
  panel the drag pins as above.
- **Drop.** Over the panel, the page calls `pin_target({ kind: 'shortcut', path })` for each
  `.lnk` path and `pin_target({ kind: 'exe', path, args: [], workingDir: null })` for each
  `.exe` path, one at a time in drag order. It skips other paths and logs them to the console.
  Then it calls `reorder_pinned(order)` once with the whole pinned order: the order before the
  drop, with the returned ids, each once, at the insertion index (ids that were already pinned
  move there). A failing `pin_target` shows a failed-command toast (`Pinning <file name> failed:
  …`); the other paths still pin. There is no `reorder_pinned` when nothing was pinned. A drop
  elsewhere does nothing. Rust decides which files really are pinnable; the extension filter only
  avoids unneeded calls.
- **Drop on the Recycle Bin.** The page calls `delete_to_recycle_bin({ paths })` once with every
  path of the drop, as given; nothing is pinned. Rust skips, with a warning in its log:
  - anything that is not a plain absolute file path: a drive or UNC path with no `\\?\` or
    `\\.\` prefix, no `..`, no wildcard and no stream;
  - a drive or share root;
  - anything that does not exist.

  It drops repeats and anything inside another dropped folder, and passes the rest to the shell
  in one operation (`IFileOperation`, recycle, undoable) with the shell's own confirmation,
  progress and error dialogs. Those dialogs also warn before anything that cannot be recycled
  would be deleted permanently. The command resolves once the shell has finished, which can take
  as long as its dialogs are open. It rejects when nothing was left to move (`<path> does not
  exist`, `none of the 3 paths exists`, …), when the user cancelled, or when the shell failed or
  skipped an item (in use, access denied). What went to the bin before that stays there. A
  rejection shows a failed-command toast (`Moving <file name> to the Recycle Bin failed: …`, or
  `Moving <n> items …` for several paths). Either way, Rust then reads the bin's state once and
  emits `dock://state` if the item's icon changes, so it shows "full" without waiting for the 5 s
  poll.
- **What the UI shows in the meantime.** The item list as it was, with the gap, until a
  `dock://state` with that pinned order arrives (at most 1.5 s). Then the first new item takes
  the gap's place.
- `MockBackend` delivers file drags only through its `simulateFileDrag(event)` test hook (in CSS
  px, as `TauriBackend` would pass them on) and records `pinTarget` and `reorderPinned` requests
  in `calls`. Its `deleteToRecycleBin` records `{ type: 'deleteToRecycleBin', paths }`, treats
  every plain absolute path as existing (with the same path rules as Rust), counts those into the
  bin and emits the new state. An empty list does nothing, as in Rust.
