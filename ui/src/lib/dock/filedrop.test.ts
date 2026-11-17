import { describe, expect, it } from 'vitest';

import type { DockItem } from '$lib/ipc/generated/DockItem';
import type { DockItemKind } from '$lib/ipc/generated/DockItemKind';

import { along, centreAlong, spanAlong, type Box } from './edge';
import {
  dropIndex,
  dropTargetAt,
  fileDropAction,
  fileNameOf,
  gapAnchor,
  itemAt,
  orderWithDropped,
  pinCandidates,
  pinnedIds,
  pinTargetFor,
  recycleSubject,
  sameAnchor,
  type FileDragSpot,
  type ItemSpan,
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
  const SPANS: ItemSpan[] = [
    { key: 'a', kind: 'app', start: 10, end: 66 },
    { key: 'sep', kind: 'separator', start: 72, end: 73 },
    { key: 'b', kind: 'app', start: 79, end: 135 },
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

describe('dropTargetAt', () => {
  const app: ItemSpan = { key: 'app:a', kind: 'app', start: 10, end: 66 };
  const sep: ItemSpan = { key: 'sep:3', kind: 'separator', start: 72, end: 73 };
  const bin: ItemSpan = { key: 'fixed:recycle-bin', kind: 'recycleBin', start: 79, end: 135 };

  it('is the item under the pointer, or nothing between items', () => {
    expect(dropTargetAt(30, [app, sep, bin])).toBe(app);
    expect(dropTargetAt(100, [app, sep, bin])).toBe(bin);
    expect(dropTargetAt(70, [app, sep, bin])).toBeNull();
    expect(dropTargetAt(5, [app, sep, bin])).toBeNull();
  });

  it('counts the padding past the Recycle Bin as the bin, so a gap can never push it under the pointer', () => {
    expect(dropTargetAt(135, [app, sep, bin])).toBe(bin);
    expect(dropTargetAt(143, [app, sep, bin])).toBe(bin);
    // Only the bin: past another last item there is nothing.
    expect(dropTargetAt(80, [app, sep])).toBeNull();
    expect(dropTargetAt(80, [])).toBeNull();
  });
});

describe('fileDropAction', () => {
  const spot = (over: Partial<FileDragSpot> = {}): FileDragSpot => ({
    onPanel: true,
    over: null,
    index: 2,
    ...over,
  });
  const BIN: ItemSpan = { key: 'fixed:recycle-bin', kind: 'recycleBin', start: 600, end: 656 };
  const APP: ItemSpan = { key: 'app:a', kind: 'app', start: 100, end: 156 };
  const SEPARATOR: ItemSpan = { key: 'sep:3', kind: 'separator', start: 586, end: 587 };
  const EXE = 'C:\\Tools\\putty.exe';
  const TXT = 'C:\\Users\\me\\notes.txt';
  const FOLDER = 'C:\\Users\\me\\Old photos';

  it('pins at the index anywhere else on the panel when the drag carries a program', () => {
    expect(fileDropAction(spot(), [EXE])).toEqual({ kind: 'pin', index: 2 });
    expect(fileDropAction(spot({ over: APP, index: 0 }), [TXT, EXE])).toEqual({
      kind: 'pin',
      index: 0,
    });
    // Next to the bin, on the separator or the padding, is not on the bin.
    expect(fileDropAction(spot({ over: SEPARATOR, index: 3 }), [EXE])).toEqual({
      kind: 'pin',
      index: 3,
    });
  });

  it('recycles over the Recycle Bin item, whatever the drag carries, and never pins there', () => {
    const recycle = { kind: 'recycle', key: 'fixed:recycle-bin' };
    expect(fileDropAction(spot({ over: BIN }), [EXE])).toEqual(recycle);
    expect(fileDropAction(spot({ over: BIN }), [TXT, FOLDER])).toEqual(recycle);
    expect(fileDropAction(spot({ over: BIN, index: 5 }), ['C:\\Shortcuts\\App.lnk'])).toEqual(
      recycle,
    );
  });

  it('does nothing off the panel, without paths, or elsewhere when nothing can be pinned', () => {
    expect(fileDropAction(spot({ onPanel: false }), [EXE])).toEqual({ kind: 'none' });
    expect(fileDropAction(spot({ onPanel: false, over: BIN }), [TXT])).toEqual({ kind: 'none' });
    expect(fileDropAction(spot({ over: BIN }), [])).toEqual({ kind: 'none' });
    expect(fileDropAction(spot(), [])).toEqual({ kind: 'none' });
    expect(fileDropAction(spot(), [TXT, FOLDER])).toEqual({ kind: 'none' });
    expect(fileDropAction(spot({ over: APP }), [TXT])).toEqual({ kind: 'none' });
  });
});

describe('recycleSubject', () => {
  it('names the one file or folder of a drop, or counts the items', () => {
    expect(recycleSubject(['C:\\Users\\me\\notes.txt'])).toBe('notes.txt');
    expect(recycleSubject(['C:\\Users\\me\\Old photos\\'])).toBe('Old photos');
    expect(recycleSubject(['C:\\'])).toBe('C:');
    expect(recycleSubject(['C:\\a.txt', 'C:\\b'])).toBe('2 items');
  });
});

describe('on a vertical dock', () => {
  // A left dock's column: an app, a separator and the bin, 56 px slots from y = 300.
  const boxes: Array<{ key: string; kind: DockItemKind; box: Box }> = [
    { key: 'app:a', kind: 'app', box: { left: 16, top: 300, width: 48, height: 56 } },
    { key: 'app:b', kind: 'app', box: { left: 16, top: 356, width: 48, height: 56 } },
    { key: 'sep:3', kind: 'separator', box: { left: 20, top: 418, width: 38, height: 1 } },
    {
      key: 'fixed:recycle-bin',
      kind: 'recycleBin',
      box: { left: 16, top: 425, width: 48, height: 56 },
    },
  ];
  const spans: ItemSpan[] = boxes.map(({ key, kind, box }) => ({
    key,
    kind,
    ...spanAlong('vertical', box),
  }));
  const centres = boxes.filter((b) => b.kind === 'app').map((b) => centreAlong('vertical', b.box));

  it("finds the item and the insertion index from the pointer's y; its x does not matter", () => {
    for (const x of [20, 60, 200]) {
      const pos = along('vertical', x, 370);
      expect(dropTargetAt(pos, spans)?.key).toBe('app:b');
      expect(dropIndex(pos, centres)).toBe(1);
    }
    expect(dropIndex(along('vertical', 40, 301), centres)).toBe(0);
    expect(dropIndex(along('vertical', 40, 400), centres)).toBe(2);
  });

  it('counts the padding below the bin as the bin', () => {
    expect(dropTargetAt(along('vertical', 40, 488), spans)?.key).toBe('fixed:recycle-bin');
    expect(dropTargetAt(along('vertical', 40, 415), spans)).toBeNull();
  });
});
