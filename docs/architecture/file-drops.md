# File drops

This file describes what the dock does with files dragged onto it from Explorer: it pins
them, or moves them to the Recycle Bin.

A file dragged from Explorer is an OLE drag, which Tauri's drag-drop
handler receives; no pointer events reach the page. `Backend.onFileDrag`
converts its `tauri://drag-*` events to CSS px
([`docs/ipc/file-drags.md`](../ipc/file-drags.md)). The window is a drop
target only while the [hit-test thread](threads.md) lets it take the
pointer, so a drag counts from the moment the cursor enters the panel.

`src/dock/file-drag.svelte.ts` measures where the slots are drawn and asks
the pure module `src/lib/dock/filedrop.ts` (unit-tested) what to do. The
item under the pointer and the index of the pin gap are kept apart, so the
Recycle Bin can take a drop instead of the gap (below). The gap is a real
`.drop-gap` element in the row. It grows and shrinks like an item (160 ms,
the speed of the reorder gap), so the panel widens around it. No rest
measurement is taken while it exists, and the
[envelope](dock-window.md#click-through-outside-the-panel) is one slot wider
during the drag.

The drop calls `pin_target` for each `.lnk` or `.exe` path, then
`reorder_pinned` once. The rendered item list is held until the snapshot
with that order arrives, so the new icons appear in the gap and not first
at the end of the pinned group. A file drag holds
[auto-hide](auto-hide.md) (`fileDrag`), and over the reveal strip it
reveals the dock.

## Drops on the Recycle Bin

Over the bin's slot, or over the panel's padding past it, any drag with
paths highlights the bin instead of opening the gap. The padding counts
because the bin is the last item: a gap opening there would push the bin
under the pointer and back on every drag-over. The highlight is the hover
look without reflow: the icon is scaled by a fifth from its bottom edge,
by a transform on its bouncer, with the title pill above it.

The drop sends every path to `delete_to_recycle_bin`. Rust filters them with
the pure `rules::recycle`: plain absolute drive or UNC paths that exist, no
roots, each path once, nothing inside another dropped folder. It moves the
rest with one `IFileOperation`
(`FOF_ALLOWUNDO | FOFX_RECYCLEONDELETE | FOF_WANTNUKEWARNING`) on a
`stem-recycle` thread. The shell shows its own confirmation, progress and
error dialogs, which also warn before anything would be deleted
permanently. `IFileOperation` is used rather than
`SHFileOperationW` because it returns `HRESULT` errors instead of `DE_*`
codes, supports long paths, and takes shell items rather than a hand-built
double-NUL list. A cancel, a skipped file or an error fails the command,
and the UI shows its toast. After the call the service queries the bin
once on a short-lived thread, so the icon shows "full" at once rather than
at the next 5 s poll.
