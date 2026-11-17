import { describe, expect, it } from 'vitest';

import {
  across,
  along,
  awaySign,
  awayTranslate,
  axisOf,
  centreAlong,
  DOCK_EDGES,
  isDockEdge,
  menuAnchor,
  spanAlong,
} from './edge';

describe('edges and axes', () => {
  it('knows the four edges and nothing else', () => {
    expect([...DOCK_EDGES].sort()).toEqual(['bottom', 'left', 'right', 'top']);
    for (const edge of DOCK_EDGES) {
      expect(isDockEdge(edge)).toBe(true);
    }
    for (const value of ['', 'Bottom', 'centre', 'middle', null, undefined]) {
      expect(isDockEdge(value)).toBe(false);
    }
  });

  it('runs horizontally along bottom and top, vertically along left and right', () => {
    expect(axisOf('bottom')).toBe('horizontal');
    expect(axisOf('top')).toBe('horizontal');
    expect(axisOf('left')).toBe('vertical');
    expect(axisOf('right')).toBe('vertical');
  });

  it('reads a point along and across the axis', () => {
    expect(along('horizontal', 10, 20)).toBe(10);
    expect(across('horizontal', 10, 20)).toBe(20);
    expect(along('vertical', 10, 20)).toBe(20);
    expect(across('vertical', 10, 20)).toBe(10);
  });

  it('measures a box along the axis', () => {
    const box = { left: 100, top: 300, width: 48, height: 56 };
    expect(spanAlong('horizontal', box)).toEqual({ start: 100, end: 148 });
    expect(spanAlong('vertical', box)).toEqual({ start: 300, end: 356 });
    expect(centreAlong('horizontal', box)).toBe(124);
    expect(centreAlong('vertical', box)).toBe(328);
  });

  it('points away from the edge', () => {
    expect(awaySign('bottom')).toBe(-1);
    expect(awaySign('top')).toBe(1);
    expect(awaySign('left')).toBe(1);
    expect(awaySign('right')).toBe(-1);
  });

  it('moves a bounce away from the edge', () => {
    expect(awayTranslate('bottom', 42)).toBe('translateY(-42%)');
    expect(awayTranslate('top', 42)).toBe('translateY(42%)');
    expect(awayTranslate('left', 42)).toBe('translateX(42%)');
    expect(awayTranslate('right', 22)).toBe('translateX(-22%)');
    // No "-0%" at rest.
    expect(awayTranslate('bottom', 0)).toBe('translateY(0%)');
    expect(awayTranslate('right', 0)).toBe('translateX(0%)');
  });

  it("anchors a keyboard menu in the middle of the item's side away from the edge", () => {
    const box = { left: 100, top: 300, width: 48, height: 56 };
    expect(menuAnchor('bottom', box)).toEqual({ x: 124, y: 300 });
    expect(menuAnchor('top', box)).toEqual({ x: 124, y: 356 });
    expect(menuAnchor('left', box)).toEqual({ x: 148, y: 328 });
    expect(menuAnchor('right', box)).toEqual({ x: 100, y: 328 });
  });
});
