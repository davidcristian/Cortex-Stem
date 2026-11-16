import { untrack } from 'svelte';

import { applyUserCss, NO_USER_CSS, nextUserCss, type UserCssView } from '$lib/dock/user-css';

import type { DockController } from './controller.svelte';

/** The theme on `<html>` and the user's dock.css in the page. */
export class PageTheme {
  private readonly dock: DockController;

  // --- The user's dock.css (docs/ipc/themes.md) --------------------------
  // One <style> element at the end of <head> (src/lib/dock/user-css.ts),
  // replaced on `dock://user-css`. The stable variables only restyle, but
  // anything else in the file may change the panel's size, so the panel is
  // measured again after a change.
  userCss = $state.raw<UserCssView>(NO_USER_CSS);

  constructor(dock: DockController) {
    this.dock = dock;
    // --- Theme on <html> --------------------------------------------------------
    $effect(() => {
      const root = document.documentElement;
      root.dataset.theme = this.dock.view.theme;
      if (this.dock.view.highContrast) {
        root.dataset.highContrast = '';
      } else {
        delete root.dataset.highContrast;
      }
      if (this.dock.demo) {
        root.dataset.wallpaper = this.dock.view.theme;
      } else {
        delete root.dataset.wallpaper;
      }
    });
    $effect(() => {
      if (applyUserCss(document, this.userCss.css)) {
        this.dock.measure.scheduleMeasure();
      }
    });
    // Its rules may key on the preset (the stage's `data-theme`) or the
    // colour mode and a contrast theme (`<html>`'s attributes,
    // docs/THEMING.md): a change of those can move the panel without
    // resizing it (a margin), which the ResizeObserver of `PanelMeasure` does not see
    // (it sees a padding or a border change: it watches the border box).
    // The built-in presets keep the box.
    $effect(() => {
      void this.dock.view.preset;
      void this.dock.view.theme;
      void this.dock.view.highContrast;
      untrack(this.dock.measure.scheduleMeasure);
    });
  }

  /** Subscribes to the user's dock.css and asks for it; the function returned undoes both. */
  connect(): () => void {
    // The user's dock.css: subscribe, then ask, like the store's
    // startup sequence; an event that arrives first wins over the answer.
    let mounted = true;
    const offUserCss = this.dock.backend.on('dock://user-css', (css) => {
      this.userCss = nextUserCss(this.userCss, 'event', css);
    });
    this.dock.backend.getUserCss().then(
      (css) => {
        if (mounted) {
          this.userCss = nextUserCss(this.userCss, 'fetch', css);
        }
      },
      (err: unknown) => console.error('get_user_css failed:', err),
    );
    return () => {
      mounted = false;
      offUserCss();
      applyUserCss(document, null);
    };
  }
}
