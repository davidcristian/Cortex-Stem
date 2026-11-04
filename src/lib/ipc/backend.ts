/**
 * The frontend's only door to the Rust side (docs/ipc/, contract v1.7).
 *
 * One method per command and one `on` subscription per event (plus
 * `onFileDrag` for Tauri's own file drag events). Components
 * never call `invoke`/`listen` themselves (AGENTS.md §4); they talk to a
 * `Backend`, which is `TauriBackend` inside the app and `MockBackend` in a
 * browser or under Playwright.
 */

import type { AppId } from '$lib/ipc/generated/AppId';
import type { DockState } from '$lib/ipc/generated/DockState';
import type { IconKey } from '$lib/ipc/generated/IconKey';
import type { LaunchTarget } from '$lib/ipc/generated/LaunchTarget';
import type { MonitorInfo } from '$lib/ipc/generated/MonitorInfo';
import type { Rect } from '$lib/ipc/generated/Rect';
import type { Settings } from '$lib/ipc/generated/Settings';
import type { ShellAction } from '$lib/ipc/generated/ShellAction';
import type { ThemeInfo } from '$lib/ipc/generated/ThemeInfo';
import type { WindowAction } from '$lib/ipc/generated/WindowAction';
import type { WindowId } from '$lib/ipc/generated/WindowId';

/** Payload of `dock://item-bounce`. */
export interface BouncePayload {
  itemKey: string;
}

/** Event name → payload, exactly as docs/ipc/events.md lists them. */
export interface BackendEvents {
  'dock://state': DockState;
  'dock://settings': Settings;
  'dock://theme': ThemeInfo;
  'dock://monitors': MonitorInfo[];
  'dock://item-bounce': BouncePayload;
}

export type BackendEvent = keyof BackendEvents;

export type EventHandler<E extends BackendEvent> = (payload: BackendEvents[E]) => void;

/** Returned by `Backend.on`; calling it stops the subscription. */
export type Unsubscribe = () => void;

/** A point in CSS px relative to the dock window's viewport (like `MouseEvent.clientX/Y`). */
export interface CssPoint {
  x: number;
  y: number;
}

/**
 * A file drag from the shell over the calling window
 * (docs/ipc/file-drags.md): Tauri's webview drag-drop events with the
 * position in CSS px of the viewport. `enter` and `drop` carry the
 * dragged paths, as the shell gives them; `over` only moves; `leave`
 * means the drag left the window or was cancelled. An OLE drag like this
 * produces no pointer or HTML5 drag events in the page.
 */
export type FileDragEvent =
  | { type: 'enter'; paths: string[]; position: CssPoint }
  | { type: 'over'; position: CssPoint }
  | { type: 'drop'; paths: string[]; position: CssPoint }
  | { type: 'leave' };

export type FileDragHandler = (event: FileDragEvent) => void;

/**
 * Commands and events of docs/ipc/ v1.7. Method names are the command
 * names in camelCase; argument order follows the command's argument list.
 *
 * Failed commands reject with a `string` (the Rust error message).
 */
export interface Backend {
  getDockState(): Promise<DockState>;
  getSettings(): Promise<Settings>;
  /** Validated and persisted; the returned copy is what Rust kept. */
  updateSettings(settings: Settings): Promise<Settings>;
  getTheme(): Promise<ThemeInfo>;
  getMonitors(): Promise<MonitorInfo[]>;
  /** The engine decides: launch / activate / minimise / cycle / shell action. */
  primaryClick(itemKey: string): Promise<void>;
  windowAction(windowId: WindowId, action: WindowAction): Promise<void>;
  /** Start a new instance; Rust emits `dock://item-bounce`. */
  launchApp(appId: AppId): Promise<void>;
  pinApp(appId: AppId): Promise<void>;
  /** Pin a dropped/chosen target; returns the canonical id it was pinned under. */
  pinTarget(target: LaunchTarget): Promise<AppId>;
  unpinApp(appId: AppId): Promise<void>;
  reorderPinned(order: AppId[]): Promise<void>;
  /** PNG `data:` URL, or `null` for `shell:*` keys and unknown keys. */
  getIcon(iconKey: IconKey, size: number): Promise<string | null>;
  /** CSS pixels; Rust sizes the calling window (docs/ipc/sizing-protocol.md). */
  setDockContentSize(width: number, height: number): Promise<void>;
  /**
   * Where the calling dock window accepts clicks (docs/ipc/hit-region.md):
   * integer CSS px relative to the viewport; `envelope` contains `panel`.
   * Outside `panel` the window is click-through until the pointer enters it;
   * it stays clickable until the pointer leaves `envelope`.
   */
  setDockHitRegion(panel: Rect, envelope: Rect): Promise<void>;
  /** Run a shell action directly (menus, Recycle Bin "Empty"). */
  shellAction(action: ShellAction): Promise<void>;
  /** Open the settings window, or focus it if it is already open. */
  openSettings(): Promise<void>;
  /** Restore and persist the defaults; Rust emits `dock://settings` and returns what it kept. */
  resetSettings(): Promise<Settings>;
  quit(): Promise<void>;
  /**
   * Native context menu of one item at `(x, y)`, CSS px relative to the
   * calling dock window's viewport. Rust builds the menu, shows it, runs the
   * chosen entry, and resolves once the menu has closed. Separators have no
   * menu (resolves without showing one); an unknown key rejects.
   */
  showItemMenu(itemKey: string, x: number, y: number): Promise<void>;
  /** The dock's own menu (panel background): settings and quit. */
  showDockMenu(x: number, y: number): Promise<void>;

  on<E extends BackendEvent>(event: E, handler: EventHandler<E>): Unsubscribe;

  /**
   * File drags over this window (docs/ipc/file-drags.md). `TauriBackend`
   * converts Tauri's physical pixels to CSS px; `MockBackend` delivers what
   * its `simulateFileDrag` hook is given.
   */
  onFileDrag(handler: FileDragHandler): Unsubscribe;

  // Planned for M2 (docs/ipc/commands.md). Optional until Rust
  // implements it; the UI checks for presence before calling.
  setDockHover?(hovering: boolean): Promise<void>;
}

/** The `shell:` icon namespace is rendered from built-in vector art. */
export function isShellIconKey(key: IconKey): boolean {
  return key.startsWith('shell:');
}
