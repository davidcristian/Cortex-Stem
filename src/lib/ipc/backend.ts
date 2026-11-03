/**
 * The frontend's only door to the Rust side (docs/ipc/, contract v1).
 *
 * One method per command and one `on` subscription per event. Components
 * never call `invoke`/`listen` themselves (AGENTS.md §4); they talk to a
 * `Backend`, which is `TauriBackend` inside the app and `MockBackend` in a
 * browser or under Playwright.
 */

import type { AppId } from '$lib/ipc/generated/AppId';
import type { DockState } from '$lib/ipc/generated/DockState';
import type { IconKey } from '$lib/ipc/generated/IconKey';
import type { LaunchTarget } from '$lib/ipc/generated/LaunchTarget';
import type { MonitorInfo } from '$lib/ipc/generated/MonitorInfo';
import type { Settings } from '$lib/ipc/generated/Settings';
import type { ShellAction } from '$lib/ipc/generated/ShellAction';
import type { ThemeInfo } from '$lib/ipc/generated/ThemeInfo';
import type { WindowAction } from '$lib/ipc/generated/WindowAction';
import type { WindowId } from '$lib/ipc/generated/WindowId';

/** Payload of `dock://item-bounce`. */
export interface BouncePayload {
  itemKey: string;
}

export type VisibilityReason = 'autoHide' | 'fullscreen' | 'user';

/** Payload of `dock://visibility` (M2). */
export interface VisibilityPayload {
  visible: boolean;
  reason: VisibilityReason;
}

/** Event name → payload, exactly as docs/ipc/events.md lists them. */
export interface BackendEvents {
  'dock://state': DockState;
  'dock://settings': Settings;
  'dock://theme': ThemeInfo;
  'dock://monitors': MonitorInfo[];
  'dock://item-bounce': BouncePayload;
  'dock://visibility': VisibilityPayload;
}

export type BackendEvent = keyof BackendEvents;

export type EventHandler<E extends BackendEvent> = (payload: BackendEvents[E]) => void;

/** Returned by `Backend.on`; calling it stops the subscription. */
export type Unsubscribe = () => void;

/**
 * Commands and events of docs/ipc/ v1. Method names are the command names
 * in camelCase; argument order follows the command's argument list.
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
  /** CSS pixels; Rust resizes and re-centres the calling window. */
  setDockContentSize(width: number, height: number): Promise<void>;
  quit(): Promise<void>;

  on<E extends BackendEvent>(event: E, handler: EventHandler<E>): Unsubscribe;

  // Planned for M2 (docs/ipc/commands.md). Optional until Rust implements them; the
  // UI checks for presence before calling.
  showItemMenu?(itemKey: string, x: number, y: number): Promise<void>;
  showDockMenu?(x: number, y: number): Promise<void>;
  openSettings?(): Promise<void>;
  setDockHover?(hovering: boolean): Promise<void>;
  shellAction?(action: ShellAction): Promise<void>;
}

/** The `shell:` icon namespace is rendered from built-in vector art. */
export function isShellIconKey(key: IconKey): boolean {
  return key.startsWith('shell:');
}
