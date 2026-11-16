import type { DockEdge } from '$lib/dock/edge';
import type { ClockFormat } from '$lib/ipc/generated/ClockFormat';
import type { DockItem } from '$lib/ipc/generated/DockItem';
import type { PowerStatus } from '$lib/ipc/generated/PowerStatus';
import type { VolumeStatus } from '$lib/ipc/generated/VolumeStatus';

/** The props of DockItem.svelte. */
export interface DockItemProps {
  item: DockItem;
  /**
   * Resolved app icon at exactly its rest size in device pixels: a data
   * URL, `null` when there is none, `undefined` while loading.
   */
  iconUrl: string | null | undefined;
  /**
   * The ladder-size bitmap shown while the box is magnified:
   * `null` when the rest bitmap serves both (the sizes coincide, or
   * magnification is off), `undefined` while loading. Both images stay in
   * the DOM; the dock's magnification controller switches them by setting
   * `data-magnified` on the slot, so a switch is a style change, not a load.
   */
  magnifiedIconUrl: string | null | undefined;
  /** `behavior.runningIndicators`. */
  showDots: boolean;
  /** Show the title pill above the icon now. */
  tooltip: boolean;
  /** Changes on every `dock://item-bounce` for this item; 0 = never. */
  bounce: number;
  /**
   * False under `prefers-reduced-motion` (no bounce, no wiggle, no
   * enter/leave). The attention wiggle follows only this: the
   * launch bounce has its own setting (`appearance.bounceOnLaunch`).
   */
  animate: boolean;
  /**
   * Play the enter animation if this item appears now. Off until the dock
   * has rendered its first snapshot, so startup does not cascade icons in.
   */
  enter: boolean;
  /** Being dragged to a new place: raised, shadowed, no dots. */
  lifted: boolean;
  /**
   * The screen edge the dock sits on: the launch bounce moves away
   * from it, the attention wiggle along the dock, and the keyboard's menu
   * opens on the item's side away from it. Default `bottom`.
   */
  edge?: DockEdge;
  /**
   * The toolbar's one tab stop (a roving tabindex): `tabindex` 0
   * for this item, -1 for the others. The dock moves it with the focus.
   */
  tabStop: boolean;
  /**
   * A file drag points at this item and a drop would land on it
   * (the Recycle Bin): the icon grows a little, as under the pointer. A
   * transform only, so nothing reflows while the drag moves.
   */
  dropTarget: boolean;
  /**
   * `DockState.power`: the battery item draws its face from it.
   * `null` (the default) before the page has a state.
   */
  power?: PowerStatus | null;
  /**
   * `DockState.volume`: the volume item draws its face from it;
   * `null` (the default) without an output device or before the page has
   * a state.
   */
  volume?: VolumeStatus | null;
  /**
   * `DockState.clockFormat`: the clock item formats its face, its
   * title pill and its accessible name in it. `null` (the default) before
   * the page has a state: the page's own locale.
   */
  clockFormat?: ClockFormat | null;
  onprimary: (item: DockItem) => void;
  onmiddle: (item: DockItem) => void;
  /**
   * Open the item's native menu at (`x`, `y`), CSS px of the viewport: the
   * pointer for a right-click; for Shift+F10 and the context menu key,
   * the middle of the item's side away from the edge (`menuAnchor`:
   * its horizontal centre and top edge on `bottom`).
   */
  oncontext: (item: DockItem, x: number, y: number) => void;
  onhover: (item: DockItem | null) => void;
  /** A pointer went down on the item (the dock may start a drag from it). */
  onpress: (item: DockItem, e: PointerEvent) => void;
  /** An enter or leave animation of this slot has finished (the panel is at rest again). */
  onsettled: () => void;
}
