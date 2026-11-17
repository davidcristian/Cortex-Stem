<script lang="ts">
  import { moveItem } from '$lib/dock/reorder';
  import type { IconKey } from '$lib/ipc/generated/IconKey';
  import type { PinnedApp } from '$lib/ipc/generated/PinnedApp';
  import type { Settings } from '$lib/ipc/generated/Settings';
  import type { SettingsStore } from '$lib/stores/settings.svelte';

  import IconButton from './IconButton.svelte';
  import { removeRow } from './remove-row';
  import SettingsCard from './SettingsCard.svelte';

  interface Props {
    store: SettingsStore;
    settings: Settings;
    /** Device pixels of an app's icon in the list. */
    pinIconSize: number;
  }

  let { store, settings, pinIconSize }: Props = $props();

  /**
   * What the dock shows for the entry: the user's label, else the
   * title the engine resolved for the app (its item in the dock state), else
   * the app id while nothing is known yet.
   */
  function pinnedName(pin: PinnedApp): string {
    return pin.label ?? store.dockItem(pin.id)?.title ?? pin.id;
  }

  /** The icon key the dock resolved for the pinned app, if any yet. */
  function pinIconKey(pin: PinnedApp): IconKey | null {
    return store.dockItem(pin.id)?.iconKey ?? null;
  }

  /** A data URL, `null` when there is no icon (yet or at all), `undefined` while it loads. */
  function pinIcon(pin: PinnedApp): string | null | undefined {
    const key = pinIconKey(pin);
    return key === null ? null : store.icon(key, pinIconSize);
  }

  // Request the icon of every pin whose identity the dock has resolved; the
  // cache makes repeats free, so this may run on every change of the list,
  // the dock state or the display scale.
  $effect(() => {
    for (const pin of settings.pinned) {
      const key = pinIconKey(pin);
      if (key !== null) {
        store.ensureIcon(key, pinIconSize);
      }
    }
  });

  /** Where the entry launches from. */
  function launchSummary(pin: PinnedApp): string {
    const launch = pin.launch;
    switch (launch.kind) {
      case 'exe':
        return launch.args.length > 0 ? `${launch.path} ${launch.args.join(' ')}` : launch.path;
      case 'shortcut':
        return `Shortcut · ${launch.path}`;
      case 'storeApp':
        return `Store app · ${launch.appUserModelId}`;
      case 'uri':
        return `URI · ${launch.uri}`;
    }
  }

  function movePinned(index: number, delta: number): void {
    const ids = settings.pinned.map((p) => p.id);
    const order = moveItem(ids, index, index + delta);
    void store.reorderPinned(order);
  }

  function removePinned(pin: PinnedApp): void {
    void removeRow('.pin', 'button[aria-label^="Unpin "]', 'h-pinned', pin.id, () =>
      store.unpinApp(pin.id),
    );
  }
</script>

<SettingsCard note="Drag apps onto the dock to pin them.">
  {#if settings.pinned.length === 0}
    <p class="empty">Nothing is pinned yet.</p>
  {:else}
    <ul class="pins" aria-label="Pinned apps">
      {#each settings.pinned as pin, index (pin.id)}
        {@const icon = pinIcon(pin)}
        <li
          class="pin"
          data-id={pin.id}
          data-icon={icon === undefined ? 'loading' : icon === null ? 'none' : 'loaded'}
        >
          <!-- Decorative: the name is the text beside it. A neutral
               tile stands in until the dock has resolved the app
               (and stays when it has no icon); never a broken image. -->
          <span class="pin-icon" aria-hidden="true">
            {#if icon}
              <img src={icon} alt="" draggable="false" />
            {:else}
              <span class="placeholder"></span>
            {/if}
          </span>
          <div class="pin-text">
            <span class="pin-name">{pinnedName(pin)}</span>
            <span class="pin-launch">{launchSummary(pin)}</span>
          </div>
          <div class="pin-actions">
            <IconButton
              icon="chevron-up"
              label="Move {pinnedName(pin)} up"
              disabled={index === 0}
              onclick={() => movePinned(index, -1)}
            />
            <IconButton
              icon="chevron-down"
              label="Move {pinnedName(pin)} down"
              disabled={index === settings.pinned.length - 1}
              onclick={() => movePinned(index, 1)}
            />
            <IconButton
              icon="close"
              label="Unpin {pinnedName(pin)}"
              onclick={() => removePinned(pin)}
            />
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</SettingsCard>

<style>
  /* Pinned apps list */
  .pins {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .pin {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 14px;
    min-height: 56px;
    box-sizing: border-box;
    padding: 8px 12px 8px 16px;
  }

  .pin + .pin {
    border-top: 1px solid var(--divider);
  }

  .pin-actions {
    display: flex;
    gap: 2px;
  }
</style>
