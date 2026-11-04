# File drags

Files dragged from Explorer onto the dock are pinned there. The drag is an OLE drag: the page
gets no pointer events and no HTML5 `dragover` or `drop` for it. Tauri's drag-drop handler, on by
default for webview windows (the dock window does not turn it off), is the drop target. It emits
`tauri://drag-enter`, `tauri://drag-over`, `tauri://drag-drop` and `tauri://drag-leave` to the
window's label; `core:default` already grants the `listen` and `unlisten` that the UI needs. The
drag adds no command or event to this contract.

- **Subscription.** `Backend.onFileDrag(handler): Unsubscribe` delivers `FileDragEvent`
  (`src/lib/ipc/backend.ts`): `{ type: 'enter' | 'drop', paths: string[], position }`,
  `{ type: 'over', position }` and `{ type: 'leave' }`. `position` is in CSS px relative to the
  viewport, like `MouseEvent.clientX/Y`: Tauri reports physical pixels relative to the webview,
  and `TauriBackend` divides them by `window.devicePixelRatio` (once, there). `paths` are the
  shell's paths as given. `leave` means that the drag left the window or was cancelled.
- **Where it is received.** Only while the window takes the pointer
  ([Hit region](hit-region.md)): from the moment the cursor enters `panel` until it leaves
  `envelope`. With `behavior.clickThrough` off it is received anywhere in the window, but only a
  drop over the panel pins. While a file drag is in the window, the UI reports an `envelope` one
  slot wider (half a slot at each end) to make room for the gap below. With auto-hide on and the
  dock hidden, a file drag over the reveal strip reveals the dock, and any file drag in the
  window keeps it up until `leave` or `drop`.
- **Gap.** Over the panel itself (not the transparent margin above it), a drag that includes at
  least one `.lnk` or `.exe` path opens a one-slot gap among the pinned apps at the insertion
  index. The insertion index is the number of pinned items whose drawn centre is left of the
  pointer. A drag with nothing pinnable opens no gap.
- **Drop.** Over the panel, the page calls `pin_target({ kind: 'shortcut', path })` for each
  `.lnk` path and `pin_target({ kind: 'exe', path, args: [], workingDir: null })` for each
  `.exe` path, one at a time in drag order. It skips other paths and logs them to the console.
  Then it calls `reorder_pinned(order)` once with the whole pinned order: the order before the
  drop, with the returned ids, each once, at the insertion index (ids that were already pinned
  move there). A failing `pin_target` shows a failed-command toast (`Pinning <file name> failed:
  …`); the other paths still pin. There is no `reorder_pinned` when nothing was pinned. A drop
  elsewhere does nothing. Rust decides which files really are pinnable; the extension filter only
  avoids unneeded calls.
- **What the UI shows in the meantime.** The item list as it was, with the gap, until a
  `dock://state` with that pinned order arrives (at most 1.5 s). Then the first new item takes
  the gap's place.
- `MockBackend` delivers file drags only through its `simulateFileDrag(event)` test hook (in CSS
  px, as `TauriBackend` would pass them on) and records `pinTarget` and `reorderPinned` requests
  in `calls`.
