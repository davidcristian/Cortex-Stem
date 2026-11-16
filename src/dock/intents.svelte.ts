import { isBrowserShortcut } from '$lib/dock/keyboard';
import { WheelNotches, wheelSends } from '$lib/dock/wheel';
import type { DockItem as DockItemModel } from '$lib/ipc/generated/DockItem';
import { describeError, Toasts } from '$lib/stores/toasts.svelte';

import type { DockController } from './controller.svelte';

/** What the user asks the dock for: clicks, the wheel, menus and shortcuts, and the notices when one fails. */
export class Intents {
  private readonly dock: DockController;

  // --- Intents and failure notices ---------------------------------------------
  readonly toasts = new Toasts();

  /**
   * Failed commands are logged and, when the user asked for something, shown
   * as a toast (docs/architecture/error-handling.md). Housekeeping calls
   * (hover, sizing) only log: a toast would be noise the user cannot act on.
   */
  readonly report =
    (what: string, toast = true) =>
    (err: unknown): void => {
      console.warn(`${what} failed:`, err);
      if (toast) {
        this.toasts.push(`${what} failed: ${describeError(err)}`);
      }
    };

  // The wheel over an item (docs/ipc/scrolling-over-an-item.md):
  // one listener on the panel. `wheel.ts` adds the vertical travel up per
  // item and says when a notch is complete (100 px, or one event of 30 px
  // or more, whatever Windows' lines to scroll at a time); each one is a
  // `scroll_item`, and Rust decides what it does (cycle the item's windows,
  // at most once per 150 ms; step the volume over the volume item, every
  // notch). Nothing is counted or sent while the setting is `none`,
  // except over the volume item (`wheelSends`), nor for a pinch or Ctrl and
  // the wheel (`ctrlKey`), and the page's default handling is only
  // prevented for an event that sends something.
  readonly wheelNotches = new WheelNotches();

  constructor(dock: DockController) {
    this.dock = dock;
  }

  readonly onprimary = (item: DockItemModel): void => {
    if (this.dock.reorder.drag?.started) {
      // The release that ends a drag is not a click.
      return;
    }
    void this.dock.backend.primaryClick(item.key).catch(this.report(item.title));
  };

  readonly onpanelwheel = (e: WheelEvent): void => {
    const slot =
      e.target instanceof Element ? e.target.closest<HTMLElement>('.slot[data-key]') : null;
    const key =
      slot !== null && this.dock.panelEl?.contains(slot) ? (slot.dataset.key ?? null) : null;
    const kind =
      key === null ? null : (this.dock.view.items.find((i) => i.key === key)?.kind ?? null);
    if (
      key !== null &&
      (!wheelSends(this.dock.view.scrollAction, kind) || this.dock.reorder.drag?.started)
    ) {
      this.wheelNotches.reset();
      return;
    }
    const notches = this.wheelNotches.add({
      key,
      deltaX: e.deltaX,
      deltaY: e.deltaY,
      deltaMode: e.deltaMode,
      ctrlKey: e.ctrlKey,
      pageHeight: window.innerHeight,
    });
    if (key === null || notches.length === 0) {
      return;
    }
    // Chromium lets only the first event of a wheel gesture block: when
    // that one sent nothing (a touchpad's first small step), the rest come
    // uncancelable, and trying would only log an intervention.
    if (e.cancelable) {
      e.preventDefault();
    }
    const title = this.dock.view.items.find((i) => i.key === key)?.title ?? key;
    for (const direction of notches) {
      void this.dock.backend.scrollItem(key, direction).catch(this.report(title));
    }
  };

  readonly onmiddle = (item: DockItemModel): void => {
    if (item.appId !== null) {
      void this.dock.backend.launchApp(item.appId).catch(this.report(`Launching ${item.title}`));
    }
  };

  // Right-click: the menus are native, built and shown by Rust; the
  // UI only reports which item and where (CSS px of this window's viewport).
  // The item's own `contextmenu` handler calls `oncontext` and the event
  // bubbles on to the panel, where only the panel itself (padding between
  // the slots) opens the dock menu. Separators get nothing.
  readonly oncontext = (item: DockItemModel, x: number, y: number): void => {
    if (this.dock.reorder.drag?.started) {
      return;
    }
    this.dock.autoHide.menuOpened();
    void this.dock.backend
      .showItemMenu(item.key, x, y)
      .catch(this.report('Menu'))
      .finally(this.dock.autoHide.menuClosed);
  };

  readonly onpanelcontextmenu = (e: MouseEvent): void => {
    e.preventDefault();
    if (e.target === this.dock.panelEl && !this.dock.reorder.drag?.started) {
      this.dock.autoHide.menuOpened();
      void this.dock.backend
        .showDockMenu(e.clientX, e.clientY)
        .catch(this.report('Menu'))
        .finally(this.dock.autoHide.menuClosed);
    }
  };

  // WebView2's own context menu (Back, Forward, Reload, Inspect…) must never
  // appear. The transparent stage outside the panel takes clicks while an
  // icon is being dragged and whenever click-through is off, and a
  // right-click there reached the browser's menu. Item and panel
  // menus are native and already handled; their events bubble up here after.
  readonly onwindowcontextmenu = (e: MouseEvent): void => {
    e.preventDefault();
  };

  openSettings(): void {
    void this.dock.backend.openSettings().catch(this.report('Opening Settings'));
  }

  /**
   * Let go of the keyboard: Escape, and a second press of the focus
   * shortcut (`dock://release-focus`). The focused item, if there is
   * one, loses the focus: its pill goes, and with auto-hide on the
   * countdown starts. Rust is asked to give the keyboard focus back
   * (`release_dock_focus`): it brings back the window that was in front
   * when the focus shortcut was pressed, and does nothing when the shortcut
   * did not bring the keyboard here. Housekeeping: a failure only logs.
   * Whether an item lost the focus.
   */
  letGo(): boolean {
    const active = document.activeElement;
    const blurred = active instanceof HTMLElement && this.dock.panelEl?.contains(active) === true;
    if (blurred) {
      active.blur();
    }
    void this.dock.backend.releaseDockFocus().catch(this.report('release_dock_focus', false));
    return blurred;
  }

  /** Keyboard shortcuts while the dock window has focus. */
  readonly onwindowkeydown = (e: KeyboardEvent): void => {
    if (e.key === 'Escape' && this.dock.reorder.drag !== null) {
      e.preventDefault();
      this.dock.reorder.cancelDrag(true);
      return;
    }
    // Otherwise Escape lets go of the keyboard.
    if (e.key === 'Escape') {
      if (this.letGo()) {
        e.preventDefault();
      }
      return;
    }
    if (e.key === ',' && e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey) {
      e.preventDefault();
      this.openSettings();
      return;
    }
    // WebView2's own shortcuts (reload, find, print, save, back): nothing
    // a dock does, and a reload would reset the page.
    if (isBrowserShortcut(e)) {
      e.preventDefault();
    }
  };
}
