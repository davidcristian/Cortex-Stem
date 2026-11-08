import { describe, expect, it } from 'vitest';

import type { DockItemKind } from '$lib/ipc/generated/DockItemKind';

import {
  isFocusable,
  isNavigationKey,
  nextFocusIndex,
  successorKey,
  tabStopKey,
  type NavigableItem,
} from './keyboard';

function item(key: string, kind: DockItemKind = 'app'): NavigableItem {
  return { key, kind };
}

// The demo dock's shape: Start | apps | Recycle Bin, with separators between.
const ITEMS: NavigableItem[] = [
  item('launcher', 'launcher'), // 0
  item('separator:0', 'separator'), // 1
  item('app:a'), // 2
  item('app:b'), // 3
  item('app:c'), // 4
  item('separator:1', 'separator'), // 5
  item('recycle-bin', 'recycleBin'), // 6
];

describe('isNavigationKey', () => {
  it('knows the arrows, Home and End and nothing else', () => {
    for (const key of ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End']) {
      expect(isNavigationKey(key)).toBe(true);
    }
    for (const key of ['Enter', ' ', 'Tab', 'Escape', 'F10', 'ContextMenu', 'PageDown', 'a', '']) {
      expect(isNavigationKey(key)).toBe(false);
    }
  });
});

describe('isFocusable', () => {
  it('takes every kind but a separator', () => {
    const kinds: DockItemKind[] = ['app', 'launcher', 'taskView', 'showDesktop', 'recycleBin'];
    for (const kind of kinds) {
      expect(isFocusable({ kind })).toBe(true);
    }
    expect(isFocusable({ kind: 'separator' })).toBe(false);
  });
});

describe('nextFocusIndex', () => {
  it('moves one item right or down, skipping separators', () => {
    expect(nextFocusIndex(ITEMS, 0, 'ArrowRight')).toBe(2);
    expect(nextFocusIndex(ITEMS, 2, 'ArrowRight')).toBe(3);
    expect(nextFocusIndex(ITEMS, 4, 'ArrowRight')).toBe(6);
    expect(nextFocusIndex(ITEMS, 0, 'ArrowDown')).toBe(2);
    expect(nextFocusIndex(ITEMS, 4, 'ArrowDown')).toBe(6);
  });

  it('moves one item left or up, skipping separators', () => {
    expect(nextFocusIndex(ITEMS, 6, 'ArrowLeft')).toBe(4);
    expect(nextFocusIndex(ITEMS, 3, 'ArrowLeft')).toBe(2);
    expect(nextFocusIndex(ITEMS, 2, 'ArrowLeft')).toBe(0);
    expect(nextFocusIndex(ITEMS, 6, 'ArrowUp')).toBe(4);
    expect(nextFocusIndex(ITEMS, 2, 'ArrowUp')).toBe(0);
  });

  it('stops at either end instead of wrapping', () => {
    expect(nextFocusIndex(ITEMS, 6, 'ArrowRight')).toBe(6);
    expect(nextFocusIndex(ITEMS, 6, 'ArrowDown')).toBe(6);
    expect(nextFocusIndex(ITEMS, 0, 'ArrowLeft')).toBe(0);
    expect(nextFocusIndex(ITEMS, 0, 'ArrowUp')).toBe(0);
  });

  it('Home and End go to the first and the last item from anywhere', () => {
    for (let current = -1; current <= ITEMS.length; current += 1) {
      expect(nextFocusIndex(ITEMS, current, 'Home')).toBe(0);
      expect(nextFocusIndex(ITEMS, current, 'End')).toBe(6);
    }
  });

  it('Home and End skip separators at the ends', () => {
    const fenced = [
      item('separator:a', 'separator'),
      item('app:x'),
      item('app:y'),
      item('separator:b', 'separator'),
    ];
    expect(nextFocusIndex(fenced, 2, 'Home')).toBe(1);
    expect(nextFocusIndex(fenced, 1, 'End')).toBe(2);
    expect(nextFocusIndex(fenced, 2, 'ArrowRight')).toBe(2);
    expect(nextFocusIndex(fenced, 1, 'ArrowLeft')).toBe(1);
  });

  it('from outside the list, forward keys start at the first item and backward ones at the last', () => {
    for (const current of [-1, ITEMS.length, 99, Number.NaN, 1.5]) {
      expect(nextFocusIndex(ITEMS, current, 'ArrowRight')).toBe(0);
      expect(nextFocusIndex(ITEMS, current, 'ArrowDown')).toBe(0);
      expect(nextFocusIndex(ITEMS, current, 'ArrowLeft')).toBe(6);
      expect(nextFocusIndex(ITEMS, current, 'ArrowUp')).toBe(6);
    }
  });

  it('from a separator, moves to the nearest item in the key direction', () => {
    expect(nextFocusIndex(ITEMS, 1, 'ArrowRight')).toBe(2);
    expect(nextFocusIndex(ITEMS, 1, 'ArrowLeft')).toBe(0);
    expect(nextFocusIndex(ITEMS, 5, 'ArrowRight')).toBe(6);
    expect(nextFocusIndex(ITEMS, 5, 'ArrowLeft')).toBe(4);
  });

  it('moves nothing for other keys, or when nothing takes focus', () => {
    for (const key of ['Enter', ' ', 'Tab', 'Escape', 'a']) {
      expect(nextFocusIndex(ITEMS, 2, key)).toBeNull();
    }
    const separators = [item('separator:a', 'separator'), item('separator:b', 'separator')];
    for (const key of ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End']) {
      expect(nextFocusIndex([], 0, key)).toBeNull();
      expect(nextFocusIndex([], -1, key)).toBeNull();
      expect(nextFocusIndex(separators, 0, key)).toBeNull();
    }
  });

  it('with one item every key stays on it', () => {
    const one = [item('app:only')];
    for (const key of ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End']) {
      expect(nextFocusIndex(one, 0, key)).toBe(0);
    }
  });

  it('a held arrow walks every item once and ends where End does', () => {
    const visited: number[] = [];
    let current = nextFocusIndex(ITEMS, -1, 'Home')!;
    visited.push(current);
    for (let i = 0; i < ITEMS.length + 2; i += 1) {
      current = nextFocusIndex(ITEMS, current, 'ArrowRight')!;
      if (current !== visited.at(-1)) {
        visited.push(current);
      }
    }
    expect(visited).toEqual([0, 2, 3, 4, 6]);
    expect(current).toBe(nextFocusIndex(ITEMS, 0, 'End'));
  });
});

describe('tabStopKey', () => {
  it('is the first item until the focus has been somewhere', () => {
    expect(tabStopKey(ITEMS, null)).toBe('launcher');
    const fenced = [item('separator:a', 'separator'), item('app:x'), item('app:y')];
    expect(tabStopKey(fenced, null)).toBe('app:x');
  });

  it('remembers the item the focus was on last', () => {
    expect(tabStopKey(ITEMS, 'app:b')).toBe('app:b');
    expect(tabStopKey(ITEMS, 'recycle-bin')).toBe('recycle-bin');
  });

  it('falls back to the first item once the remembered one is gone, or is a separator', () => {
    expect(tabStopKey(ITEMS, 'app:gone')).toBe('launcher');
    expect(tabStopKey(ITEMS, 'separator:1')).toBe('launcher');
  });

  it('is null when nothing takes focus', () => {
    expect(tabStopKey([], null)).toBeNull();
    expect(tabStopKey([], 'app:a')).toBeNull();
    expect(tabStopKey([item('separator:a', 'separator')], 'separator:a')).toBeNull();
  });
});

describe('successorKey', () => {
  const without = (key: string): NavigableItem[] => ITEMS.filter((i) => i.key !== key);

  it('is the next item after the one that left, past separators', () => {
    expect(successorKey(ITEMS, without('app:b'), 'app:b')).toBe('app:c');
    expect(successorKey(ITEMS, without('app:c'), 'app:c')).toBe('recycle-bin');
    expect(successorKey(ITEMS, without('launcher'), 'launcher')).toBe('app:a');
  });

  it('is the previous item when the last one left', () => {
    expect(successorKey(ITEMS, without('recycle-bin'), 'recycle-bin')).toBe('app:c');
  });

  it('skips neighbours that left with it', () => {
    const after = ITEMS.filter((i) => !['app:b', 'app:c', 'recycle-bin'].includes(i.key));
    expect(successorKey(ITEMS, after, 'app:b')).toBe('app:a');
  });

  it('falls back to the first item when the old list does not know the key, or shares nothing', () => {
    expect(successorKey(ITEMS, without('app:b'), 'app:unknown')).toBe('launcher');
    const fresh = [item('separator:x', 'separator'), item('app:new')];
    expect(successorKey(ITEMS, fresh, 'app:b')).toBe('app:new');
  });

  it('is null when nothing is left to take focus', () => {
    expect(successorKey(ITEMS, [], 'app:b')).toBeNull();
    expect(successorKey(ITEMS, [item('separator:0', 'separator')], 'app:b')).toBeNull();
  });
});
