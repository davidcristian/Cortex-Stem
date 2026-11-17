<script lang="ts">
  import type { Settings } from '$lib/ipc/generated/Settings';
  import type { SettingsStore } from '$lib/stores/settings.svelte';

  interface Props {
    store: SettingsStore;
    /** The page's document; `null` while it loads. */
    settings: Settings | null;
  }

  let { store, settings }: Props = $props();

  // --- Reset ----------------------------------------------------------------------
  let confirmReset = $state(false);

  async function reset(): Promise<void> {
    confirmReset = false;
    await store.reset();
  }
</script>

<footer class="bottom">
  {#if confirmReset}
    <div class="confirm" role="alertdialog" aria-labelledby="confirm-reset">
      <p id="confirm-reset">
        Reset all settings to their defaults? Pinned apps are removed and hidden apps shown again.
      </p>
      <div class="actions">
        <button type="button" class="button accent" onclick={reset}>Reset</button>
        <button type="button" class="button" onclick={() => (confirmReset = false)}>
          Cancel
        </button>
      </div>
    </div>
  {:else}
    <p class="status" data-status={store.status} aria-live="polite">
      {#if store.status === 'saving'}
        <span class="dot"></span>Saving…
      {:else if store.status === 'error'}
        <span class="dot"></span>Couldn't save: {store.error}
      {:else if store.status === 'saved'}
        <svg class="check" viewBox="0 0 16 16" aria-hidden="true">
          <path d="M3 8.5 6.5 12 13 4.5" />
        </svg>Saved
      {:else}
        <span class="dot"></span>Loading…
      {/if}
    </p>
    <div class="actions">
      <button
        type="button"
        class="button"
        disabled={settings === null}
        onclick={() => (confirmReset = true)}
      >
        Reset to defaults
      </button>
    </div>
  {/if}
</footer>

<style>
  /* Footer */
  .bottom {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    min-height: 60px;
    box-sizing: border-box;
    padding: 10px 28px;
    border-top: 1px solid var(--card-border);
    background: var(--page-bg);
  }

  .status {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    font-size: 13px;
  }

  .status .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--text-secondary);
  }

  .status[data-status='saving'] .dot {
    background: var(--accent);
    animation: pulse 900ms ease-in-out infinite;
  }

  .status[data-status='error'] .dot {
    background: #ff99a4;
  }

  .status .check {
    width: 14px;
    height: 14px;
    fill: none;
    stroke: var(--accent);
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .confirm {
    display: flex;
    flex: 1 1 auto;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }

  .confirm p {
    margin: 0;
    font-size: 13px;
  }

  @keyframes pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.35;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .status[data-status='saving'] .dot {
      animation: none;
    }
  }
</style>
