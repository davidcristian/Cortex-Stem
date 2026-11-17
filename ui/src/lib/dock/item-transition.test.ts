import { describe, expect, it } from 'vitest';

import type { DockAxis } from './edge';
import {
  axisOfNode,
  GAP_TRANSITION_MS,
  gapCss,
  gapTransition,
  ITEM_START_SCALE,
  ITEM_TRANSITION_MS,
  itemTransition,
  separatorCss,
  slotCss,
} from './item-transition';

/**
 * What the transition touches on its node: its `dataset`, and `closest`,
 * which finds the dock panel carrying `data-axis` (absent: none).
 */
function fakeNode(kind: string, panelAxis?: DockAxis): HTMLElement {
  const panel = panelAxis === undefined ? null : { dataset: { axis: panelAxis } };
  return {
    dataset: { kind } as DOMStringMap,
    closest: (selector: string) => (selector === '[data-axis]' ? panel : null),
  } as unknown as HTMLElement;
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

describe('on a vertical dock', () => {
  it('a slot grows in height, from nothing to its rest length', () => {
    const d0 = declarations(slotCss(0, 'vertical'));
    expect(d0.height).toBe('calc((var(--icon) * var(--scale, 1) + var(--slot-gap)) * 0)');
    expect(d0.width).toBeUndefined();
    expect(Object.keys(declarations(slotCss(0.5, 'vertical'))).sort()).toEqual([
      'height',
      'opacity',
      'transform',
    ]);
    expect(declarations(slotCss(1, 'vertical')).transform).toBe('scale(1)');
  });

  it('a separator collapses its height and its margins above and below', () => {
    const d = declarations(separatorCss(0, 'vertical'));
    expect(d.height).toBe('0px');
    expect(d['margin-top']).toBe('calc(var(--separator-margin) * 0)');
    expect(d['margin-bottom']).toBe('calc(var(--separator-margin) * 0)');
    expect(d.width).toBeUndefined();
    expect(d['margin-left']).toBeUndefined();
  });

  it('the gap grows in height', () => {
    expect(declarations(gapCss(1, 'vertical')).height).toBe(
      'calc((var(--icon) + var(--slot-gap)) * 1)',
    );
  });

  it('takes the axis from its parameters, else from the panel it is in, else horizontal', () => {
    expect(axisOfNode(fakeNode('app', 'vertical'))).toBe('vertical');
    expect(axisOfNode(fakeNode('app', 'horizontal'))).toBe('horizontal');
    expect(axisOfNode(fakeNode('app'))).toBe('horizontal');
    const fromPanel = itemTransition(
      fakeNode('app', 'vertical'),
      { enabled: true },
      { direction: 'in' },
    );
    expect(fromPanel.css?.(0.5, 0.5)).toBe(slotCss(0.5, 'vertical'));
    const fromParams = itemTransition(
      fakeNode('separator', 'horizontal'),
      { enabled: true, axis: 'vertical' },
      { direction: 'in' },
    );
    expect(fromParams.css?.(1, 0)).toBe(separatorCss(1, 'vertical'));
    expect(gapTransition(fakeNode('gap', 'vertical'), { enabled: true }).css?.(1, 0)).toBe(
      gapCss(1, 'vertical'),
    );
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
