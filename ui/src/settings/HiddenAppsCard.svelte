<script lang="ts">
  import type { HiddenApp } from '$lib/ipc/generated/HiddenApp';
  import type { Settings } from '$lib/ipc/generated/Settings';
  import type { SettingsStore } from '$lib/stores/settings.svelte';

  import { removeRow } from './remove-row';
  import SettingsCard from './SettingsCard.svelte';

  // The rows of Hidden apps (docs/ipc/hidden-apps.md).
  interface Props {
    store: SettingsStore;
    settings: Settings;
    /** Device pixels of an app's icon in the list. */
    pinIconSize: number;
  }

  let { store, settings, pinIconSize }: Props = $props();

  // Request the icon of every hidden app that kept one; the cache makes
  // repeats free, so this may run on every change of the list or the
  // display scale.
  $effect(() => {
    for (const app of settings.hiddenApps) {
      if (app.iconKey !== null) {
        store.ensureIcon(app.iconKey, pinIconSize);
      }
    }
  });

  /** The name saved when the app was hidden; the id for a hand-written entry without one. */
  function hiddenName(app: HiddenApp): string {
    return app.name.trim() === '' ? app.id : app.name;
  }

  /**
   * Where a hidden app comes from, for its row's second line:
   * for an app with an AppUserModelID (`AppId::for_app_user_model_id`)
   * the package family of a packaged app (the part before its `!`), or the
   * whole AppUserModelID of an app that names its own; otherwise the
   * program's path saved when the app was hidden (`HiddenApp.path`, in its
   * original case); for an entry without one (saved by an older version) the
   * path read from an `exe:` id the Windows way, with backslashes and the
   * drive letter in capitals (the id keeps the rest in lower case); `null`
   * for anything else (a window class), which says nothing readable.
   */
  function hiddenLocation(app: HiddenApp): string | null {
    if (app.id.startsWith('aumid:')) {
      const aumid = app.id.slice('aumid:'.length);
      const bang = aumid.indexOf('!');
      return bang > 0 ? `Store app · ${aumid.slice(0, bang)}` : `App ID · ${aumid}`;
    }
    if (app.path !== null && app.path.trim() !== '') {
      return app.path;
    }
    if (app.id.startsWith('exe:')) {
      return app.id
        .slice('exe:'.length)
        .replaceAll('/', '\\')
        .replace(/^([a-z]):/, (_, drive: string) => `${drive.toUpperCase()}:`);
    }
    return null;
  }

  /**
   * The icon a hidden app's item showed when it was hidden, as for a
   * pin: a data URL, `null` when it kept none or the key no longer resolves
   * (one bound to a window that has closed), `undefined` while it loads.
   */
  function hiddenIcon(app: HiddenApp): string | null | undefined {
    return app.iconKey === null ? null : store.icon(app.iconKey, pinIconSize);
  }

  /** *Show*: off the list, the focus to a neighbour (`removeRow`). */
  function showHidden(app: HiddenApp): void {
    void removeRow('.hidden-app', 'button', 'h-hidden', app.id, () => store.unhideApp(app.id));
  }
</script>

<SettingsCard note="Choose Hide from Stem in a running app's menu to keep it off the dock.">
  {#if settings.hiddenApps.length === 0}
    <p class="empty" data-hidden-apps="none">No apps are hidden.</p>
  {:else}
    <ul class="hidden-apps" aria-label="Hidden apps">
      {#each settings.hiddenApps as app (app.id)}
        {@const location = hiddenLocation(app)}
        {@const icon = hiddenIcon(app)}
        <li
          class="hidden-app"
          data-id={app.id}
          data-icon={icon === undefined ? 'loading' : icon === null ? 'none' : 'loaded'}
        >
          <!-- The icon saved with the entry; the pins'
               neutral tile without one. Decorative, as for a pin. -->
          <span class="pin-icon" aria-hidden="true">
            {#if icon}
              <img src={icon} alt="" draggable="false" />
            {:else}
              <span class="placeholder"></span>
            {/if}
          </span>
          <div class="pin-text">
            <span class="pin-name">{hiddenName(app)}</span>
            {#if location !== null}
              <span class="pin-launch">{location}</span>
            {/if}
          </div>
          <button
            type="button"
            class="button"
            aria-label="Show {hiddenName(app)} on the dock"
            onclick={() => showHidden(app)}
          >
            Show
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</SettingsCard>

<style>
  /* Hidden apps: a pin's row, its icon included, with Show. */
  .hidden-apps {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .hidden-app {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 14px;
    min-height: 56px;
    box-sizing: border-box;
    padding: 8px 16px;
  }

  .hidden-app + .hidden-app {
    border-top: 1px solid var(--divider);
  }
</style>
