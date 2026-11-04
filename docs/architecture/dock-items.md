# Dock items

This file describes the dock's fixed items and how an item that needs attention is shown.

## Fixed items

Start, Task View, Show Desktop and the Recycle Bin are ordinary
`DockItem`s of their own kinds, drawn from the built-in `shell:` vector art
(`src/lib/icons/ShellIcon.svelte`). A click is `primary_click` (the engine
maps the key to its `ShellAction`), and a right-click is `show_item_menu`
(Rust builds the menu), as for any item. The `items.*` settings add and remove
them live.

## Attention

`DockItem.needsAttention` marks the slot `data-attention`. The engine sets
it on `HSHELL_FLASH` and clears it when the window becomes the foreground.
With motion allowed the icon wiggles: a 2 s CSS `transform` keyframe loop
on the `.wiggler`, an element of its own, so it composes with the bounce
and the magnification. Under reduced motion it does not wiggle. Either way
its running dots are replaced by one short pill in `--attention` (Windows
orange), shown even with the running indicators off, so the state is
visible without motion and whatever the accent colour is. Nothing runs
while no item needs attention: no animation and no timer.
