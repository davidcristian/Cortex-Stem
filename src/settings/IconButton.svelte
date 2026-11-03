<script lang="ts">
  // 32 px square button with a 16-unit stroke icon and an accessible name.
  interface Props {
    label: string;
    /** `chevron-up`, `chevron-down` or `close`. */
    icon: 'chevron-up' | 'chevron-down' | 'close';
    disabled?: boolean;
    onclick: () => void;
  }

  let { label, icon, disabled = false, onclick }: Props = $props();
</script>

<button type="button" aria-label={label} title={label} {disabled} {onclick}>
  <svg viewBox="0 0 16 16" aria-hidden="true" focusable="false">
    {#if icon === 'chevron-up'}
      <path d="M3.5 10 8 5.5l4.5 4.5" />
    {:else if icon === 'chevron-down'}
      <path d="M3.5 6 8 10.5 12.5 6" />
    {:else}
      <path d="M4 4l8 8M12 4l-8 8" />
    {/if}
  </svg>
</button>

<style>
  button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    box-sizing: border-box;
    width: 32px;
    height: 32px;
    padding: 0;
    border: 1px solid transparent;
    border-radius: 4px;
    background: transparent;
    color: var(--text);
    cursor: default;
  }

  button:hover:not(:disabled) {
    background: var(--control-bg-hover);
    border-color: var(--control-border);
  }

  button:active:not(:disabled) {
    background: var(--control-bg);
    color: var(--text-secondary);
  }

  button:disabled {
    color: var(--text-disabled);
  }

  button:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: 1px;
  }

  svg {
    width: 16px;
    height: 16px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
</style>
