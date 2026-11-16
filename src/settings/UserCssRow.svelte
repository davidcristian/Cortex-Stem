<script lang="ts">
  import { userCssStatus, type UserCssAction } from '$lib/dock/user-css';
  import type { UserCss } from '$lib/ipc/generated/UserCss';
  import type { SettingsStore } from '$lib/stores/settings.svelte';
  import { describeError } from '$lib/stores/toasts.svelte';

  // The page keeps the row's state: it reads the file before the store starts.
  interface Props {
    store: SettingsStore;
    userCss: UserCss | null;
    userCssError: string | null;
    userCssLast: UserCssAction;
  }

  let {
    store,
    userCss = $bindable(),
    userCssError = $bindable(),
    userCssLast = $bindable(),
  }: Props = $props();

  async function editUserCss(): Promise<void> {
    userCssError = null;
    try {
      await store.backend.openUserCss();
      userCssLast = 'edit';
    } catch (err) {
      userCssError = `Couldn't open dock.css: ${describeError(err)}`;
      console.error('open_user_css failed:', err);
    }
  }

  async function reloadUserCss(): Promise<void> {
    userCssError = null;
    try {
      userCss = await store.backend.reloadUserCss();
      // The read says why it found nothing (`UserCss.problem`); an
      // Edit before it is no longer pending.
      userCssLast = 'reload';
    } catch (err) {
      userCssError = `Couldn't read dock.css: ${describeError(err)}`;
      console.error('reload_user_css failed:', err);
    }
  }
</script>

<!-- The user's stylesheet. Rust reads it at start and on
     Reload; Edit creates it from a template first. -->
<div class="file-row" data-row="user-css">
  <div class="file-text">
    <span class="file-label">Your styles (dock.css)</span>
    <span class="file-detail"><code>{userCss?.path ?? '…'}</code></span>
    <span class="file-detail" data-user-css-status>{userCssStatus(userCss, userCssLast)}</span>
  </div>
  <div class="actions">
    <button
      type="button"
      class="button"
      aria-label="Edit dock.css"
      onclick={() => void editUserCss()}
    >
      Edit dock.css
    </button>
    <button
      type="button"
      class="button"
      aria-label="Reload dock.css"
      onclick={() => void reloadUserCss()}
    >
      Reload
    </button>
  </div>
</div>
{#if userCssError !== null}
  <p class="file-error" role="alert">{userCssError}</p>
{/if}

<style>
  /* Appearance → dock.css: a row like SettingRow's, with the file's
     path and whether the dock uses it instead of a description. */
  .file-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 16px;
    min-height: 64px;
    box-sizing: border-box;
    padding: 12px 16px;
  }

  .file-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .file-label {
    font-size: 14px;
    line-height: 20px;
  }

  .file-detail {
    margin-top: 2px;
    color: var(--text-secondary);
    font-size: 12px;
    line-height: 16px;
    overflow-wrap: anywhere;
  }

  .file-error {
    margin: 0;
    padding: 10px 16px;
    color: var(--error-text);
    font-size: 12px;
    line-height: 16px;
    overflow-wrap: anywhere;
  }
</style>
