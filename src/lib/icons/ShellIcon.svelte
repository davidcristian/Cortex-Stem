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
  {:else if name === 'recycle-bin-empty' || name === 'recycle-bin-full'}
    {#if name === 'recycle-bin-full'}
      <path
        d="M23 21 L25 10 L37 7 L41 21 Z"
        fill="currentColor"
        fill-opacity="0.28"
        stroke="currentColor"
        stroke-width="2.5"
        stroke-linejoin="round"
      />
      <path
        d="M29 11l2 5M34 9l1 6"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        opacity="0.7"
      />
    {:else}
      <path
        d="M26 20v-4a3 3 0 0 1 3-3h6a3 3 0 0 1 3 3v4"
        fill="none"
        stroke="currentColor"
        stroke-width="3"
        stroke-linecap="round"
      />
    {/if}
    <path d="M14 21h36" stroke="currentColor" stroke-width="3.5" stroke-linecap="round" />
    <path
      d="M18 21h28l-2.6 32.5A4 4 0 0 1 39.4 57H24.6a4 4 0 0 1-4-3.5Z"
      fill="currentColor"
      fill-opacity="0.12"
      stroke="currentColor"
      stroke-width="3"
      stroke-linejoin="round"
    />
    <path
      d="M26 28v22M32 28v22M38 28v22"
      stroke="currentColor"
      stroke-width="2.5"
      stroke-linecap="round"
      opacity="0.55"
    />
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
