<script lang="ts">
  import { onMount } from 'svelte';

  import { watchDevicePixelRatio } from '$lib/dock/dpr';
  import { accentColor, resolveTheme } from '$lib/dock/theme';
  import type { UserCssAction } from '$lib/dock/user-css';
  import type { Settings } from '$lib/ipc/generated/Settings';
  import type { UserCss } from '$lib/ipc/generated/UserCss';
  import type { SettingsStore } from '$lib/stores/settings.svelte';
  import { describeError } from '$lib/stores/toasts.svelte';

  import AboutCard from './AboutCard.svelte';
  import AppearanceCard from './AppearanceCard.svelte';
  import BehaviourCard from './BehaviourCard.svelte';
  import HiddenAppsCard from './HiddenAppsCard.svelte';
  import ItemsCard from './ItemsCard.svelte';
  import PinnedAppsCard from './PinnedAppsCard.svelte';
  import PositionCard from './PositionCard.svelte';
  import SettingsFooter from './SettingsFooter.svelte';
  import SystemCard from './SystemCard.svelte';
  import UserCssRow from './UserCssRow.svelte';
  import './settings.css';

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

  /** Device pixels of a pin icon on this display. */
  const pinIconSize = $derived(Math.max(1, Math.round(PIN_ICON_PX * dpr)));

  // --- The user's dock.css (docs/ipc/themes.md) -----------------------------------
  // Not a setting: Rust reads the file at start and on Reload, and its
  // row (`UserCssRow`) shows where it is and whether the dock uses it. A
  // failure is the row's own notice, apart from the footer's save status.
  let userCss = $state.raw<UserCss | null>(null);
  let userCssError = $state<string | null>(null);
  /** What the user last did in the row, which the status reads (`userCssStatus`). */
  let userCssLast = $state<UserCssAction>('none');

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
    let mounted = true;
    store.backend.getUserCss().then(
      (css) => {
        // A Reload that answered first is newer.
        if (mounted && userCss === null) {
          userCss = css;
        }
      },
      (err: unknown) => {
        userCssError = `Couldn't read dock.css: ${describeError(err)}`;
        console.error('get_user_css failed:', err);
      },
    );
    void store.start();
    return () => {
      mounted = false;
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
        <AppearanceCard {settings} {edit}>
          <UserCssRow {store} bind:userCss bind:userCssError bind:userCssLast />
        </AppearanceCard>
      </section>

      <section aria-labelledby="h-behaviour">
        <h2 id="h-behaviour">Behaviour</h2>
        <BehaviourCard {settings} {edit} />
      </section>

      <section aria-labelledby="h-items">
        <h2 id="h-items">Items</h2>
        <ItemsCard {settings} {edit} />
      </section>

      <section aria-labelledby="h-pinned">
        <!-- Focusable by script: where the focus goes when the last row goes. -->
        <h2 id="h-pinned" tabindex="-1">Pinned apps</h2>
        <PinnedAppsCard {store} {settings} {pinIconSize} />
      </section>

      <!-- Running apps the dock does not show. Rust adds them (Hide
           from Stem in an app's menu) and saves them; Show asks it to take
           one off the list. -->
      <section aria-labelledby="h-hidden">
        <h2 id="h-hidden" tabindex="-1">Hidden apps</h2>
        <HiddenAppsCard {store} {settings} {pinIconSize} />
      </section>

      <section aria-labelledby="h-position">
        <h2 id="h-position">Position &amp; monitors</h2>
        <PositionCard {settings} {edit} />
      </section>

      <section aria-labelledby="h-system">
        <h2 id="h-system">System</h2>
        <SystemCard {settings} {edit} />
      </section>

      <!-- Where this installation keeps its files, for when something goes
           wrong. Rust names the files; the buttons
           ask it to open their folders in Explorer. -->
      <section aria-labelledby="h-about">
        <h2 id="h-about">About</h2>
        <AboutCard {store} />
      </section>
    {/if}
  </main>

  <SettingsFooter {store} {settings} />
</div>

<style>
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

  /* A heading takes the focus only from script, when the last row of its
     list goes; the ring shows only after the keyboard. */
  h2:focus {
    outline: none;
  }

  h2:focus-visible {
    width: fit-content;
    outline: 2px solid var(--focus-ring);
    outline-offset: 2px;
    border-radius: 2px;
  }
</style>
