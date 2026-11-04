import { PhysicalPosition } from '@tauri-apps/api/dpi';
import { describe, expect, it } from 'vitest';

import { fileDragFromTauri } from './tauri';

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
