import type * as TauriCore from '@tauri-apps/api/core';
import { PhysicalPosition } from '@tauri-apps/api/dpi';
import type * as TauriEvent from '@tauri-apps/api/event';
import type * as TauriWebview from '@tauri-apps/api/webview';
import { describe, expect, it, vi } from 'vitest';

import { fileDragFromTauri, TauriBackend } from './tauri';

// `invoke` is the only part of Tauri the commands touch: recorded here,
// everything else of `@tauri-apps/api/core` as it is.
const invokeMock = vi.hoisted(() => vi.fn(async (..._args: unknown[]) => undefined));
vi.mock('@tauri-apps/api/core', async (importOriginal) => ({
  ...(await importOriginal<typeof TauriCore>()),
  invoke: invokeMock,
}));

// The subscriptions: `listen` recorded (it resolves to an unlisten that is
// recorded too), and the page's webview is a second monitor's dock window.
const unlistenMock = vi.hoisted(() => vi.fn());
const listenMock = vi.hoisted(() =>
  vi.fn(async (..._args: unknown[]): Promise<() => void> => unlistenMock),
);
vi.mock('@tauri-apps/api/event', async (importOriginal) => ({
  ...(await importOriginal<typeof TauriEvent>()),
  listen: listenMock,
}));
vi.mock('@tauri-apps/api/webview', async (importOriginal) => ({
  ...(await importOriginal<typeof TauriWebview>()),
  getCurrentWebview: () => ({ label: 'dock-DISPLAY3' }),
}));

describe('fileDragFromTauri (docs/ipc/file-drags.md)', () => {
  const paths = ['C:\\Windows\\notepad.exe', 'C:\\Users\\me\\notes.txt'];

  it('turns physical pixels into CSS px of the viewport, once', () => {
    expect(
      fileDragFromTauri({ type: 'enter', paths, position: new PhysicalPosition(300, 150) }, 1.5),
    ).toEqual({ type: 'enter', paths, position: { x: 200, y: 100 } });
    expect(fileDragFromTauri({ type: 'over', position: new PhysicalPosition(640, 90) }, 2)).toEqual(
      { type: 'over', position: { x: 320, y: 45 } },
    );
    expect(
      fileDragFromTauri({ type: 'drop', paths, position: new PhysicalPosition(10, 20) }, 1),
    ).toEqual({ type: 'drop', paths, position: { x: 10, y: 20 } });
    expect(fileDragFromTauri({ type: 'leave' }, 2)).toEqual({ type: 'leave' });
  });

  it('copies the paths and treats a nonsensical ratio as 1', () => {
    const event = fileDragFromTauri(
      { type: 'drop', paths, position: new PhysicalPosition(10, 20) },
      0,
    );
    expect(event).toEqual({ type: 'drop', paths, position: { x: 10, y: 20 } });
    expect(event.type === 'drop' && event.paths).not.toBe(paths);
    expect(
      fileDragFromTauri({ type: 'over', position: new PhysicalPosition(10, 20) }, Number.NaN),
    ).toEqual({ type: 'over', position: { x: 10, y: 20 } });
  });
});

describe('TauriBackend geometry reports (docs/ipc/report-provenance.md, v1.18)', () => {
  it('sends the edge with all three and the viewport with the panel rect, as the commands name them', async () => {
    const backend = new TauriBackend();
    await backend.setDockContentSize(1300.5, 135, 'left');
    await backend.setDockHitRegion(
      { x: 8.4, y: 399.5, width: 64, height: 640 },
      { x: 8, y: 340, width: 110.2, height: 760 },
      'left',
    );
    await backend.setDockPanelRect({ x: 286.5, y: 80, width: 64, height: 640 }, 'right', {
      width: 360,
      height: 800,
    });
    await backend.setDockPanelRect({ x: 0, y: 0, width: 1, height: 1 }, 'top', {
      width: 1706.6,
      height: 95.5,
    });
    expect(invokeMock.mock.calls).toEqual([
      ['set_dock_content_size', { width: 1300.5, height: 135, edge: 'left' }],
      [
        'set_dock_hit_region',
        {
          // `Rect` is `i32` in Rust: rounded outwards.
          panel: { x: 8, y: 399, width: 65, height: 641 },
          envelope: { x: 8, y: 340, width: 111, height: 760 },
          edge: 'left',
        },
      ],
      [
        'set_dock_panel_rect',
        {
          rect: { x: 286, y: 80, width: 65, height: 640 },
          edge: 'right',
          viewport: { width: 360, height: 800 },
        },
      ],
      [
        'set_dock_panel_rect',
        {
          rect: { x: 0, y: 0, width: 1, height: 1 },
          edge: 'top',
          // Whole CSS px.
          viewport: { width: 1707, height: 96 },
        },
      ],
    ]);
  });
});

describe('TauriBackend keyboard entry (docs/ipc/keyboard-entry.md, v1.19)', () => {
  it('gives the focus back with release_dock_focus and no arguments', async () => {
    const backend = new TauriBackend();
    const before = invokeMock.mock.calls.length;
    await backend.releaseDockFocus();
    expect(invokeMock.mock.calls.slice(before)).toEqual([['release_dock_focus']]);
  });
});

describe('TauriBackend events (docs/ipc/events.md)', () => {
  it('listens on its own window, so an event Rust sends to another dock window never reaches it', () => {
    const backend = new TauriBackend();
    const events = [
      'dock://state',
      'dock://settings',
      'dock://show',
      'dock://focus',
      'dock://release-focus',
      'dock://user-css',
    ] as const;
    for (const event of events) {
      backend.on(event, () => undefined);
    }
    // Not `{ kind: 'Any' }` (`listen`'s default), which Tauri hands every
    // `emit_to` whatever window it names.
    expect(listenMock.mock.calls.map(([event, , options]) => [event, options])).toEqual(
      events.map((event) => [event, { target: { kind: 'WebviewWindow', label: 'dock-DISPLAY3' } }]),
    );
  });

  it('hands the payload to the handler until it unsubscribes, then unlistens', async () => {
    const backend = new TauriBackend();
    const seen: unknown[] = [];
    const stop = backend.on('dock://focus', (payload) => seen.push(payload));
    const [, callback] = listenMock.mock.calls[0] as [string, (e: { payload: unknown }) => void];
    callback({ payload: null });
    expect(seen).toEqual([null]);
    // Let the registration resolve, so the unlisten is known.
    await Promise.resolve();
    await Promise.resolve();
    stop();
    expect(unlistenMock).toHaveBeenCalledTimes(1);
    callback({ payload: null });
    expect(seen).toEqual([null]);
  });
});

describe('TauriBackend hidden apps (docs/ipc/hidden-apps.md, v1.23)', () => {
  it('sends hide_app and unhide_app with the app id under appId', async () => {
    const backend = new TauriBackend();
    const before = invokeMock.mock.calls.length;
    await backend.hideApp('exe:c:/windows/notepad.exe');
    await backend.unhideApp('exe:c:/windows/notepad.exe');
    expect(invokeMock.mock.calls.slice(before)).toEqual([
      ['hide_app', { appId: 'exe:c:/windows/notepad.exe' }],
      ['unhide_app', { appId: 'exe:c:/windows/notepad.exe' }],
    ]);
  });
});

describe('TauriBackend scrolling over an item (docs/ipc/scrolling-over-an-item.md, v1.24)', () => {
  it('sends scroll_item with the item key and the direction', async () => {
    const backend = new TauriBackend();
    const before = invokeMock.mock.calls.length;
    await backend.scrollItem('app:exe:c:/windows/notepad.exe', 'next');
    await backend.scrollItem('app:exe:c:/windows/notepad.exe', 'previous');
    expect(invokeMock.mock.calls.slice(before)).toEqual([
      ['scroll_item', { itemKey: 'app:exe:c:/windows/notepad.exe', direction: 'next' }],
      ['scroll_item', { itemKey: 'app:exe:c:/windows/notepad.exe', direction: 'previous' }],
    ]);
  });
});
