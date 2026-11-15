<script lang="ts">
  // Built-in vector art for the `shell:` icon namespace (docs/ipc/icon-keys.md).
  // Tiles use the same 22 % corner radius as app icons so the row reads as one.
  interface Props {
    /** The key without its `shell:` prefix, e.g. `start`, `recycle-bin-full`. */
    name: string;
  }

  let { name }: Props = $props();
</script>

<svg viewBox="0 0 64 64" aria-hidden="true" focusable="false">
  {#if name === 'start'}
    <defs>
      <linearGradient id="shell-start-shine" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0" stop-color="#fff" stop-opacity="0.28" />
        <stop offset="1" stop-color="#000" stop-opacity="0.14" />
      </linearGradient>
    </defs>
    <rect x="2" y="2" width="60" height="60" rx="14" fill="var(--accent)" />
    <rect x="2" y="2" width="60" height="60" rx="14" fill="url(#shell-start-shine)" />
    <rect x="16" y="16" width="14" height="14" rx="2.5" fill="#fff" />
    <rect x="34" y="16" width="14" height="14" rx="2.5" fill="#fff" />
    <rect x="16" y="34" width="14" height="14" rx="2.5" fill="#fff" />
    <rect x="34" y="34" width="14" height="14" rx="2.5" fill="#fff" />
  {:else if name === 'task-view'}
    <rect x="2" y="2" width="60" height="60" rx="14" fill="var(--tile-neutral)" />
    <rect
      x="22"
      y="14"
      width="28"
      height="20"
      rx="3"
      fill="none"
      stroke="currentColor"
      stroke-width="3"
    />
    <rect
      x="14"
      y="26"
      width="28"
      height="20"
      rx="3"
      fill="rgb(var(--panel-tint))"
      stroke="currentColor"
      stroke-width="3"
    />
  {:else if name === 'show-desktop'}
    <rect x="2" y="2" width="60" height="60" rx="14" fill="var(--tile-neutral)" />
    <rect
      x="14"
      y="16"
      width="36"
      height="26"
      rx="3"
      fill="none"
      stroke="currentColor"
      stroke-width="3"
    />
    <path d="M26 48h12M32 42v6" stroke="currentColor" stroke-width="3" stroke-linecap="round" />
    <rect x="36" y="30" width="10" height="8" rx="1.5" fill="currentColor" opacity="0.8" />
  {:else if name === 'quick-settings'}
    <!-- Two toggle switches, one on and one off: Windows' quick settings. -->
    <rect x="2" y="2" width="60" height="60" rx="14" fill="var(--tile-neutral)" />
    <rect x="12" y="15" width="40" height="15" rx="7.5" fill="currentColor" />
    <circle cx="44.5" cy="22.5" r="4.5" fill="rgb(var(--panel-tint))" />
    <rect
      x="13.5"
      y="35.5"
      width="37"
      height="12"
      rx="6"
      fill="none"
      stroke="currentColor"
      stroke-width="3"
    />
    <circle cx="21.5" cy="41.5" r="3.5" fill="currentColor" />
  {:else if name === 'notification-center'}
    <!-- A bell: Windows' notification center. -->
    <rect x="2" y="2" width="60" height="60" rx="14" fill="var(--tile-neutral)" />
    <path
      d="M20 39V28a12 12 0 0 1 24 0v11l3.5 4.5h-31Z"
      fill="none"
      stroke="currentColor"
      stroke-width="3"
      stroke-linejoin="round"
    />
    <path
      d="M27.5 47.5a4.5 4.5 0 0 0 9 0M32 12.5v3.5"
      fill="none"
      stroke="currentColor"
      stroke-width="3"
      stroke-linecap="round"
    />
  {:else if name === 'recycle-bin-empty' || name === 'recycle-bin-full'}
    <!-- A free-standing silver bin, like the macOS Trash: fluted body with a
         horizontal "cylinder" gradient, a flared rim, and a lid that sits flat
         when empty or rests tilted on crumpled paper when full. Fixed greys
         with a dark outline and white highlights read on both panel tints. -->
    <defs>
      <linearGradient id="shell-bin-body" x1="0" y1="0" x2="1" y2="0">
        <stop offset="0" stop-color="#8d8f98" />
        <stop offset="0.22" stop-color="#d6d7dd" />
        <stop offset="0.5" stop-color="#c2c3ca" />
        <stop offset="0.8" stop-color="#a7a8b0" />
        <stop offset="1" stop-color="#7e8089" />
      </linearGradient>
      <linearGradient id="shell-bin-lid" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0" stop-color="#eef0f4" />
        <stop offset="0.5" stop-color="#cfd1d8" />
        <stop offset="1" stop-color="#9c9ea7" />
      </linearGradient>
      <linearGradient id="shell-bin-rim" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0" stop-color="#e3e5ea" />
        <stop offset="1" stop-color="#a1a3ac" />
      </linearGradient>
    </defs>
    <!-- Drawn in a 64-unit box and scaled 8 % to match the weight of app tiles; the
         bin sits on the tiles' baseline (y = 62) so the tilted lid of the full
         variant stays inside the box at the top. -->
    <g transform="translate(-2.56 0.6) scale(1.08)" stroke-width="1" stroke="rgba(0,0,0,0.32)">
      <!-- body: tapered, rounded bottom corners -->
      <path
        d="M15.5 22.5 H48.5 L45.9 53.6 A4 4 0 0 1 41.9 57.3 H22.1 A4 4 0 0 1 18.1 53.6 Z"
        fill="url(#shell-bin-body)"
        stroke-linejoin="round"
      />
      <!-- fluting: shadow and highlight lines following the taper -->
      <path
        d="M21.5 27 L22.9 52.5 M27.5 27 L28.1 52.5 M33.5 27 L33.3 52.5 M39.5 27 L38.7 52.5"
        fill="none"
        stroke="rgba(0,0,0,0.16)"
        stroke-width="1.8"
        stroke-linecap="round"
      />
      <path
        d="M24.3 27 L25.3 52.5 M30.3 27 L30.5 52.5 M36.3 27 L35.8 52.5 M42.3 27 L41.2 52.5"
        fill="none"
        stroke="rgba(255,255,255,0.45)"
        stroke-width="1.4"
        stroke-linecap="round"
      />
      {#if name === 'recycle-bin-full'}
        <!-- crumpled paper above the rim -->
        <path
          d="M19 20 L22.5 12.5 L29 14.5 L33 7.5 L39.5 12 L45.5 10.5 L46 20 Z"
          fill="#f6f6f8"
          stroke-linejoin="round"
        />
        <path
          d="M22.5 12.5 L27 18 L29 14.5 M33 7.5 L34.5 15 L39.5 12 M34.5 15 L38 19.5 M45.5 10.5 L41 16"
          fill="none"
          stroke="rgba(0,0,0,0.2)"
          stroke-linejoin="round"
          stroke-linecap="round"
        />
      {/if}
      <!-- rim -->
      <rect x="12.5" y="18.5" width="39" height="5.5" rx="2.2" fill="url(#shell-bin-rim)" />
      <path d="M14.5 19.6 H49.5" stroke="rgba(255,255,255,0.7)" stroke-linecap="round" />
      <!-- lid: flat when empty, tilted and resting on the paper when full -->
      <g transform={name === 'recycle-bin-full' ? 'translate(1 -2) rotate(-14 16 19)' : undefined}>
        <rect x="14.5" y="12.5" width="35" height="6.5" rx="2.6" fill="url(#shell-bin-lid)" />
        <rect x="26.5" y="8.5" width="11" height="4.5" rx="2" fill="url(#shell-bin-lid)" />
        <path d="M16.5 13.6 H47.5" stroke="rgba(255,255,255,0.75)" stroke-linecap="round" />
      </g>
    </g>
  {:else}
    <rect x="2" y="2" width="60" height="60" rx="14" fill="var(--tile-neutral)" />
    <circle cx="32" cy="32" r="10" fill="none" stroke="currentColor" stroke-width="3" />
  {/if}
</svg>

<style>
  svg {
    display: block;
    width: 100%;
    height: 100%;
    color: var(--text);
  }
</style>
