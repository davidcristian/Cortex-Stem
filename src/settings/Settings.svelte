<script lang="ts">
  import { onMount } from 'svelte';

  import { watchDevicePixelRatio } from '$lib/dock/dpr';
  import { moveItem } from '$lib/dock/reorder';
  import { accentColor, resolveTheme } from '$lib/dock/theme';
  import type { IconKey } from '$lib/ipc/generated/IconKey';
  import type { PinnedApp } from '$lib/ipc/generated/PinnedApp';
  import type { Settings } from '$lib/ipc/generated/Settings';
  import type { SettingsStore } from '$lib/stores/settings.svelte';

  import IconButton from './IconButton.svelte';
  import Select from './Select.svelte';
  import SettingRow from './SettingRow.svelte';
  import SettingsCard from './SettingsCard.svelte';
  import Slider from './Slider.svelte';
  import TextField from './TextField.svelte';
  import Toggle from './Toggle.svelte';

  /** CSS px of a pinned app's icon in the list. */
  const PIN_ICON_PX = 24;

  interface Props {
    /** Created once by the page entry; it owns the backend subscription. */
    store: SettingsStore;
  }

  let { store }: Props = $props();

  // --- Environment facts ----------------------------------------------------
  let prefersDark = $state(false);
  /** Device pixels per CSS px; the pin icons are requested at this × 24 px. */
  let dpr = $state(1);

  // --- Derived ----------------------------------------------------------------
  const settings = $derived(store.settings);
  const theme = $derived(resolveTheme(settings?.theme ?? 'system', store.theme, prefersDark));
  const accent = $derived(accentColor(store.theme));
  const highContrast = $derived(store.theme?.highContrast ?? false);

  // The page follows `data-theme` and `data-high-contrast` like the dock does.
  $effect(() => {
    const root = document.documentElement;
    root.dataset.theme = theme;
    if (highContrast) {
      root.dataset.highContrast = '';
    } else {
      delete root.dataset.highContrast;
    }
  });

  const edit = (mutate: (draft: Settings) => void): void => store.edit(mutate);

  // --- Formatting -------------------------------------------------------------
  const px = (v: number): string => `${Math.round(v)} px`;
  const percent = (v: number): string => `${Math.round(v * 100)} %`;
  const scale = (v: number): string => `${v.toFixed(2)}×`;
  const icons = (v: number): string => (v === 1 ? '1 icon' : `${v} icons`);
  const millis = (v: number): string => `${Math.round(v)} ms`;

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

  /** Device pixels of a pin icon on this display. */
  const pinIconSize = $derived(Math.max(1, Math.round(PIN_ICON_PX * dpr)));

  /** A data URL, `null` when there is no icon (yet or at all), `undefined` while it loads. */
  function pinIcon(pin: PinnedApp): string | null | undefined {
    const key = pinIconKey(pin);
    return key === null ? null : store.icon(key, pinIconSize);
  }

  // Request the icon of every pin whose identity the dock has resolved; the
  // cache makes repeats free, so this may run on every change of the pins,
  // the dock state or the display scale.
  $effect(() => {
    for (const pin of settings?.pinned ?? []) {
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
    // The scale changes when the window moves to a monitor with another DPI
    // or the display scale is changed (no resize event need follow); the pin
    // icons are requested again at the new size.
    const stopScale = watchDevicePixelRatio(window, (ratio) => {
      dpr = ratio;
    });
    void store.start();
    return () => {
      dark.removeEventListener('change', onDark);
      stopScale();
      store.stop();
    };
  });
</script>

<!-- A native-looking window has no browser context menu (Back, Reload, Inspect…). -->
<svelte:window oncontextmenu={(e) => e.preventDefault()} />

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
          <!-- Only the built-in blur is implemented, not the system
               materials. The other two stay as options so a settings
               file that names one still shows its value, but cannot be chosen. -->
          <SettingRow
            id="backdrop"
            label="Backdrop"
            description="How the panel blends with what is behind it. Acrylic and Mica are not available yet."
          >
            <Select
              id="backdrop"
              value={settings.appearance.backdrop}
              options={[
                { value: 'css', label: 'Blur (built in)' },
                { value: 'acrylic', label: 'Acrylic (not yet available)', disabled: true },
                { value: 'mica', label: 'Mica (not yet available)', disabled: true },
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
            id="click-through"
            label="Click through outside the dock"
            description="Clicks beside and above the panel reach the desktop and the windows behind it"
          >
            <Toggle
              id="click-through"
              checked={settings.behavior.clickThrough}
              onchange={(v) =>
                edit((s) => {
                  s.behavior.clickThrough = v;
                })}
            />
          </SettingRow>
          <SettingRow
            id="toggle-shortcut"
            label="Toggle the dock"
            description="Shows or hides the dock from any app. Ctrl, Alt, Shift or Super, then one key, joined by +, like Ctrl+Alt+D; empty for none."
          >
            <TextField
              id="toggle-shortcut"
              value={settings.behavior.toggleShortcut}
              placeholder="None"
              describedby="toggle-shortcut-description"
              oncommit={(v) =>
                edit((s) => {
                  s.behavior.toggleShortcut = v;
                })}
            />
          </SettingRow>
          <!-- ADR-0008: the hidden dock leaves its window
               behind, which takes clicks unless click-through is on. -->
          <SettingRow
            id="auto-hide"
            label="Auto-hide"
            description="Always: slides the dock away whenever the pointer leaves it. When a window overlaps: only while a window covers the dock, and back as soon as none does. Touch the bottom edge below it to bring it back. Works best with click-through on: with it off, the space the hidden dock leaves still takes clicks."
          >
            <Select
              id="auto-hide"
              value={settings.autoHide.mode}
              options={[
                { value: 'off', label: 'Off' },
                { value: 'always', label: 'Always' },
                { value: 'whenOverlapped', label: 'When a window overlaps' },
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
            description="Wait this long after the pointer leaves, or after a window starts to overlap the dock, before hiding"
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
          <SettingRow
            id="item-task-view"
            label="Task View"
            description="Shows your open windows and desktops"
          >
            <Toggle
              id="item-task-view"
              checked={settings.items.taskView}
              onchange={(v) =>
                edit((s) => {
                  s.items.taskView = v;
                })}
            />
          </SettingRow>
          <SettingRow
            id="item-show-desktop"
            label="Show Desktop"
            description="Minimises every window; a second click brings them back"
          >
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
          <SettingRow
            id="autostart"
            label="Start Stem when you sign in"
            description="Adds Stem to the apps Windows starts for you"
          >
            <Toggle
              id="autostart"
              checked={settings.system.autostart}
              onchange={(v) =>
                edit((s) => {
                  s.system.autostart = v;
                })}
            />
          </SettingRow>
          <SettingRow
            id="hide-taskbar"
            label="Hide the Windows taskbar"
            description="Windows hides it automatically while Stem runs; put back when Stem quits"
          >
            <Toggle
              id="hide-taskbar"
              checked={settings.system.hideWindowsTaskbar}
              onchange={(v) =>
                edit((s) => {
                  s.system.hideWindowsTaskbar = v;
                })}
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

      <!-- Where this installation keeps its files, for when something goes
           wrong. Rust names the files; the buttons
           ask it to open their folders in Explorer. -->
      <section aria-labelledby="h-about">
        <h2 id="h-about">About</h2>
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
    --error-text: #ff99a4;
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
    --error-text: #c42b1c;
    --chevron: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 12 12'%3E%3Cpath d='M2 4.5 6 8.5 10 4.5' fill='none' stroke='%231b1b1b' stroke-width='1.3' stroke-linecap='round' stroke-linejoin='round'/%3E%3C/svg%3E");
  }

  /* Windows contrast themes (`data-high-contrast`): solid black or
     white surfaces, every line, outline and secondary text in the text
     colour. Under forced colours the browser swaps in the theme's own
     colours on top; the switches and sliders keep their parts visible
     through their own `forced-colors` rules. */
  :global(html[data-high-contrast][data-theme='dark']) {
    --page-bg: #000000;
  }

  :global(html[data-high-contrast][data-theme='light']) {
    --page-bg: #ffffff;
  }

  :global(html[data-high-contrast]) .page {
    --card-bg: var(--page-bg);
    --card-border: var(--text);
    --divider: var(--text);
    --text-secondary: var(--text);
    --control-bg: var(--page-bg);
    --control-border: var(--text);
    --control-border-bottom: var(--text);
    --switch-border: var(--text);
    --switch-knob: var(--text);
    --track: var(--text);
    --thumb-bg: var(--page-bg);
    --scrollbar: var(--text);
    --error-text: var(--text);
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

  .pin-icon {
    display: block;
    width: 24px;
    height: 24px;
  }

  .pin-icon img,
  .pin-icon .placeholder {
    display: block;
    width: 100%;
    height: 100%;
  }

  /* Same tile as the dock draws while an icon loads (`--placeholder` from app.css). */
  .pin-icon .placeholder {
    border-radius: 22%;
    background: var(--placeholder);
  }

  :global(html[data-high-contrast]) .pin-icon .placeholder {
    outline: 1px solid var(--text);
    outline-offset: -1px;
  }

  @media (forced-colors: active) {
    .pin-icon .placeholder {
      outline: 1px solid CanvasText;
      outline-offset: -1px;
    }
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

  .empty,
  .about-pending {
    margin: 0;
    padding: 18px 16px;
    color: var(--text-secondary);
  }

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
