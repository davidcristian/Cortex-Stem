/**
 * `MockBackend`, the last of its layers: the shell and app commands, event
 * subscriptions, native menus and the user's `dock.css`. Each layer in
 * `backend/` extends the one before: the base (state, command order,
 * events), the platform, the scripting hooks, the item commands and the
 * page's reports.
 */

import type {
  Backend,
  BackendEvent,
  EventHandler,
  FileDragHandler,
  Unsubscribe,
} from '$lib/ipc/backend';
import type { AppFolder } from '$lib/ipc/generated/AppFolder';
import type { AppInfo } from '$lib/ipc/generated/AppInfo';
import type { Settings } from '$lib/ipc/generated/Settings';
import type { ShellAction } from '$lib/ipc/generated/ShellAction';
import type { UserCss } from '$lib/ipc/generated/UserCss';

import { MOCK_USER_CSS_TEMPLATE, userCssFrom } from './app-files';
import { MockBackendReports } from './backend/reports';
import { nothingToRecycle, recyclePlan } from './recycle';
import { defaultSettings } from './settings';

export class MockBackend extends MockBackendReports implements Backend {
  shellAction(action: ShellAction): Promise<void> {
    return this.command(() => this.doShellAction(action));
  }

  /**
   * `DockService::delete_to_recycle_bin` over `MockPlatform`: an empty list
   * does nothing; otherwise the paths are recorded, every plain absolute
   * one counts as existing and goes into the bin, and the service's refresh
   * shows it (the icon turns full). Nothing usable rejects like Rust.
   */
  deleteToRecycleBin(paths: string[]): Promise<void> {
    return this.command(() => {
      if (paths.length === 0) {
        return;
      }
      this.record({ type: 'deleteToRecycleBin', paths: [...paths] });
      this.log(`delete_to_recycle_bin(${paths.join(', ')})`);
      const plan = recyclePlan(paths);
      if (plan.recycle.length === 0) {
        throw nothingToRecycle(plan);
      }
      this.pushRecycleBin({
        itemCount: this.engine.recycleBin.itemCount + plan.recycle.length,
        sizeBytes: this.engine.recycleBin.sizeBytes,
      });
    });
  }

  openSettings(): Promise<void> {
    return this.command(() => {
      this.record({ type: 'openSettings' });
      this.log('open_settings()');
    });
  }

  /** `DockService::reset_settings`: the defaults, pins and hidden apps included, validated and applied. */
  resetSettings(): Promise<Settings> {
    return this.command(() => {
      return structuredClone(this.applySettings(defaultSettings()));
    });
  }

  quit(): Promise<void> {
    return this.command(() => {
      this.record({ type: 'quit' });
      this.log('quit()');
    });
  }

  /** `get_app_info`: the demo facts, unless `setAppInfo` changed them. */
  getAppInfo(): Promise<AppInfo> {
    return this.command(() => ({ ...this.appInfo }));
  }

  /** `open_app_folder`: a browser opens no Explorer; the request is recorded. */
  openAppFolder(folder: AppFolder): Promise<void> {
    return this.command(() => {
      this.record({ type: 'openAppFolder', folder });
      this.log(`open_app_folder(${folder})`);
    });
  }

  on<E extends BackendEvent>(event: E, handler: EventHandler<E>): Unsubscribe {
    let set = this.listeners[event] as Set<EventHandler<E>> | undefined;
    if (!set) {
      set = new Set<EventHandler<E>>();
      (this.listeners as Record<string, unknown>)[event] = set;
    }
    set.add(handler);
    return () => {
      set.delete(handler);
    };
  }

  /** File drags arrive only through `simulateFileDrag`: a browser page gets no OLE drags. */
  onFileDrag(handler: FileDragHandler): Unsubscribe {
    this.fileDragHandlers.add(handler);
    return () => {
      this.fileDragHandlers.delete(handler);
    };
  }

  // --- Backend: native context menus. A browser has none; the mock
  // records the request (Rust would build and show the menu itself).

  /** `show_item_menu`: an unknown key rejects like Rust; separators have no menu. */
  showItemMenu(itemKey: string, x: number, y: number): Promise<void> {
    return this.command(() => {
      const item = this.engine.snapshot().items.find((i) => i.key === itemKey);
      if (!item) {
        throw `unknown dock item: ${itemKey}`;
      }
      if (item.kind === 'separator') {
        return;
      }
      this.record({ type: 'showItemMenu', itemKey, x, y });
      this.log(`show_item_menu(${itemKey}, ${x}, ${y})`);
    });
  }

  showDockMenu(x: number, y: number): Promise<void> {
    return this.command(() => {
      this.record({ type: 'showDockMenu', x, y });
      this.log(`show_dock_menu(${x}, ${y})`);
    });
  }

  /**
   * `release_dock_focus`: Rust would bring back the window that was
   * in front when the focus shortcut was pressed; a browser has no
   * foreground to give back, so the call is recorded and logged only.
   */
  releaseDockFocus(): Promise<void> {
    return this.command(() => {
      this.record({ type: 'releaseDockFocus' });
      this.shortcutFocus = false;
      this.log('release_dock_focus');
    });
  }

  // --- Backend: the user's dock.css (docs/ipc/themes.md)

  /** `get_user_css`: the last read (at construction from `MockOptions.userCss`, or the last reload). */
  getUserCss(): Promise<UserCss> {
    return this.command(() => ({ ...this.userCssRead }));
  }

  /**
   * `reload_user_css`: read the file again (`readUserCss`), keep the
   * result, send it as `dock://user-css` (to this page: the mock serves one
   * page) and return it.
   */
  reloadUserCss(): Promise<UserCss> {
    return this.command(() => {
      this.record({ type: 'reloadUserCss' });
      this.userCssRead = userCssFrom(this.userCssFile);
      const { css, problem } = this.userCssRead;
      this.log(
        `reload_user_css(): ${css === null ? `no CSS (${problem})` : `${css.length} chars`}`,
      );
      this.emit('dock://user-css', { ...this.userCssRead });
      return { ...this.userCssRead };
    });
  }

  /**
   * `open_user_css`: a browser opens no editor; the file is created from
   * the (stand-in) template if there is none, and the request is recorded
   * with whether it was. Nothing is read.
   */
  openUserCss(): Promise<void> {
    return this.command(() => {
      const created = this.userCssFile === null;
      if (created) {
        this.userCssFile = MOCK_USER_CSS_TEMPLATE;
      }
      this.record({ type: 'openUserCss', created });
      this.log(`open_user_css()${created ? ': wrote the template' : ''}`);
    });
  }
}
