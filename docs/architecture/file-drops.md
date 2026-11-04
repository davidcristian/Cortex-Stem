# File drops

This file describes what the dock does with files dragged onto it from Explorer: it pins
them.

A file dragged from Explorer is an OLE drag, which Tauri's drag-drop
handler receives; no pointer events reach the page. `Backend.onFileDrag`
converts its `tauri://drag-*` events to CSS px
([`docs/ipc/file-drags.md`](../ipc/file-drags.md)). The window is a drop
target only while the [hit-test thread](threads.md) lets it take the
pointer, so a drag counts from the moment the cursor enters the panel.

`Dock.svelte` measures where the slots are drawn and asks the pure module
`src/lib/dock/filedrop.ts` (unit-tested) what to do. The item under the
pointer and the index of the pin gap are kept apart, so that an item could
take a drop instead of the gap. The gap is a real `.drop-gap`
element in the row. It grows and shrinks like an item (160 ms, the speed of
the reorder gap), so the panel widens around it. No rest measurement is
taken while it exists, and the
[envelope](dock-window.md#click-through-outside-the-panel) is one slot wider
during the drag.

The drop calls `pin_target` for each `.lnk` or `.exe` path, then
`reorder_pinned` once. The rendered item list is held until the snapshot
with that order arrives, so the new icons appear in the gap and not first
at the end of the pinned group. A file drag holds
[auto-hide](auto-hide.md) (`fileDrag`), and over the reveal strip it
reveals the dock.
