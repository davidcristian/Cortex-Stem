/** Icons (mirror of `color_for` and `solid_png` in mock/icon.rs, as inline SVG). */

/** FNV-1a 64 → hue, exactly as `color_for` in mock/icon.rs, so colours match Rust. */
export function colorFor(key: string): [number, number, number] {
  let hash = 0xcbf29ce484222325n;
  for (const byte of new TextEncoder().encode(key)) {
    hash = BigInt.asUintN(64, (hash ^ BigInt(byte)) * 0x100000001b3n);
  }
  return hslToRgb(Number(hash % 360n), 0.65, 0.55);
}

function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = l - c / 2;
  // Six 60° hue sectors, same integer truncation as the Rust `match h as u32`.
  const sector = Math.trunc(h);
  const [r, g, b]: [number, number, number] =
    sector < 60
      ? [c, x, 0]
      : sector < 120
        ? [x, c, 0]
        : sector < 180
          ? [0, c, x]
          : sector < 240
            ? [0, x, c]
            : sector < 300
              ? [x, 0, c]
              : [c, 0, x];
  return [Math.trunc((r + m) * 255), Math.trunc((g + m) * 255), Math.trunc((b + m) * 255)];
}

/**
 * A rounded square (radius 22 % like `solid_png`) in the key's colour with the
 * app's initial letter, as an SVG data URL. Deterministic, so screenshots are
 * stable.
 */
export function mockIconDataUrl(key: string, letter: string, size: number): string {
  const [r, g, b] = colorFor(key);
  const text = letter.replace(/[<>&"]/g, '');
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 64 64">` +
    `<defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1">` +
    `<stop offset="0" stop-color="#fff" stop-opacity="0.22"/>` +
    `<stop offset="1" stop-color="#000" stop-opacity="0.12"/></linearGradient></defs>` +
    `<rect x="2" y="2" width="60" height="60" rx="14" fill="rgb(${r},${g},${b})"/>` +
    `<rect x="2" y="2" width="60" height="60" rx="14" fill="url(#g)"/>` +
    `<text x="32" y="33" dominant-baseline="central" text-anchor="middle" ` +
    `font-family="Segoe UI, Helvetica Neue, Arial, sans-serif" font-size="34" font-weight="600" ` +
    `fill="#fff">${text}</text></svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}
