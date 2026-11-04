import { describe, expect, it } from 'vitest';

import {
  GAP_TRANSITION_MS,
  gapCss,
  gapTransition,
  ITEM_START_SCALE,
  ITEM_TRANSITION_MS,
  itemTransition,
  separatorCss,
  slotCss,
} from './item-transition';

/** The two things the transition touches on its node. */
function fakeNode(kind: string): HTMLElement {
  return { dataset: { kind } as DOMStringMap } as HTMLElement;
}

/** `prop: value` pairs of an inline style string. */
function declarations(css: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const part of css.split(';')) {
    const colon = part.indexOf(':');
    if (colon !== -1) {
      out[part.slice(0, colon).trim()] = part.slice(colon + 1).trim();
    }
  }
  return out;
}

describe('slotCss', () => {
  it('is collapsed, invisible and shrunk at t = 0', () => {
    const d = declarations(slotCss(0));
    expect(d.width).toBe('calc((var(--icon) * var(--scale, 1) + var(--slot-gap)) * 0)');
    expect(d.opacity).toBe('0');
    expect(d.transform).toBe(`scale(${ITEM_START_SCALE})`);
  });

  it('is exactly the rest state at t = 1', () => {
    const d = declarations(slotCss(1));
    expect(d.width).toBe('calc((var(--icon) * var(--scale, 1) + var(--slot-gap)) * 1)');
    expect(d.opacity).toBe('1');
    expect(d.transform).toBe('scale(1)');
  });

  it('uses only the properties the dock already animates (width, opacity, transform)', () => {
    expect(Object.keys(declarations(slotCss(0.5))).sort()).toEqual([
      'opacity',
      'transform',
      'width',
    ]);
  });
});

describe('separatorCss', () => {
  it('collapses the line and its margins together', () => {
    const d0 = declarations(separatorCss(0));
    expect(d0.width).toBe('0px');
    expect(d0['margin-left']).toBe('calc(var(--separator-margin) * 0)');
    expect(d0.opacity).toBe('0');
    const d1 = declarations(separatorCss(1));
    expect(d1.width).toBe('1px');
    expect(d1['margin-right']).toBe('calc(var(--separator-margin) * 1)');
  });
});

describe('itemTransition', () => {
  it('marks a leaving slot and plays the outro', () => {
    const node = fakeNode('app');
    const config = itemTransition(node, { enabled: true }, { direction: 'out' });
    expect(node.dataset.leaving).toBe('');
    expect(config.duration).toBe(ITEM_TRANSITION_MS);
    expect(config.css?.(0.5, 0.5)).toContain('opacity: 0.5');
    expect(config.css?.(0.5, 0.5)).toContain('width: calc(');
  });

  it('clears the mark on the way in', () => {
    const node = fakeNode('app');
    node.dataset.leaving = '';
    itemTransition(node, { enabled: true }, { direction: 'in' });
    expect('leaving' in node.dataset).toBe(false);
  });

  it('plays nothing when disabled but still keeps the mark correct', () => {
    const node = fakeNode('app');
    expect(itemTransition(node, { enabled: false }, { direction: 'out' })).toEqual({
      duration: 0,
    });
    expect(node.dataset.leaving).toBe('');
    expect(itemTransition(node, { enabled: false }, { direction: 'in' })).toEqual({
      duration: 0,
    });
    expect('leaving' in node.dataset).toBe(false);
  });

  it('picks the separator keyframes by the slot kind', () => {
    const config = itemTransition(fakeNode('separator'), { enabled: true }, { direction: 'in' });
    expect(config.css?.(1, 0)).toBe(separatorCss(1));
    const app = itemTransition(fakeNode('app'), { enabled: true }, { direction: 'in' });
    expect(app.css?.(1, 0)).toBe(slotCss(1));
  });
});

describe('gapCss / gapTransition (the drop gap)', () => {
  it('grows from nothing to exactly one rest slot, like an item', () => {
    const d0 = declarations(gapCss(0));
    expect(d0.width).toBe('calc((var(--icon) + var(--slot-gap)) * 0)');
    expect(d0.opacity).toBe('0');
    expect(d0.transform).toBe(`scale(${ITEM_START_SCALE})`);
    const d1 = declarations(gapCss(1));
    expect(d1.width).toBe('calc((var(--icon) + var(--slot-gap)) * 1)');
    expect(d1.opacity).toBe('1');
    expect(d1.transform).toBe('scale(1)');
    expect(Object.keys(declarations(gapCss(0.5))).sort()).toEqual([
      'opacity',
      'transform',
      'width',
    ]);
  });

  it('takes the 160 ms of the reorder gap, and nothing when disabled', () => {
    const node = fakeNode('gap');
    const config = gapTransition(node, { enabled: true });
    expect(config.duration).toBe(GAP_TRANSITION_MS);
    expect(GAP_TRANSITION_MS).toBe(160);
    expect(config.css?.(1, 0)).toBe(gapCss(1));
    expect(gapTransition(node, { enabled: false })).toEqual({ duration: 0 });
  });
});
