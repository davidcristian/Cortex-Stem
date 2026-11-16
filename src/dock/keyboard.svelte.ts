import { tick, untrack } from 'svelte';

import { entryKey, nextFocusIndex, successorKey, tabStopKey } from '$lib/dock/keyboard';
import type { DockItem as DockItemModel } from '$lib/ipc/generated/DockItem';

import type { DockController } from './controller.svelte';

/**
 * The options of `HTMLElement.focus` with `focusVisible` (HTML; not in
 * TypeScript's DOM types yet): `true` asks the browser to show the focus
 * as keyboard focus whatever its heuristics say.
 */
type VisibleFocusOptions = NonNullable<Parameters<HTMLElement['focus']>[0]> & {
  focusVisible?: boolean;
};

/** The toolbar's keyboard focus: one tab stop, the arrows, the focus shortcut and letting go. */
export class DockKeyboard {
  /** Set in the constructor; the `$derived` fields read it only later. */
  private readonly dock!: DockController;

  // --- Keyboard -----------------------------------------------------------------
  // The toolbar is one tab stop with a roving tabindex: the item the focus
  // was on last has tabindex 0, every other one -1 (`tabStopKey`). Arrows,
  // Home and End move the focus (`nextFocusIndex`, src/lib/dock/keyboard.ts,
  // pure); Enter, Space and the menu keys are the item's own (DockItem). An
  // item with keyboard focus (`:focus-visible`) shows its title pill as
  // hover does, without magnifying, and holds auto-hide (`focus`). Left and
  // middle clicks never focus an item (DockItem prevents their mouse down);
  // a right-click may, but that focus is not `:focus-visible`, so it moves
  // the tab stop and nothing else.
  /** The item the focus was on last: the toolbar's tab stop while it is still there. */
  focusedKey = $state<string | null>(null);
  /** The item that has keyboard focus (`:focus-visible`), or `null`. */
  keyboardKey = $state<string | null>(null);
  tabStop = $derived(tabStopKey(this.dock.view.items, this.focusedKey));
  /**
   * The focused item's pill; the pointer's, a lifted icon and a file drag
   * over the bin come first (one pill at a time).
   */
  focusTooltipKey = $derived(
    this.dock.view.tooltipsEnabled &&
      this.dock.tooltip.tooltipKey === null &&
      this.dock.reorder.liftedKey === null &&
      this.dock.files.dropTargetKey === null
      ? this.keyboardKey
      : null,
  );

  renderedItems: readonly DockItemModel[] = [];

  constructor(dock: DockController) {
    this.dock = dock;
    // The item with the focus left the dock (its app's last window closed):
    // the focus moves on to its neighbour (`successorKey`), so the keyboard
    // stays in the toolbar, and the tab stop goes with it. After the slots
    // effect of `Magnifier`, which has collected the new slots by then.
    $effect(() => {
      const now = this.dock.view.items;
      untrack(() => {
        const before = this.renderedItems;
        this.renderedItems = now;
        const key = this.focusedKey;
        if (key === null || now.some((i) => i.key === key)) {
          return;
        }
        const active = document.activeElement;
        const hadFocus =
          this.keyboardKey === key || (active instanceof HTMLElement && active.dataset.key === key);
        const next = successorKey(before, now, key);
        this.focusedKey = next;
        if (!hadFocus) {
          return;
        }
        const el =
          next === null ? undefined : this.dock.hover.slots.find((s) => s.dataset.key === next);
        if (el !== undefined) {
          el.focus();
          return;
        }
        if (active instanceof HTMLElement && this.dock.panelEl?.contains(active)) {
          active.blur();
        }
        this.releaseKeyboardFocus();
      });
    });
  }

  /** `target` is an item's slot in this panel (not a separator, not the panel itself). */
  itemSlotOf(target: EventTarget | null): HTMLElement | null {
    if (
      !(target instanceof HTMLElement) ||
      target.dataset.key === undefined ||
      target.dataset.kind === 'separator' ||
      !this.dock.panelEl?.contains(target)
    ) {
      return null;
    }
    return target;
  }

  /**
   * Move the focus to the item `key`. `visible` (the focus shortcut):
   * show it as keyboard focus even when the page's last input was
   * the pointer. Chromium decides `:focus-visible` for a script's `focus()`
   * by the page's last input, and the shortcut's keys never reach the page
   * (Windows takes them), so after any click on the dock the item would get
   * the focus without the ring, the pill or the auto-hide hold.
   */
  focusItem(key: string, visible = false): void {
    this.focusedKey = key;
    const options: VisibleFocusOptions = visible ? { focusVisible: true } : {};
    this.dock.hover.slots.find((el) => el.dataset.key === key)?.focus(options);
  }

  readonly onpanelkeydown = (e: KeyboardEvent): void => {
    if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey || this.dock.reorder.drag?.started) {
      return;
    }
    const key = this.itemSlotOf(e.target)?.dataset.key;
    const current = key === undefined ? -1 : this.dock.view.items.findIndex((i) => i.key === key);
    const next = nextFocusIndex(this.dock.view.items, current, e.key, this.dock.view.axis);
    if (next === null) {
      return;
    }
    e.preventDefault();
    this.focusItem(this.dock.view.items[next]!.key);
  };

  // The focus shortcut (docs/ipc/keyboard-entry.md): Rust has made
  // this dock window the foreground window. The keyboard lands on the
  // active app's item, else the first (`entryKey`), the way Tab would
  // bring it there: the ring, the title pill and the `focus` hold follow
  // from `focusin`. A panel still slid out (a press that crossed the slide,
  // which Rust did not see yet) comes back first: the hold keeps a dock up
  // but never reveals one. Either way the focus waits for the DOM to catch
  // up, since a panel the page has just brought back (here, or for the
  // `dock://show` before this event) is still hidden until then, and a
  // hidden panel takes no focus.
  async focusEntry(): Promise<void> {
    if (this.dock.autoHide.autoHidden) {
      this.dock.autoHide.send({ type: 'show' });
    }
    await tick();
    const key = entryKey(this.dock.view.items);
    if (key !== null) {
      this.focusItem(key, true);
    }
  }

  readonly onpanelfocusin = (e: FocusEvent): void => {
    const slot = this.itemSlotOf(e.target);
    if (slot === null) {
      return;
    }
    this.focusedKey = slot.dataset.key ?? null;
    this.keyboardKey = slot.matches(':focus-visible') ? this.focusedKey : null;
    this.dock.autoHide.send({ type: 'hold', hold: 'focus', active: this.keyboardKey !== null });
  };

  readonly onpanelfocusout = (e: FocusEvent): void => {
    if (this.itemSlotOf(e.relatedTarget) !== null) {
      // On to another item: its `focusin` follows.
      return;
    }
    const key = this.itemSlotOf(e.target)?.dataset.key;
    if (key !== undefined && !this.dock.view.items.some((i) => i.key === key)) {
      // The item is being removed: the constructor's effect moves the focus on.
      return;
    }
    this.releaseKeyboardFocus();
  };

  releaseKeyboardFocus(): void {
    this.keyboardKey = null;
    this.dock.autoHide.send({ type: 'hold', hold: 'focus', active: false });
  }
}
