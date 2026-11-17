/**
 * The device pixel ratio, watched. The dock and the settings page
 * keep running while the display scale changes (Windows Settings → System →
 * Display → Scale) or the window lands on a monitor with another DPI, and
 * both draw icons requested at `CSS size × ratio` device pixels
 * (icon-size.ts; the dock also snaps its panel to whole device pixels,
 * geometry.ts), so the ratio has to be followed, not read once. No `resize`
 * event need come with a scale change (the viewport keeps its CSS size), so
 * the ratio is watched the standard way: a media query on the current
 * value, `(resolution: <ratio>dppx)`, matches exactly that one ratio and
 * fires `change` once it stops matching; it is then re-armed on the new
 * value.
 *
 * The pages pass `window`; the tests a fake. Nothing here touches the DOM.
 */

/** The part of `MediaQueryList` the watcher needs. */
export interface RatioQuery {
  addEventListener(type: 'change', listener: () => void): void;
  removeEventListener(type: 'change', listener: () => void): void;
}

/** The part of `Window` the watcher needs. */
export interface RatioSource {
  readonly devicePixelRatio: number;
  matchMedia(query: string): RatioQuery;
}

/** A usable ratio: finite and positive, else 1 (an unknown or broken value means 100 %). */
export function normalizeRatio(raw: number | undefined): number {
  return raw !== undefined && Number.isFinite(raw) && raw > 0 ? raw : 1;
}

/** The media query that matches exactly `ratio` device pixels per CSS px. */
export function ratioQuery(ratio: number): string {
  return `(resolution: ${ratio}dppx)`;
}

/**
 * Watch the device pixel ratio of `source`: `onChange` gets the current
 * ratio now and every ratio it changes to later, never the same value twice
 * in a row (a query that fires after the ratio has changed and changed back
 * only re-arms). Returns the function that stops watching.
 */
export function watchDevicePixelRatio(
  source: RatioSource,
  onChange: (ratio: number) => void,
): () => void {
  let current = normalizeRatio(source.devicePixelRatio);
  let query: RatioQuery | null = null;
  const stop = (): void => {
    query?.removeEventListener('change', onQueryChange);
    query = null;
  };
  const arm = (): void => {
    query = source.matchMedia(ratioQuery(current));
    query.addEventListener('change', onQueryChange);
  };
  function onQueryChange(): void {
    // Each query matches one ratio: the one that fired is done with.
    stop();
    const previous = current;
    current = normalizeRatio(source.devicePixelRatio);
    arm();
    if (current !== previous) {
      onChange(current);
    }
  }
  arm();
  onChange(current);
  return stop;
}
