import { describe, expect, it } from 'vitest';

import type { DockItem } from '$lib/ipc/generated/DockItem';
import type { DockItemKind } from '$lib/ipc/generated/DockItemKind';

import {
  dropIndex,
  fileDropAction,
  fileNameOf,
  gapAnchor,
  itemAt,
  orderWithDropped,
  pinCandidates,
  pinnedIds,
  pinTargetFor,
  sameAnchor,
  type FileDragSpot,
} from './filedrop';

function item(kind: DockItemKind, key: string, pinned = false): DockItem {
  return {
    key,
    kind,
    appId: kind === 'app' ? key.slice('app:'.length) : null,
    title: key,
    iconKey: null,
    pinned,
    windows: [],
    isActive: false,
    needsAttention: false,
    badge: null,
  };
}

const app = (id: string, pinned: boolean): DockItem => item('app', `app:${id}`, pinned);

/** The demo layout: Start, a separator, three pinned apps, a separator, one running app, a separator, the bin. */
const DEMO: DockItem[] = [
  item('launcher', 'fixed:launcher'),
  item('separator', 'sep:1'),
  app('a', true),
  app('b', true),
  app('c', true),
  item('separator', 'sep:2'),
  app('r', false),
  item('separator', 'sep:3'),
  item('recycleBin', 'fixed:recycle-bin'),
];

describe('pinTargetFor (the UI-side filter; Rust decides in pin_target)', () => {
  it('asks to pin shortcuts as shortcuts and programs as exes', () => {
    expect(pinTargetFor('C:\\Users\\me\\Desktop\\Notepad++.lnk')).toEqual({
      kind: 'shortcut',
      path: 'C:\\Users\\me\\Desktop\\Notepad++.lnk',
    });
    expect(pinTargetFor('C:\\Windows\\notepad.exe')).toEqual({
      kind: 'exe',
      path: 'C:\\Windows\\notepad.exe',
      args: [],
      workingDir: null,
    });
  });

  it('ignores the letter case of the extension and keeps the path as dropped', () => {
    expect(pinTargetFor('C:\\TOOLS\\PUTTY.EXE')).toMatchObject({
      kind: 'exe',
      path: 'C:\\TOOLS\\PUTTY.EXE',
    });
    expect(pinTargetFor('D:/Links/App.Lnk')).toEqual({
      kind: 'shortcut',
      path: 'D:/Links/App.Lnk',
    });
  });

  it('leaves everything else out', () => {
    for (const path of [
      'C:\\Users\\me\\notes.txt',
      'C:\\Users\\me\\Documents',
      'C:\\Users\\me\\archive.exe.zip',
      'C:\\Users\\me\\.lnk',
      'C:\\Users\\me\\exe',
      'C:\\some.exe.dir\\README',
      'C:\\Users\\me\\Website.url',
      '',
    ]) {
      expect(pinTargetFor(path), path).toBeNull();
    }
  });
});

describe('pinCandidates', () => {
  it('keeps the pinnable paths in the order of the drag', () => {
    const paths = ['C:\\b.exe', 'C:\\x.txt', 'C:\\a.lnk', 'C:\\Folder'];
    expect(pinCandidates(paths).map((c) => [c.path, c.target.kind])).toEqual([
      ['C:\\b.exe', 'exe'],
      ['C:\\a.lnk', 'shortcut'],
    ]);
    expect(pinCandidates(['C:\\x.txt'])).toEqual([]);
    expect(pinCandidates([])).toEqual([]);
  });
});

describe('fileNameOf', () => {
  it('treats both separators alike', () => {
    expect(fileNameOf('C:\\Windows\\notepad.exe')).toBe('notepad.exe');
    expect(fileNameOf('C:/Windows/notepad.exe')).toBe('notepad.exe');
    expect(fileNameOf('notepad.exe')).toBe('notepad.exe');
  });
});

describe('pinnedIds', () => {
  it('lists the pinned apps in dock order and nothing else', () => {
    expect(pinnedIds(DEMO)).toEqual(['a', 'b', 'c']);
    expect(pinnedIds([])).toEqual([]);
  });
});

describe('dropIndex', () => {
  // Three pinned slots, 56 px apart, centred at 28, 84, 140.
  const CENTRES = Float64Array.of(28, 84, 140);

  it('counts the pinned centres left of the pointer: every position 0..=n', () => {
    expect(dropIndex(0, CENTRES)).toBe(0);
    expect(dropIndex(27, CENTRES)).toBe(0);
    expect(dropIndex(29, CENTRES)).toBe(1);
    expect(dropIndex(56, CENTRES)).toBe(1);
    expect(dropIndex(112, CENTRES)).toBe(2);
    expect(dropIndex(141, CENTRES)).toBe(3);
    expect(dropIndex(10_000, CENTRES)).toBe(3);
  });

  it('is 0 with nothing pinned', () => {
    expect(dropIndex(500, [])).toBe(0);
  });

  it('keeps its answer while the gap it opened pushes the neighbours away', () => {
    // The gap opened at 2, between the centres 84 and 140: the two
    // neighbours moved half a slot outwards each.
    const opened = Float64Array.of(0, 56, 168);
    for (const x of [85, 112, 139]) {
      expect(dropIndex(x, CENTRES)).toBe(2);
      expect(dropIndex(x, opened)).toBe(2);
    }
  });
});

describe('orderWithDropped', () => {
  const PINNED = ['a', 'b', 'c', 'd'];

  it('puts new ids at the index, in drop order', () => {
    expect(orderWithDropped(PINNED, ['x', 'y'], 2)).toEqual(['a', 'b', 'x', 'y', 'c', 'd']);
    expect(orderWithDropped(PINNED, ['x'], 0)).toEqual(['x', 'a', 'b', 'c', 'd']);
    expect(orderWithDropped(PINNED, ['x'], 4)).toEqual(['a', 'b', 'c', 'd', 'x']);
    expect(orderWithDropped([], ['x', 'y'], 0)).toEqual(['x', 'y']);
  });

  it('moves ids that were pinned already instead of repeating them', () => {
    // `a` was before the index, `d` after it; both end up at the gap.
    expect(orderWithDropped(PINNED, ['d', 'a'], 2)).toEqual(['b', 'd', 'a', 'c']);
    expect(orderWithDropped(PINNED, ['b'], 2)).toEqual(['a', 'b', 'c', 'd']);
    expect(orderWithDropped(PINNED, ['c', 'x'], 1)).toEqual(['a', 'c', 'x', 'b', 'd']);
  });

  it('lists an id dropped twice (two shortcuts to one program) once', () => {
    expect(orderWithDropped(PINNED, ['x', 'x'], 1)).toEqual(['a', 'x', 'b', 'c', 'd']);
  });

  it('clamps the index', () => {
    expect(orderWithDropped(PINNED, ['x'], -3)).toEqual(['x', 'a', 'b', 'c', 'd']);
    expect(orderWithDropped(PINNED, ['x'], 99)).toEqual(['a', 'b', 'c', 'd', 'x']);
  });

  it('is the old order when nothing was dropped', () => {
    expect(orderWithDropped(PINNED, [], 2)).toEqual(PINNED);
  });
});

describe('gapAnchor', () => {
  it('sits before the pinned item at the index, or after the last one', () => {
    expect(gapAnchor(DEMO, 0)).toEqual({ key: 'app:a', side: 'before' });
    expect(gapAnchor(DEMO, 2)).toEqual({ key: 'app:c', side: 'before' });
    expect(gapAnchor(DEMO, 3)).toEqual({ key: 'app:c', side: 'after' });
    expect(gapAnchor(DEMO, 9)).toEqual({ key: 'app:c', side: 'after' });
    expect(gapAnchor(DEMO, -1)).toEqual({ key: 'app:a', side: 'before' });
  });

  it('with nothing pinned, goes where the pinned group would start', () => {
    const unpinned = DEMO.filter((i) => !i.pinned && i.key !== 'sep:2');
    expect(gapAnchor(unpinned, 0)).toEqual({ key: 'app:r', side: 'before' });
    const fixedOnly = [
      item('launcher', 'fixed:launcher'),
      item('taskView', 'fixed:task-view'),
      item('separator', 'sep:1'),
      item('recycleBin', 'fixed:recycle-bin'),
    ];
    expect(gapAnchor(fixedOnly, 0)).toEqual({ key: 'fixed:task-view', side: 'after' });
    const binOnly = [item('recycleBin', 'fixed:recycle-bin')];
    expect(gapAnchor(binOnly, 0)).toEqual({ key: 'fixed:recycle-bin', side: 'before' });
    expect(gapAnchor([], 0)).toEqual({ key: null, side: 'after' });
  });
});

describe('sameAnchor', () => {
  it('compares by value and handles null', () => {
    expect(sameAnchor(null, null)).toBe(true);
    expect(sameAnchor({ key: 'a', side: 'before' }, { key: 'a', side: 'before' })).toBe(true);
    expect(sameAnchor({ key: 'a', side: 'before' }, { key: 'a', side: 'after' })).toBe(false);
    expect(sameAnchor({ key: 'a', side: 'before' }, null)).toBe(false);
    expect(sameAnchor(null, { key: null, side: 'after' })).toBe(false);
  });
});

describe('itemAt', () => {
  const SPANS = [
    { key: 'a', left: 10, right: 66 },
    { key: 'sep', left: 72, right: 73 },
    { key: 'b', left: 79, right: 135 },
  ];

  it('finds the item under the pointer; nothing between items', () => {
    expect(itemAt(10, SPANS)?.key).toBe('a');
    expect(itemAt(65.5, SPANS)?.key).toBe('a');
    expect(itemAt(66, SPANS)).toBeNull();
    expect(itemAt(72, SPANS)?.key).toBe('sep');
    expect(itemAt(100, SPANS)?.key).toBe('b');
    expect(itemAt(5, SPANS)).toBeNull();
    expect(itemAt(200, SPANS)).toBeNull();
  });
});

describe('fileDropAction', () => {
  const spot = (over: Partial<FileDragSpot> = {}): FileDragSpot => ({
    onPanel: true,
    over: null,
    index: 2,
    ...over,
  });

  it('pins at the index anywhere on the panel, whatever item is under the pointer', () => {
    expect(fileDropAction(spot(), true)).toEqual({ kind: 'pin', index: 2 });
    const bin = { key: 'fixed:recycle-bin', left: 0, right: 56 };
    expect(fileDropAction(spot({ over: bin, index: 3 }), true)).toEqual({ kind: 'pin', index: 3 });
  });

  it('does nothing off the panel or when nothing in the drag can be pinned', () => {
    expect(fileDropAction(spot({ onPanel: false }), true)).toEqual({ kind: 'none' });
    expect(fileDropAction(spot(), false)).toEqual({ kind: 'none' });
  });
});
