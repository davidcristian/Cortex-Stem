# Keyboard

This file describes how the dock is used with the keyboard.

Inside the page, the panel is a `toolbar` labelled "Stem", whose
`aria-orientation` follows the dock's axis. It has one tab stop: a roving
tabindex on the item the focus was on last. The arrows of the dock's main
axis move the focus (Left and Right on a horizontal dock, Up and Down on a
vertical one; the other pair does nothing), and Home and End go to the
first and last item. The focus skips separators and stops at the ends
(`src/lib/dock/keyboard.ts`, pure and unit-tested). When an item with the
focus leaves the dock, the focus moves to its neighbour.

Enter and Space are `primary_click`. Shift+F10 and the menu key are
`show_item_menu` at the slot's horizontal centre and top edge. The key down and the menu
key's release are both handled (`preventDefault`), so Chromium's own
keyboard `contextmenu` never opens the menu a second time. Escape cancels a
drag; otherwise it releases the focus.

An item with keyboard focus (`:focus-visible`) shows its title pill (the
pointer's pill takes precedence) and the focus ring. It never magnifies,
and it holds [auto-hide](auto-hide.md) (`focus`, a hold that does not
reveal). Labels add "minimised" and "needs attention".

Nothing moves keyboard focus into the `WS_EX_NOACTIVATE` window, so the
keyboard alone does not get into the dock. Assumes that a screen reader can
still reach the items through UI Automation.
