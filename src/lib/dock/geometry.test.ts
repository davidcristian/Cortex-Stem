import { describe, expect, it } from 'vitest';

import { DOCK_EDGES, type DockEdge } from './edge';
import {
  edgeShift,
  hitRegionFor,
  integerRect,
  LAYOUT_UNIT,
  magnifiedRise,
  panelRectReport,
  rectContains,
  rectContainsPoint,
  sameHitRegion,
  sameRect,
  sameSnap,
  snapMarginProperties,
  snapOffsets,
  type SnapMargins,
} from './geometry';

describe('integerRect', () => {
  it('rounds outwards so the rect still covers the original', () => {
    expect(integerRect({ x: 10.4, y: 20.6, width: 30.2, height: 5.1 })).toEqual({
      x: 10,
      y: 20,
      width: 31, // right edge 40.6 → 41
      height: 6, // bottom edge 25.7 → 26
    });
  });

  it('leaves whole-pixel rects alone', () => {
    const r = { x: 3, y: 4, width: 50, height: 60 };
    expect(integerRect(r)).toEqual(r);
  });

  it('never produces a negative size', () => {
    expect(integerRect({ x: 1, y: 1, width: -3, height: -3 })).toEqual({
      x: 1,
      y: 1,
      width: 0,
      height: 0,
    });
  });

  it('handles negative coordinates', () => {
    expect(integerRect({ x: -0.5, y: -2.5, width: 1, height: 1 })).toEqual({
      x: -1,
      y: -3,
      width: 2,
      height: 2,
    });
  });
});

describe('sameRect / rectContains', () => {
  it('compares every field', () => {
    const a = { x: 1, y: 2, width: 3, height: 4 };
    expect(sameRect(a, { ...a })).toBe(true);
    expect(sameRect(a, { ...a, height: 5 })).toBe(false);
  });

  it('containment allows touching edges', () => {
    const outer = { x: 0, y: 0, width: 100, height: 50 };
    expect(rectContains(outer, outer)).toBe(true);
    expect(rectContains(outer, { x: 10, y: 10, width: 90, height: 40 })).toBe(true);
    expect(rectContains(outer, { x: 10, y: 10, width: 91, height: 40 })).toBe(false);
    expect(rectContains(outer, { x: -1, y: 10, width: 10, height: 10 })).toBe(false);
  });

  it('a point is inside from the left/top edge up to, not including, the right/bottom edge', () => {
    const r = { x: 10, y: 20, width: 100, height: 50 };
    expect(rectContainsPoint(r, 10, 20)).toBe(true);
    expect(rectContainsPoint(r, 109.5, 69.5)).toBe(true);
    expect(rectContainsPoint(r, 110, 40)).toBe(false);
    expect(rectContainsPoint(r, 50, 70)).toBe(false);
    expect(rectContainsPoint(r, 9.9, 40)).toBe(false);
    expect(rectContainsPoint(r, 50, 19.9)).toBe(false);
  });
});

describe('hitRegionFor', () => {
  const panel = { x: 390.5, y: 168, width: 499, height: 64 };

  it('widens by half the growth on each side and raises by the rise, bottom fixed', () => {
    const { panel: p, envelope: e } = hitRegionFor(panel, 117.6, 36.8, 'bottom');
    expect(p).toEqual({ x: 390, y: 168, width: 500, height: 64 });
    // left edge 390.5 - 58.8 = 331.7 → 331; right edge 889.5 + 58.8 = 948.3 → 949
    expect(e).toEqual({ x: 331, y: 131, width: 618, height: 101 });
    expect(e.y + e.height).toBe(p.y + p.height);
  });

  it('on top it widens the same and grows downwards, its top edge fixed', () => {
    const { panel: p, envelope: e } = hitRegionFor({ ...panel, y: 8 }, 117.6, 36.8, 'top');
    expect(p).toEqual({ x: 390, y: 8, width: 500, height: 64 });
    // bottom edge 72 + 36.8 = 108.8 → 109
    expect(e).toEqual({ x: 331, y: 8, width: 618, height: 101 });
  });

  it('on a side edge it lengthens along y and grows away from the edge, its edge side fixed', () => {
    const side = { x: 8, y: 390.5, width: 64, height: 499 };
    const left = hitRegionFor(side, 117.6, 36.8, 'left');
    expect(left.panel).toEqual({ x: 8, y: 390, width: 64, height: 500 });
    // right edge 72 + 36.8 = 108.8 → 109
    expect(left.envelope).toEqual({ x: 8, y: 331, width: 101, height: 618 });
    const right = hitRegionFor({ ...side, x: 248 }, 117.6, 36.8, 'right');
    expect(right.panel).toEqual({ x: 248, y: 390, width: 64, height: 500 });
    // left edge 248 - 36.8 = 211.2 → 211; the right edge stays at 312
    expect(right.envelope).toEqual({ x: 211, y: 331, width: 101, height: 618 });
    expect(right.envelope.x + right.envelope.width).toBe(right.panel.x + right.panel.width);
  });

  it('always contains the panel, on every edge', () => {
    for (const edge of DOCK_EDGES) {
      for (const [g, r] of [
        [0, 0],
        [0, 8],
        [117.6, 36.8],
        [1, 1],
        [-5, -5],
      ] as const) {
        const region = hitRegionFor(panel, g, r, edge);
        expect(rectContains(region.envelope, region.panel)).toBe(true);
      }
    }
  });

  it('is the panel itself when nothing grows', () => {
    for (const edge of DOCK_EDGES) {
      const region = hitRegionFor({ x: 10, y: 20, width: 30, height: 40 }, 0, 0, edge);
      expect(sameHitRegion(region, { panel: region.panel, envelope: region.panel })).toBe(true);
    }
  });
});

describe('edgeShift', () => {
  const before = { width: 2560, height: 150 };

  it('follows the side of the window that moves when it grows away from its edge', () => {
    expect(edgeShift('bottom', before, { width: 2560, height: 1392 })).toEqual({ dx: 0, dy: 1242 });
    expect(edgeShift('top', before, { width: 2560, height: 1392 })).toEqual({ dx: 0, dy: 0 });
    const side = { width: 330, height: 1392 };
    expect(edgeShift('right', side, { width: 2560, height: 1392 })).toEqual({ dx: 2230, dy: 0 });
    expect(edgeShift('left', side, { width: 2560, height: 1392 })).toEqual({ dx: 0, dy: 0 });
  });

  it('runs backwards when the window shrinks again', () => {
    expect(edgeShift('bottom', { width: 2560, height: 1392 }, before)).toEqual({
      dx: 0,
      dy: -1242,
    });
  });
});

describe('panelRectReport', () => {
  const wide = { width: 1280, height: 240 };
  const tall = { width: 240, height: 1280 };
  // Rest boxes centred along the edge, 8 px from its side, fractional as measured.
  const row = { x: 360.25, y: 168, width: 559.5, height: 64 };
  const topRow = { ...row, y: 8 };
  const column = { x: 8, y: 360.25, width: 64, height: 559.5 };
  const rightColumn = { ...column, x: 168 };

  it('is the measurement rounded outwards in the viewport it was taken in when nothing changed', () => {
    expect(panelRectReport('bottom', row, wide, wide)).toEqual({
      rect: { x: 360, y: 168, width: 560, height: 64 },
      viewport: wide,
    });
    expect(panelRectReport('top', topRow, wide, wide)).toEqual({
      rect: { x: 360, y: 8, width: 560, height: 64 },
      viewport: wide,
    });
    expect(panelRectReport('left', column, tall, tall)).toEqual({
      rect: { x: 8, y: 360, width: 64, height: 560 },
      viewport: tall,
    });
    expect(panelRectReport('right', rightColumn, tall, tall)).toEqual({
      rect: { x: 168, y: 360, width: 64, height: 560 },
      viewport: tall,
    });
  });

  it('follows the panel into a window grown across the edge, in that viewport (a drag)', () => {
    const grown = { width: 1280, height: 1392 };
    expect(panelRectReport('bottom', row, wide, grown)).toEqual({
      rect: { x: 360, y: 168 + 1152, width: 560, height: 64 },
      viewport: grown,
    });
    expect(panelRectReport('top', topRow, wide, grown)).toEqual({
      rect: { x: 360, y: 8, width: 560, height: 64 },
      viewport: grown,
    });
    const wider = { width: 2560, height: 1280 };
    expect(panelRectReport('right', rightColumn, tall, wider)).toEqual({
      rect: { x: 168 + 2320, y: 360, width: 64, height: 560 },
      viewport: wider,
    });
    expect(panelRectReport('left', column, tall, wider)).toEqual({
      rect: { x: 8, y: 360, width: 64, height: 560 },
      viewport: wider,
    });
  });

  it('keeps the measured length after a length change not measured yet: the rect still describes it', () => {
    // Another work area, 280 px shorter along the edge, before the next
    // measurement: nothing moved the rect along the edge, so the viewport
    // says the old length there (and the current thickness across).
    const shorter = { width: 1000, height: 240 };
    expect(panelRectReport('bottom', row, wide, shorter)).toEqual({
      rect: { x: 360, y: 168, width: 560, height: 64 },
      viewport: wide,
    });
    expect(panelRectReport('top', topRow, wide, shorter).viewport).toEqual(wide);
    const lower = { width: 240, height: 1000 };
    expect(panelRectReport('left', column, tall, lower).viewport).toEqual(tall);
    expect(panelRectReport('right', rightColumn, tall, lower).viewport).toEqual(tall);
    // Both at once on `bottom`: the thickness is the current one, the length the measured one.
    const both = { width: 1000, height: 300 };
    expect(panelRectReport('bottom', row, wide, both)).toEqual({
      rect: { x: 360, y: 228, width: 560, height: 64 },
      viewport: { width: 1280, height: 300 },
    });
    expect(panelRectReport('right', rightColumn, tall, { width: 300, height: 1000 })).toEqual({
      rect: { x: 228, y: 360, width: 64, height: 560 },
      viewport: { width: 300, height: 1280 },
    });
  });
});

describe('magnifiedRise', () => {
  it('is the extra height of a fully magnified icon plus the slack', () => {
    expect(magnifiedRise(48, 1.6, 8)).toBeCloseTo(36.8);
    expect(magnifiedRise(48, 1, 8)).toBe(8);
    expect(magnifiedRise(48, 0.5, 8)).toBe(8);
  });
});

describe('snapOffsets', () => {
  const EDGE = 8;
  const REST: SnapMargins = { start: 0, edge: EDGE };

  /**
   * Where the panel's corner lands once `margins` replace `current`: centred
   * along the main axis (half the start margin), anchored to the edge side
   * (the edge margin one to one, away from the edge).
   */
  function apply(
    edge: DockEdge,
    left: number,
    top: number,
    current: SnapMargins,
    margins: SnapMargins,
  ): { left: number; top: number } {
    const main = (margins.start - current.start) / 2;
    const delta = margins.edge - current.edge;
    switch (edge) {
      case 'bottom':
        return { left: left + main, top: top - delta };
      case 'top':
        return { left: left + main, top: top + delta };
      case 'left':
        return { left: left + delta, top: top + main };
      case 'right':
        return { left: left - delta, top: top + main };
    }
  }

  it('leaves a panel that already sits on whole device pixels alone', () => {
    for (const edge of DOCK_EDGES) {
      expect(snapOffsets(360, 168, 1, REST, edge)).toEqual(REST);
      // 360.5 CSS px is 721 device px at 2×: whole.
      expect(snapOffsets(360.5, 168, 2, REST, edge)).toEqual(REST);
      expect(snapOffsets(360, 168, 1.5, REST, edge)).toEqual(REST);
    }
  });

  it('doubles the horizontal correction, because a flex-centred panel moves by half its margin', () => {
    // A 713 px panel centred in a 2560 px window at 100 %.
    const margins = snapOffsets(923.5, 168, 1, REST, 'bottom');
    expect(margins).toEqual({ start: 1, edge: EDGE });
    expect(apply('bottom', 923.5, 168, REST, margins).left).toBe(924);
  });

  it('corrects the top edge through the bottom margin, one to one', () => {
    // 169 CSS px is 253.5 device px at 1.5×; the nearest whole is 254, a third of a CSS px down.
    const margins = snapOffsets(360, 169, 1.5, REST, 'bottom');
    expect(margins.start).toBe(0);
    expect(margins.edge).toBeCloseTo(EDGE - 1 / 3, 12);
    const { top } = apply('bottom', 360, 169, REST, margins);
    expect(top * 1.5).toBeCloseTo(254, 12);
  });

  it('on a vertical dock doubles the vertical correction and corrects x through the edge margin', () => {
    // Centred along y: a top of 412.5 at 100 % asks for a top margin of 1.
    const left = snapOffsets(8.5, 412.5, 1, REST, 'left');
    expect(left).toEqual({ start: 1, edge: EDGE + 0.5 });
    expect(apply('left', 8.5, 412.5, REST, left)).toEqual({ left: 9, top: 413 });
    // On the right edge a larger right margin moves the panel left.
    const right = snapOffsets(247.5, 412.5, 1, REST, 'right');
    expect(right).toEqual({ start: 1, edge: EDGE - 0.5 });
    expect(apply('right', 247.5, 412.5, REST, right)).toEqual({ left: 248, top: 413 });
    // On the top edge a larger top margin moves the panel down.
    const top = snapOffsets(923.5, 8.5, 1, REST, 'top');
    expect(top).toEqual({ start: 1, edge: EDGE + 0.5 });
    expect(apply('top', 923.5, 8.5, REST, top)).toEqual({ left: 924, top: 9 });
  });

  it('works at any device pixel ratio, on every edge', () => {
    for (const edge of DOCK_EDGES) {
      for (const dpr of [1, 1.25, 1.5, 1.75, 2, 2.5, 3]) {
        for (const [left, top] of [
          [360.5, 168],
          [923.5, 113.328125],
          [100.25, 50.75],
          [0.1, 0.9],
          [-12.5, 3],
        ] as const) {
          const margins = snapOffsets(left, top, dpr, REST, edge);
          const landed = apply(edge, left, top, REST, margins);
          expect(landed.left * dpr).toBeCloseTo(Math.round(landed.left * dpr), 9);
          expect(landed.top * dpr).toBeCloseTo(Math.round(landed.top * dpr), 9);
          // Never more than half a device pixel either way.
          expect(Math.abs(landed.left - left) * dpr).toBeLessThanOrEqual(0.5 + 1e-9);
          expect(Math.abs(landed.top - top) * dpr).toBeLessThanOrEqual(0.5 + 1e-9);
        }
      }
    }
  });

  it('is idempotent: measuring the snapped panel asks for the same margins again', () => {
    for (const edge of DOCK_EDGES) {
      const first = snapOffsets(360.5, 169, 1.5, REST, edge);
      const landed = apply(edge, 360.5, 169, REST, first);
      const second = snapOffsets(landed.left, landed.top, 1.5, first, edge);
      expect(second.start).toBeCloseTo(first.start, 12);
      expect(second.edge).toBeCloseTo(first.edge, 12);
    }
  });

  it('builds on the margins the panel already has', () => {
    // Already nudged by 1 px, now half a pixel off again (an item came and went).
    expect(snapOffsets(200.5, 168, 1, { start: 1, edge: EDGE }, 'bottom')).toEqual({
      start: 2,
      edge: EDGE,
    });
    expect(snapOffsets(200.5, 168, 1, { start: -1, edge: EDGE }, 'bottom')).toEqual({
      start: 0,
      edge: EDGE,
    });
    expect(snapOffsets(200, 168.5, 1, { start: 0, edge: 7.5 }, 'bottom')).toEqual({
      start: 0,
      edge: 7,
    });
  });

  it('treats a nonsense device pixel ratio as 1', () => {
    for (const dpr of [0, NaN, -2]) {
      expect(snapOffsets(10.5, 20, dpr, REST, 'bottom')).toEqual({ start: 1, edge: EDGE });
    }
  });
});

describe('snapMarginProperties', () => {
  it('puts the start margin on the main axis and the edge margin on the edge side', () => {
    expect(snapMarginProperties('bottom')).toEqual({ start: 'marginLeft', edge: 'marginBottom' });
    expect(snapMarginProperties('top')).toEqual({ start: 'marginLeft', edge: 'marginTop' });
    expect(snapMarginProperties('left')).toEqual({ start: 'marginTop', edge: 'marginLeft' });
    expect(snapMarginProperties('right')).toEqual({ start: 'marginTop', edge: 'marginRight' });
  });
});

describe('sameSnap', () => {
  it('ignores corrections within two layout units, which could not move the panel', () => {
    const a = { start: 1 / 3, edge: 8 - 1 / 3 };
    expect(sameSnap(a, a)).toBe(true);
    expect(sameSnap(a, { ...a, start: a.start + LAYOUT_UNIT })).toBe(true);
    // Measured at 150 %: the laid-out 1/3 px margin asked for 1/48 px (1.33 units) more.
    expect(sameSnap(a, { ...a, start: a.start + 1 / 48 })).toBe(true);
    expect(sameSnap(a, { ...a, start: a.start - 2 * LAYOUT_UNIT })).toBe(true);
    expect(sameSnap(a, { ...a, edge: a.edge - LAYOUT_UNIT })).toBe(true);
  });

  it('sees a real correction', () => {
    const a = { start: 0, edge: 8 };
    expect(sameSnap(a, { start: 1, edge: 8 })).toBe(false);
    expect(sameSnap(a, { start: 0, edge: 7.5 })).toBe(false);
    expect(sameSnap(a, { start: 3 * LAYOUT_UNIT, edge: 8 })).toBe(false);
    // The smallest real snap at 200 %: a quarter CSS px, doubled.
    expect(sameSnap(a, { start: 0.5, edge: 8 })).toBe(false);
  });
});
