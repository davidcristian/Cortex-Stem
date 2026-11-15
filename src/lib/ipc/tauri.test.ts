import type * as TauriCore from '@tauri-apps/api/core';
import { PhysicalPosition } from '@tauri-apps/api/dpi';
import { describe, expect, it, vi } from 'vitest';

import { fileDragFromTauri, TauriBackend } from './tauri';

// `invoke` is the only part of Tauri the commands touch: recorded here,
// everything else of `@tauri-apps/api/core` as it is.
const invokeMock = vi.hoisted(() => vi.fn(async (..._args: unknown[]) => undefined));
vi.mock('@tauri-apps/api/core', async (importOriginal) => ({
  ...(await importOriginal<typeof TauriCore>()),
  invoke: invokeMock,
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
