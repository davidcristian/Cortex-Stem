/**
 * The user's `dock.css` on the dock page (docs/ipc/themes.md,
 * docs/THEMING.md).
 *
 * Rust reads the file at start and on `reload_user_css`; the page asks
 * once (`get_user_css`) after subscribing to `dock://user-css`, and puts
 * the text in one `<style>` element at the end of `<head>`, after the
 * page's own styles, so the user's rules win a tie. The element's
 * `textContent` is set, never its HTML: the text is CSS only, nothing in it
 * is parsed as markup or run as script. What `url()` may load is the app's
 * content security policy's business (docs/THEMING.md).
 */

import type { UserCss } from '$lib/ipc/generated/UserCss';

/**
 * The stable variables (docs/THEMING.md): the dock's own stylesheet reads
 * each of them, with the theme preset's value as the fallback, so a value
 * the user sets anywhere above the element (`:root`, `.dock-stage`,
 * `.dock-panel`) applies. Anything else in `dock.css` is at the user's own
 * risk across versions.
 */
export const STEM_CSS_VARIABLES = [
  '--stem-panel-fill',
  '--stem-panel-blur',
  '--stem-panel-hairline',
  '--stem-panel-shadow',
  '--stem-panel-radius',
  '--stem-separator',
  '--stem-dot',
  '--stem-pill-fill',
  '--stem-pill-text',
] as const;

export type StemCssVariable = (typeof STEM_CSS_VARIABLES)[number];

/** `id` of the one `<style>` element that holds the user's CSS. */
export const USER_CSS_ELEMENT_ID = 'stem-user-css';

/** Where a `UserCss` reached the page from. */
export type UserCssSource = 'fetch' | 'event';

/** What the page applies, and what it has heard so far. */
export interface UserCssView {
  /** The CSS for the `<style>` element; `null`: no element. */
  css: string | null;
  /**
   * A `dock://user-css` has arrived. From then on an answer to the start's
   * `get_user_css` is never newer than what is applied: the page asks after
   * subscribing, so a read that came after the answer was taken is an event
   * the page gets too.
   */
  fromEvent: boolean;
}

/** Before anything arrived: no user CSS. */
export const NO_USER_CSS: UserCssView = { css: null, fromEvent: false };

/** The text worth an element: `null` for no file and for one with nothing but white space. */
export function cssOf(payload: UserCss): string | null {
  return payload.css === null || payload.css.trim() === '' ? null : payload.css;
}

/**
 * The view after `payload` arrived from `source`: an event always applies;
 * the fetched answer only while no event has arrived yet (it would be the
 * same or older).
 */
export function nextUserCss(
  view: UserCssView,
  source: UserCssSource,
  payload: UserCss,
): UserCssView {
  if (source === 'fetch' && view.fromEvent) {
    return view;
  }
  return { css: cssOf(payload), fromEvent: view.fromEvent || source === 'event' };
}

/**
 * Put `css` in the page's one user `<style>` element (`USER_CSS_ELEMENT_ID`),
 * creating it at the end of `<head>` or moving it back there if something
 * was added after it; `null` removes the element. Returns whether anything
 * changed, so the caller measures the panel again only then.
 */
export function applyUserCss(doc: Document, css: string | null): boolean {
  const existing = doc.getElementById(USER_CSS_ELEMENT_ID);
  if (css === null) {
    if (existing === null) {
      return false;
    }
    existing.remove();
    return true;
  }
  let changed = false;
  let element = existing;
  if (element === null) {
    element = doc.createElement('style');
    element.id = USER_CSS_ELEMENT_ID;
    element.dataset.source = 'dock.css';
    changed = true;
  }
  if (element.textContent !== css) {
    element.textContent = css;
    changed = true;
  }
  if (doc.head.lastElementChild !== element) {
    doc.head.append(element);
    changed = true;
  }
  return changed;
}

/**
 * What the settings page says about the file under its path: whether the
 * dock uses it, and if not, the reasons Rust may have had (its log says
 * which one).
 */
export function userCssStatus(payload: UserCss | null): string {
  if (payload === null) {
    return 'Not read yet';
  }
  if (payload.css === null) {
    return 'Not in use: the file is missing, unreadable, not UTF-8 or larger than 64 KiB (the log says which)';
  }
  return 'In use since the last start or Reload';
}
