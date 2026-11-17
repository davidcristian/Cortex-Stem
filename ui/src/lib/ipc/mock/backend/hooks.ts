/**
 * The scripting hooks of `MockBackend` that stand for the app layer and the
 * user: file drags, failing commands, the files on disk, a settings document,
 * and the tray, shortcut and window requests Rust sends a page.
 */

import type { FileDragEvent } from '$lib/ipc/backend';
import type { AppInfo } from '$lib/ipc/generated/AppInfo';
import type { DockPosition } from '$lib/ipc/generated/DockPosition';
import type { Rect } from '$lib/ipc/generated/Rect';
import type { Settings } from '$lib/ipc/generated/Settings';

import type { MockUserCssFile } from '../app-files';
import { MockBackendPlatform } from './platform';

export class MockBackendHooks extends MockBackendPlatform {
  /**
   * Deliver a file drag event, exactly what `TauriBackend.onFileDrag` would
   * hand over for an OLE drag from Explorer (positions in CSS px of the
   * viewport). Synchronous, like Tauri's events arriving between frames.
   */
  simulateFileDrag(event: FileDragEvent): void {
    this.log(`file drag ${event.type}${'paths' in event ? ` ${event.paths.join(', ')}` : ''}`);
    for (const handler of [...this.fileDragHandlers]) {
      handler(structuredClone(event));
    }
  }

  /** Make the next command reject with `message` (what a Rust error looks like across IPC). */
  failNextCommand(message: string): void {
    this.failNext = message;
  }

  /**
   * Change what `get_app_info` answers from now on: a run with file
   * logging off is `{ logPath: '' }`. The settings page asks once at start,
   * so set it before the page's store starts.
   */
  setAppInfo(info: Partial<AppInfo>): void {
    this.appInfo = { ...this.appInfo, ...info };
  }

  /**
   * Change the user's `dock.css` on "disk" (`null` deletes it; bytes
   * or `{ unreadable }` make a file Stem cannot use), as an editor
   * would: nothing is read until the next `reloadUserCss`.
   */
  writeUserCss(file: MockUserCssFile): void {
    this.userCssFile = file;
  }

  /** The mock's `dock.css` on "disk" (`null`: none), e.g. after `openUserCss` wrote the template. */
  userCssOnDisk(): MockUserCssFile {
    return this.userCssFile;
  }

  /**
   * Where the engine believes the panel of the dock window `label` rests
   * (screen px; this page's window by default), or `null` before its first
   * report, while it is hidden, or for a label never registered.
   */
  dockPanel(label: string = this.pageLabel): Rect | null {
    const rect = this.engine.panels.get(label)?.rect ?? null;
    return rect === null ? null : { ...rect };
  }

  /**
   * A whole settings document, the pins and hidden apps included, as the
   * settings file gives one at start (validated, `dock://settings` to
   * whoever listens): for tests of a list that no command made, such as a
   * pin the dock has not resolved yet. No command does this at runtime:
   * `updateSettings` keeps the lists (IPC v1.26).
   */
  loadSettings(settings: Settings): void {
    this.applySettings(settings);
  }

  /**
   * The settings window was asked to close: what Rust tells its
   * page before it destroys it (`dock://settings-closing`). Playwright's
   * way to close the settings page without losing what it holds back.
   */
  requestSettingsClose(): void {
    this.emit('dock://settings-closing', null);
  }

  /**
   * Put the dock on `position` the way a settings change does
   * (validated, `dock://settings` to whoever listens): the browser demo's
   * and Playwright's way to show each edge. The dock page calls it at load
   * for `?position=` (`demo.ts`), before it has asked for anything.
   */
  setPosition(position: DockPosition): void {
    const settings = structuredClone(this.engine.settings);
    settings.position = position;
    this.applySettings(settings);
  }

  /**
   * The tray or the toggle shortcut was pressed while the dock window was
   * visible with its panel slid out by auto-hide: what
   * `windows::toggle_dock` emits to that window instead of hiding it. One
   * `dock://show` with a `null` payload, whatever the UI last reported.
   */
  requestShow(): void {
    this.log('dock reveal requested');
    this.emit('dock://show', null);
  }

  /**
   * The focus shortcut was pressed (docs/ipc/keyboard-entry.md):
   * what `windows::on_focus_shortcut` sends this page's dock window. While
   * the dock has the keyboard from an earlier press (`shortcutFocus`), one
   * `dock://release-focus`: the page does what Escape does. Otherwise, once
   * Rust has made it the foreground window: a panel the page last reported
   * slid out by auto-hide is asked back first (`dock://show`, as
   * `requestShow` does), then one `dock://focus`. Payloads are `null`. A
   * browser has no foreground to take, so nothing else happens.
   */
  requestFocus(): void {
    if (this.shortcutFocus) {
      this.log('dock focus release requested');
      this.emit('dock://release-focus', null);
      return;
    }
    this.shortcutFocus = true;
    if (this.autoHidden) {
      this.log('dock reveal requested');
      this.emit('dock://show', null);
    }
    this.log('dock focus requested');
    this.emit('dock://focus', null);
  }
}
