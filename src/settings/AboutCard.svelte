<script lang="ts">
  import type { SettingsStore } from '$lib/stores/settings.svelte';

  import SettingsCard from './SettingsCard.svelte';

  interface Props {
    store: SettingsStore;
  }

  let { store }: Props = $props();
</script>

<SettingsCard>
  {#if store.appInfo === null}
    <p class="about-pending" aria-live="polite">
      {store.aboutError === null
        ? 'Loading…'
        : `Couldn't read the app's details: ${store.aboutError}`}
    </p>
  {:else}
    {@const info = store.appInfo}
    <div class="about-row" data-row="version">
      <div class="about-text">
        <span class="about-label">Stem {info.version}</span>
        {#if info.platform === 'mock'}
          <span class="about-detail">Platform: mock (demo data)</span>
        {/if}
      </div>
    </div>
    <div class="about-row" data-row="settings">
      <div class="about-text">
        <span class="about-label">Settings file</span>
        <span class="about-detail"><code>{info.settingsPath}</code></span>
      </div>
      <div class="actions">
        <button
          type="button"
          class="button"
          aria-label="Open folder of the settings file"
          onclick={() => void store.openAppFolder('settings')}
        >
          Open folder
        </button>
      </div>
    </div>
    <div class="about-row" data-row="log">
      <div class="about-text">
        <span class="about-label">Log file</span>
        <span class="about-detail">
          {#if info.logPath === ''}
            File logging is off
          {:else}
            <code>{info.logPath}</code>
          {/if}
        </span>
      </div>
      <div class="actions">
        <button
          type="button"
          class="button"
          aria-label="Open folder of the log file"
          disabled={info.logPath === ''}
          onclick={() => void store.openAppFolder('logs')}
        >
          Open folder
        </button>
      </div>
    </div>
    {#if store.aboutError !== null}
      <p class="about-error" role="alert">Couldn't open the folder: {store.aboutError}</p>
    {/if}
  {/if}
</SettingsCard>

<style>
  /* About: the rows of SettingRow, with a path instead of a control's description. */
  .about-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 16px;
    min-height: 64px;
    box-sizing: border-box;
    padding: 12px 16px;
  }

  .about-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .about-label {
    font-size: 14px;
    line-height: 20px;
  }

  /* A path is read, and copied, whole: it wraps rather than being cut. */
  .about-detail {
    margin-top: 2px;
    color: var(--text-secondary);
    font-size: 12px;
    line-height: 16px;
    overflow-wrap: anywhere;
  }

  .about-error {
    margin: 0;
    padding: 10px 16px;
    color: var(--error-text);
    font-size: 12px;
    line-height: 16px;
    overflow-wrap: anywhere;
  }
</style>
