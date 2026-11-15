/**
 * `Backend` over Tauri's `invoke` / `listen`. Command names and argument keys
 * are exactly those of docs/ipc/commands.md (`src-tauri/src/commands.rs`):
 * Tauri maps camelCase argument keys to the Rust parameter names.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWebview, type DragDropEvent } from '@tauri-apps/api/webview';

import type { AppFolder } from '$lib/ipc/generated/AppFolder';
import type { AppId } from '$lib/ipc/generated/AppId';
import type { AppInfo } from '$lib/ipc/generated/AppInfo';
import type { DockPosition } from '$lib/ipc/generated/DockPosition';
import type { DockState } from '$lib/ipc/generated/DockState';
import type { IconKey } from '$lib/ipc/generated/IconKey';
import type { LaunchTarget } from '$lib/ipc/generated/LaunchTarget';
import type { MonitorInfo } from '$lib/ipc/generated/MonitorInfo';
import type { Rect } from '$lib/ipc/generated/Rect';
import type { Settings } from '$lib/ipc/generated/Settings';
import type { ShellAction } from '$lib/ipc/generated/ShellAction';
import type { ThemeInfo } from '$lib/ipc/generated/ThemeInfo';
import type { UserCss } from '$lib/ipc/generated/UserCss';
import type { WindowAction } from '$lib/ipc/generated/WindowAction';
import type { WindowId } from '$lib/ipc/generated/WindowId';

import { integerRect, type Viewport } from '$lib/dock/geometry';

import type {
  Backend,
  BackendEvent,
  BackendEvents,
  EventHandler,
  FileDragEvent,
  FileDragHandler,
  Unsubscribe,
} from './backend';

/**
 * Tauri's drag-drop payload in the page's terms: positions arrive in
 * physical pixels relative to the webview's top-left corner and become CSS
 * px of the viewport by dividing by the device pixel ratio, here and only
 * here (docs/ipc/file-drags.md).
 */
export function fileDragFromTauri(payload: DragDropEvent, devicePixelRatio: number): FileDragEvent {
  const scale = Number.isFinite(devicePixelRatio) && devicePixelRatio > 0 ? devicePixelRatio : 1;
  const css = (p: { x: number; y: number }): { x: number; y: number } => ({
    x: p.x / scale,
    y: p.y / scale,
  });
  switch (payload.type) {
    case 'enter':
      return { type: 'enter', paths: [...payload.paths], position: css(payload.position) };
    case 'over':
      return { type: 'over', position: css(payload.position) };
    case 'drop':
      return { type: 'drop', paths: [...payload.paths], position: css(payload.position) };
    case 'leave':
      return { type: 'leave' };
  }
}

/**
 * Subscribe through an async registration and return a synchronous
 * `Unsubscribe` that works whether or not the registration has completed.
 * `deliver` is only called while subscribed.
 */
function subscribe(
  what: string,
  register: (deliver: (fn: () => void) => void) => Promise<UnlistenFn>,
): Unsubscribe {
  let disposed = false;
  let unlisten: UnlistenFn | undefined;
  const deliver = (fn: () => void): void => {
    if (!disposed) {
      fn();
    }
  };
  // Registered now, in the caller's tick: the startup sequence subscribes
  // before it fetches (docs/ipc/events.md). A synchronous throw (no
  // webview metadata) is reported like a rejection instead of escaping.
  let registration: Promise<UnlistenFn>;
  try {
    registration = register(deliver);
  } catch (err) {
    registration = Promise.reject(err);
  }
  void registration.then(
    (fn) => {
      if (disposed) {
        fn();
      } else {
        unlisten = fn;
      }
    },
    (err: unknown) => console.error(`${what} failed:`, err),
  );
  return () => {
    disposed = true;
    unlisten?.();
    unlisten = undefined;
  };
}

export class TauriBackend implements Backend {
  getDockState(): Promise<DockState> {
    return invoke<DockState>('get_dock_state');
  }

  getSettings(): Promise<Settings> {
    return invoke<Settings>('get_settings');
  }

  updateSettings(settings: Settings): Promise<Settings> {
    return invoke<Settings>('update_settings', { settings });
  }

  getTheme(): Promise<ThemeInfo> {
    return invoke<ThemeInfo>('get_theme');
  }

  getMonitors(): Promise<MonitorInfo[]> {
    return invoke<MonitorInfo[]>('get_monitors');
  }

  primaryClick(itemKey: string): Promise<void> {
    return invoke<void>('primary_click', { itemKey });
  }

  windowAction(windowId: WindowId, action: WindowAction): Promise<void> {
    return invoke<void>('window_action', { windowId, action });
  }

  launchApp(appId: AppId): Promise<void> {
    return invoke<void>('launch_app', { appId });
  }

  pinApp(appId: AppId): Promise<void> {
    return invoke<void>('pin_app', { appId });
  }

  pinTarget(target: LaunchTarget): Promise<AppId> {
    return invoke<AppId>('pin_target', { target });
  }

  unpinApp(appId: AppId): Promise<void> {
    return invoke<void>('unpin_app', { appId });
  }

  reorderPinned(order: AppId[]): Promise<void> {
    return invoke<void>('reorder_pinned', { order });
  }

  getIcon(iconKey: IconKey, size: number): Promise<string | null> {
    // `size` is a `u32` on the Rust side.
    return invoke<string | null>('get_icon', { iconKey, size: Math.max(1, Math.round(size)) });
  }

  setDockContentSize(width: number, height: number, edge: DockPosition): Promise<void> {
    return invoke<void>('set_dock_content_size', { width, height, edge });
  }

  setDockHitRegion(panel: Rect, envelope: Rect, edge: DockPosition): Promise<void> {
    // `Rect` fields are `i32` on the Rust side.
    return invoke<void>('set_dock_hit_region', {
      panel: integerRect(panel),
      envelope: integerRect(envelope),
      edge,
    });
  }

  setDockPanelRect(rect: Rect, edge: DockPosition, viewport: Viewport): Promise<void> {
    // The viewport is whole CSS px (`innerWidth` × `innerHeight` already are);
    // Rust rejects a non-positive or non-finite one.
    return invoke<void>('set_dock_panel_rect', {
      rect: integerRect(rect),
      edge,
      viewport: { width: Math.round(viewport.width), height: Math.round(viewport.height) },
    });
  }

  setDockAutoHidden(hidden: boolean): Promise<void> {
    return invoke<void>('set_dock_auto_hidden', { hidden });
  }

  shellAction(action: ShellAction): Promise<void> {
    return invoke<void>('shell_action', { action });
  }

  deleteToRecycleBin(paths: string[]): Promise<void> {
    return invoke<void>('delete_to_recycle_bin', { paths });
  }

  openSettings(): Promise<void> {
    return invoke<void>('open_settings');
  }

  resetSettings(): Promise<Settings> {
    return invoke<Settings>('reset_settings');
  }

  quit(): Promise<void> {
    return invoke<void>('quit');
  }

  getAppInfo(): Promise<AppInfo> {
    return invoke<AppInfo>('get_app_info');
  }

  openAppFolder(folder: AppFolder): Promise<void> {
    return invoke<void>('open_app_folder', { folder });
  }

  showItemMenu(itemKey: string, x: number, y: number): Promise<void> {
    return invoke<void>('show_item_menu', { itemKey, x, y });
  }

  showDockMenu(x: number, y: number): Promise<void> {
    return invoke<void>('show_dock_menu', { x, y });
  }

  releaseDockFocus(): Promise<void> {
    return invoke<void>('release_dock_focus');
  }

  getUserCss(): Promise<UserCss> {
    return invoke<UserCss>('get_user_css');
  }

  reloadUserCss(): Promise<UserCss> {
    return invoke<UserCss>('reload_user_css');
  }

  openUserCss(): Promise<void> {
    return invoke<void>('open_user_css');
  }

  on<E extends BackendEvent>(event: E, handler: EventHandler<E>): Unsubscribe {
    return subscribe(`listen(${event})`, (deliver) =>
      listen<BackendEvents[E]>(event, (e) => deliver(() => handler(e.payload))),
    );
  }

  /**
   * Tauri's own `tauri://drag-*` events for this webview (its drag-drop
   * handler is on for the dock window; `core:default` grants the listen).
   */
  onFileDrag(handler: FileDragHandler): Unsubscribe {
    return subscribe('onDragDropEvent', (deliver) =>
      getCurrentWebview().onDragDropEvent((e) =>
        deliver(() => handler(fileDragFromTauri(e.payload, window.devicePixelRatio))),
      ),
    );
  }
}
