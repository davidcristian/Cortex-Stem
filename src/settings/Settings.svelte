<script lang="ts">
  import { onMount } from 'svelte';

  import { moveItem } from '$lib/dock/reorder';
  import { accentColor, resolveTheme } from '$lib/dock/theme';
  import type { PinnedApp } from '$lib/ipc/generated/PinnedApp';
  import type { Settings } from '$lib/ipc/generated/Settings';
  import type { SettingsStore } from '$lib/stores/settings.svelte';

  import IconButton from './IconButton.svelte';
  import Select from './Select.svelte';
  import SettingRow from './SettingRow.svelte';
  import SettingsCard from './SettingsCard.svelte';
  import Slider from './Slider.svelte';
  import Toggle from './Toggle.svelte';

  interface Props {
    /** Created once by the page entry; it owns the backend subscription. */
    store: SettingsStore;
  }

  let { store }: Props = $props();

  // --- Environment facts ----------------------------------------------------
  let prefersDark = $state(false);

  // --- Derived ----------------------------------------------------------------
  const settings = $derived(store.settings);
  const theme = $derived(resolveTheme(settings?.theme ?? 'system', store.theme, prefersDark));
  const accent = $derived(accentColor(store.theme));

  // The page follows `data-theme` like the dock does.
  $effect(() => {
    document.documentElement.dataset.theme = theme;
  });

  const edit = (mutate: (draft: Settings) => void): void => store.edit(mutate);

  // --- Formatting -------------------------------------------------------------
  const px = (v: number): string => `${Math.round(v)} px`;
  const percent = (v: number): string => `${Math.round(v * 100)} %`;
  const scale = (v: number): string => `${v.toFixed(2)}×`;
  const icons = (v: number): string => (v === 1 ? '1 icon' : `${v} icons`);
  const millis = (v: number): string => `${Math.round(v)} ms`;

  /** What the dock shows for the entry: the user's label, else the app id. */
  function pinnedName(pin: PinnedApp): string {
    return pin.label ?? pin.id;
  }

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

  // --- Pinned apps --------------------------------------------------------------
  function movePinned(index: number, delta: number): void {
    const ids = (settings?.pinned ?? []).map((p) => p.id);
    const order = moveItem(ids, index, index + delta);
    void store.reorderPinned(order);
  }

  function removePinned(pin: PinnedApp): void {
    void store.unpinApp(pin.id);
  }

  // --- Reset ----------------------------------------------------------------------
  let confirmReset = $state(false);

  async function reset(): Promise<void> {
    confirmReset = false;
    await store.reset();
  }

  // --- Lifecycle ------------------------------------------------------------------
  onMount(() => {
    const dark = window.matchMedia('(prefers-color-scheme: dark)');
    prefersDark = dark.matches;
    const onDark = (): void => {
      prefersDark = dark.matches;
    };
    dark.addEventListener('change', onDark);
    void store.start();
    return () => {
      dark.removeEventListener('change', onDark);
      store.stop();
    };
  });
</script>

<div class="page" style:--accent={accent} data-ready={settings !== null ? 'true' : undefined}>
  <header class="top">
    <h1>Settings</h1>
    <p class="subtitle">Stem</p>
  </header>

  <main class="content">
    {#if settings === null}
      <p class="loading" aria-live="polite">
        {store.status === 'error' ? `Couldn't load settings: ${store.error}` : 'Loading…'}
      </p>
    {:else}
      <section aria-labelledby="h-appearance">
        <h2 id="h-appearance">Appearance</h2>
        <SettingsCard>
          <SettingRow id="icon-size" label="Icon size" description="Size of the icons at rest">
            <Slider
              id="icon-size"
              min={24}
              max={128}
              step={1}
              value={settings.iconSize}
              format={px}
              onchange={(v) =>
                edit((s) => {
                  s.iconSize = v;
                })}
            />
          </SettingRow>
          <SettingRow
            id="magnification"
            label="Magnification"
            description="Icons grow as the pointer approaches them"
          >
            <Toggle
              id="magnification"
              checked={settings.magnification.enabled}
              onchange={(v) =>
                edit((s) => {
                  s.magnification.enabled = v;
                })}
            />
          </SettingRow>
          <SettingRow
            id="magnification-scale"
            label="Maximum scale"
            description="Size of the icon under the pointer"
            disabled={!settings.magnification.enabled}
          >
            <Slider
              id="magnification-scale"
              min={1}
              max={2.5}
              step={0.05}
              value={settings.magnification.maxScale}
              disabled={!settings.magnification.enabled}
              format={scale}
              onchange={(v) =>
                edit((s) => {
                  s.magnification.maxScale = v;
                })}
            />
          </SettingRow>
          <SettingRow
            id="magnification-range"
            label="Range"
            description="Neighbouring icons on each side that grow too"
            disabled={!settings.magnification.enabled}
          >
            <Slider
              id="magnification-range"
              min={1}
              max={6}
              step={1}
              value={settings.magnification.rangeIcons}
              disabled={!settings.magnification.enabled}
              format={icons}
              onchange={(v) =>
                edit((s) => {
                  s.magnification.rangeIcons = v;
                })}
            />
          </SettingRow>
          <SettingRow id="opacity" label="Panel opacity">
            <Slider
              id="opacity"
              min={0}
              max={1}
              step={0.01}
              value={settings.appearance.opacity}
              format={percent}
              onchange={(v) =>
                edit((s) => {
                  s.appearance.opacity = v;
                })}
            />
          </SettingRow>
          <SettingRow id="corner-radius" label="Corner radius">
            <Slider
              id="corner-radius"
              min={0}
              max={64}
              step={1}
              value={settings.appearance.cornerRadius}
              format={px}
              onchange={(v) =>
                edit((s) => {
                  s.appearance.cornerRadius = v;
                })}
            />
          </SettingRow>
          <SettingRow
            id="backdrop"
            label="Backdrop"
            description="How the panel blends with what is behind it"
          >
            <Select
              id="backdrop"
              value={settings.appearance.backdrop}
              options={[
                { value: 'css', label: 'Blur (built in)' },
                { value: 'acrylic', label: 'Acrylic (Windows 11; applies on restart)' },
                { value: 'mica', label: 'Mica (Windows 11; applies on restart)' },
              ]}
              onchange={(v) =>
                edit((s) => {
                  s.appearance.backdrop = v;
                })}
            />
          </SettingRow>
          <SettingRow id="tooltips" label="Tooltips" description="Show app names on hover">
            <Toggle
              id="tooltips"
              checked={settings.appearance.tooltips}
              onchange={(v) =>
                edit((s) => {
                  s.appearance.tooltips = v;
                })}
            />
          </SettingRow>
          <SettingRow
            id="bounce"
            label="Bounce on launch"
            description="Animate the icon while an app starts"
          >
            <Toggle
              id="bounce"
              checked={settings.appearance.bounceOnLaunch}
              onchange={(v) =>
                edit((s) => {
                  s.appearance.bounceOnLaunch = v;
                })}
            />
          </SettingRow>
          <SettingRow id="theme" label="Theme" description="Colours of the dock and this window">
            <Select
              id="theme"
              value={settings.theme}
              options={[
                { value: 'system', label: 'Use Windows setting' },
                { value: 'light', label: 'Light' },
                { value: 'dark', label: 'Dark' },
              ]}
              onchange={(v) =>
                edit((s) => {
                  s.theme = v;
                })}
            />
          </SettingRow>
        </SettingsCard>
      </section>

      <section aria-labelledby="h-behaviour">
        <h2 id="h-behaviour">Behaviour</h2>
        <SettingsCard>
          <SettingRow
            id="click-active-minimizes"
            label="Clicking the active app minimises it"
            description="Otherwise the click brings its window to the front again"
          >
            <Toggle
              id="click-active-minimizes"
              checked={settings.behavior.clickActiveMinimizes}
              onchange={(v) =>
                edit((s) => {
                  s.behavior.clickActiveMinimizes = v;
                })}
            />
          </SettingRow>
          <SettingRow
            id="current-desktop"
            label="Show only windows on the current desktop"
            description="Hide windows that live on another virtual desktop"
          >
            <Toggle
              id="current-desktop"
              checked={settings.behavior.showOnlyCurrentDesktop}
              onchange={(v) =>
                edit((s) => {
                  s.behavior.showOnlyCurrentDesktop = v;
                })}
            />
          </SettingRow>
          <SettingRow
            id="current-monitor"
            label="Show only windows on the dock's monitor"
            description="Each monitor's dock lists the windows on that monitor"
            disabled
            tag="M3"
          >
            <Toggle
              id="current-monitor"
              checked={settings.behavior.showOnlyCurrentMonitor}
              disabled
              onchange={() => {}}
            />
          </SettingRow>
          <SettingRow
            id="running-indicators"
            label="Running indicators"
            description="Dots under apps that have open windows"
          >
            <Toggle
              id="running-indicators"
              checked={settings.behavior.runningIndicators}
              onchange={(v) =>
                edit((s) => {
                  s.behavior.runningIndicators = v;
                })}
            />
          </SettingRow>
          <SettingRow
            id="auto-hide"
            label="Auto-hide"
            description="Hide the dock until the pointer touches the screen edge"
          >
            <Select
              id="auto-hide"
              value={settings.autoHide.mode}
              options={[
                { value: 'off', label: 'Off' },
                { value: 'always', label: 'Always (M2)', disabled: true },
                { value: 'whenOverlapped', label: 'When a window overlaps (M2)', disabled: true },
              ]}
              onchange={(v) =>
                edit((s) => {
                  s.autoHide.mode = v;
                })}
            />
          </SettingRow>
          <SettingRow
            id="auto-hide-delay"
            label="Auto-hide delay"
            description="Wait this long after the pointer leaves before hiding"
          >
            <Slider
              id="auto-hide-delay"
              min={0}
              max={5000}
              step={50}
              value={settings.autoHide.delayMs}
              format={millis}
              onchange={(v) =>
                edit((s) => {
                  s.autoHide.delayMs = v;
                })}
            />
          </SettingRow>
        </SettingsCard>
      </section>

      <section aria-labelledby="h-items">
        <h2 id="h-items">Items</h2>
        <SettingsCard>
          <SettingRow id="item-launcher" label="Start" description="Opens the Start menu">
            <Toggle
              id="item-launcher"
              checked={settings.items.launcher}
              onchange={(v) =>
                edit((s) => {
                  s.items.launcher = v;
                })}
            />
          </SettingRow>
          <SettingRow id="item-task-view" label="Task View">
            <Toggle
              id="item-task-view"
              checked={settings.items.taskView}
              onchange={(v) =>
                edit((s) => {
                  s.items.taskView = v;
                })}
            />
          </SettingRow>
          <SettingRow id="item-show-desktop" label="Show Desktop">
            <Toggle
              id="item-show-desktop"
              checked={settings.items.showDesktop}
              onchange={(v) =>
                edit((s) => {
                  s.items.showDesktop = v;
                })}
            />
          </SettingRow>
          <SettingRow id="item-recycle-bin" label="Recycle Bin">
            <Toggle
              id="item-recycle-bin"
              checked={settings.items.recycleBin}
              onchange={(v) =>
                edit((s) => {
                  s.items.recycleBin = v;
                })}
            />
          </SettingRow>
          <SettingRow
            id="item-separators"
            label="Separators"
            description="Thin lines between the fixed, pinned and running groups"
          >
            <Toggle
              id="item-separators"
              checked={settings.items.separators}
              onchange={(v) =>
                edit((s) => {
                  s.items.separators = v;
                })}
            />
          </SettingRow>
        </SettingsCard>
      </section>

      <section aria-labelledby="h-pinned">
        <h2 id="h-pinned">Pinned apps</h2>
        <SettingsCard note="Drag apps onto the dock to pin them (M2).">
          {#if settings.pinned.length === 0}
            <p class="empty">Nothing is pinned yet.</p>
          {:else}
            <ul class="pins" aria-label="Pinned apps">
              {#each settings.pinned as pin, index (pin.id)}
                <li class="pin">
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
      </section>

      <section aria-labelledby="h-position">
        <h2 id="h-position">Position &amp; monitors</h2>
        <SettingsCard>
          <SettingRow
            id="position"
            label="Position"
            description="Edge of the screen the dock sits on"
          >
            <Select
              id="position"
              value={settings.position}
              options={[
                { value: 'bottom', label: 'Bottom' },
                { value: 'left', label: 'Left (M4)', disabled: true },
                { value: 'right', label: 'Right (M4)', disabled: true },
                { value: 'top', label: 'Top (M4)', disabled: true },
              ]}
              onchange={(v) =>
                edit((s) => {
                  s.position = v;
                })}
            />
          </SettingRow>
          <SettingRow id="monitors" label="Monitors" description="Where the dock appears">
            <Select
              id="monitors"
              value={settings.monitors}
              options={[
                { value: 'primary', label: 'Primary monitor only' },
                { value: 'all', label: 'All monitors (M3)', disabled: true },
              ]}
              onchange={(v) =>
                edit((s) => {
                  s.monitors = v;
                })}
            />
          </SettingRow>
        </SettingsCard>
      </section>

      <section aria-labelledby="h-system">
        <h2 id="h-system">System</h2>
        <SettingsCard>
          <SettingRow id="autostart" label="Start Stem when you sign in" disabled tag="M2">
            <Toggle
              id="autostart"
              checked={settings.system.autostart}
              disabled
              onchange={() => {}}
            />
          </SettingRow>
          <SettingRow
            id="hide-taskbar"
            label="Hide the Windows taskbar"
            description="While Stem runs; restored when it exits"
            disabled
            tag="M4"
          >
            <Toggle
              id="hide-taskbar"
              checked={settings.system.hideWindowsTaskbar}
              disabled
              onchange={() => {}}
            />
          </SettingRow>
          <SettingRow
            id="reserve-space"
            label="Reserve screen space"
            description="Maximised windows stop above the dock"
            disabled
            tag="M4"
          >
            <Toggle
              id="reserve-space"
              checked={settings.system.reserveScreenSpace}
              disabled
              onchange={() => {}}
            />
          </SettingRow>
          <SettingRow
            id="file-logging"
            label="Write a log file"
            description="In the app data folder. Applies on restart."
          >
            <Toggle
              id="file-logging"
              checked={settings.system.fileLogging}
              onchange={(v) =>
                edit((s) => {
                  s.system.fileLogging = v;
                })}
            />
          </SettingRow>
        </SettingsCard>
      </section>
    {/if}
  </main>

  <footer class="bottom">
    {#if confirmReset}
      <div class="confirm" role="alertdialog" aria-labelledby="confirm-reset">
        <p id="confirm-reset">Reset all settings to their defaults? Pinned apps are removed too.</p>
        <div class="actions">
          <button type="button" class="button accent" onclick={reset}>Reset</button>
          <button type="button" class="button" onclick={() => (confirmReset = false)}>
            Cancel
          </button>
        </div>
      </div>
    {:else}
      <div class="meta">
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
        <p class="hint">
          Settings are stored in <code>%APPDATA%\dev.davidcristian.stem\settings.json</code>
        </p>
      </div>
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
</div>

<style>
  /* Opaque window: the Tauri settings window has native chrome (unlike the
     dock), so the page paints its own background edge to edge. */
  :global(html),
  :global(body) {
    background: var(--page-bg, #202020);
  }

  :global(html[data-theme='light']) {
    --page-bg: #f3f3f3;
  }

  :global(html[data-theme='dark']) {
    --page-bg: #202020;
  }

  .page {
    /* Windows 11 settings tokens, dark. */
    --card-bg: #2b2b2b;
    --card-border: rgba(255, 255, 255, 0.08);
    --divider: rgba(255, 255, 255, 0.08);
    --text-secondary: rgba(255, 255, 255, 0.78);
    --text-disabled: rgba(255, 255, 255, 0.36);
    --control-bg: rgba(255, 255, 255, 0.06);
    --control-bg-hover: rgba(255, 255, 255, 0.09);
    --control-bg-disabled: rgba(255, 255, 255, 0.04);
    --control-border: rgba(255, 255, 255, 0.07);
    --control-border-bottom: rgba(255, 255, 255, 0.16);
    --switch-bg: transparent;
    --switch-border: rgba(255, 255, 255, 0.55);
    --switch-knob: rgba(255, 255, 255, 0.79);
    --track: rgba(255, 255, 255, 0.54);
    --thumb-bg: #454545;
    --scrollbar: rgba(255, 255, 255, 0.35);
    --chevron: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 12 12'%3E%3Cpath d='M2 4.5 6 8.5 10 4.5' fill='none' stroke='%23ffffff' stroke-width='1.3' stroke-linecap='round' stroke-linejoin='round'/%3E%3C/svg%3E");

    display: flex;
    flex-direction: column;
    height: 100%;
    color: var(--text);
    background: var(--page-bg);
    font-size: 14px;
    line-height: 20px;
  }

  :global(html[data-theme='light']) .page {
    --card-bg: #fbfbfb;
    --card-border: rgba(0, 0, 0, 0.06);
    --divider: rgba(0, 0, 0, 0.06);
    --text-secondary: rgba(0, 0, 0, 0.62);
    --text-disabled: rgba(0, 0, 0, 0.36);
    --control-bg: rgba(255, 255, 255, 0.7);
    --control-bg-hover: rgba(249, 249, 249, 0.5);
    --control-bg-disabled: rgba(249, 249, 249, 0.3);
    --control-border: rgba(0, 0, 0, 0.06);
    --control-border-bottom: rgba(0, 0, 0, 0.16);
    --switch-border: rgba(0, 0, 0, 0.45);
    --switch-knob: rgba(0, 0, 0, 0.61);
    --track: rgba(0, 0, 0, 0.45);
    --thumb-bg: #ffffff;
    --scrollbar: rgba(0, 0, 0, 0.3);
    --chevron: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 12 12'%3E%3Cpath d='M2 4.5 6 8.5 10 4.5' fill='none' stroke='%231b1b1b' stroke-width='1.3' stroke-linecap='round' stroke-linejoin='round'/%3E%3C/svg%3E");
  }

  .top {
    flex: none;
    display: flex;
    align-items: baseline;
    gap: 10px;
    padding: 22px 28px 10px;
  }

  h1 {
    margin: 0;
    font-size: 26px;
    line-height: 32px;
    font-weight: 600;
  }

  .subtitle {
    margin: 0;
    color: var(--text-secondary);
    font-size: 14px;
  }

  .content {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    /* 16 px + the 12 px scrollbar gutter = the 28 px left margin. */
    padding: 0 16px 20px 28px;
    scrollbar-gutter: stable;
  }

  .content::-webkit-scrollbar {
    width: 12px;
  }

  .content::-webkit-scrollbar-thumb {
    border: 4px solid transparent;
    border-radius: 6px;
    background: var(--scrollbar) content-box;
  }

  .content::-webkit-scrollbar-thumb:hover {
    background-color: var(--text-secondary);
  }

  .loading {
    margin: 24px 0;
    color: var(--text-secondary);
  }

  section {
    margin-top: 6px;
  }

  section + section {
    margin-top: 22px;
  }

  h2 {
    margin: 0 0 8px 1px;
    font-size: 14px;
    line-height: 20px;
    font-weight: 600;
  }

  /* Pinned apps list */
  .pins {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .pin {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 16px;
    min-height: 56px;
    box-sizing: border-box;
    padding: 8px 12px 8px 16px;
  }

  .pin + .pin {
    border-top: 1px solid var(--divider);
  }

  .pin-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .pin-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pin-launch {
    overflow: hidden;
    color: var(--text-secondary);
    font-size: 12px;
    line-height: 16px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pin-actions {
    display: flex;
    gap: 2px;
  }

  .empty {
    margin: 0;
    padding: 18px 16px;
    color: var(--text-secondary);
  }

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

  .meta {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
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

  .hint {
    margin: 0;
    overflow: hidden;
    color: var(--text-secondary);
    font-size: 12px;
    line-height: 16px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  code {
    font-family: 'Cascadia Mono', 'Cascadia Code', Consolas, monospace;
    font-size: 11.5px;
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

  .actions {
    flex: none;
    display: flex;
    gap: 8px;
  }

  .button {
    box-sizing: border-box;
    height: 32px;
    padding: 0 12px;
    border: 1px solid var(--control-border);
    border-bottom-color: var(--control-border-bottom);
    border-radius: 4px;
    background: var(--control-bg);
    color: var(--text);
    font: inherit;
    font-size: 14px;
    cursor: default;
  }

  .button:hover:not(:disabled) {
    background: var(--control-bg-hover);
  }

  .button:active:not(:disabled) {
    color: var(--text-secondary);
  }

  .button:disabled {
    color: var(--text-disabled);
    background: var(--control-bg-disabled);
  }

  .button.accent {
    border-color: var(--accent);
    background: var(--accent);
    color: #fff;
  }

  .button.accent:hover:not(:disabled) {
    filter: brightness(0.92);
  }

  .button:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: 1px;
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
