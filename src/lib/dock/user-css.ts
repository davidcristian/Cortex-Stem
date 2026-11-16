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
import type { UserCssProblem } from '$lib/ipc/generated/UserCssProblem';

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
 * What the user last did in the settings page's dock.css row, which the
 * status reads besides the last read: `'none'` nothing yet, the last read
 * is the one at start; `'edit'` *Edit dock.css* ran since the last read
 * (`open_user_css` creates the file from the template when there is none,
 * so the file is there once it has succeeded, but the last read predates
 * it); `'reload'` the last read was a *Reload*, after an *Edit* or not,
 * which reads like the one at start.
 */
export type UserCssAction = 'none' | 'edit' | 'reload';

/** Why the dock does not use the file, from `UserCss.problem` (IPC v1.29). */
const NOT_IN_USE: Record<UserCssProblem, string> = {
  notFound: 'Not in use: there is no dock.css yet. Edit dock.css creates one from a template',
  unreadable: 'Not in use: Stem cannot read the file (the log says why)',
  notUtf8: 'Not in use: the file is not UTF-8 text. Save it as UTF-8, then press Reload',
  tooLarge: 'Not in use: the file is larger than 64 KiB, the most Stem reads',
};

/**
 * What the settings page says about the file under its path: whether the
 * dock uses it, and if not, why:
 *
 * - a path in parentheses: a run that saves no files has no `dock.css`;
 * - after an *Edit*, before a *Reload*: the file is there but not read yet;
 * - otherwise what the last read found (`UserCss.problem`): no file, the
 *   normal case, says what *Edit* does; a file Stem cannot use says why
 *   (the log has the details). A payload without a problem (an older
 *   build's) reads as no file.
 */
export function userCssStatus(payload: UserCss | null, last: UserCssAction = 'none'): string {
  if (payload === null) {
    return 'Not read yet';
  }
  if (payload.css !== null) {
    return 'In use since the last start or Reload';
  }
  if (payload.path.startsWith('(')) {
    return 'Not in use: this run saves no files, so it has no dock.css';
  }
  if (last === 'edit') {
    return 'Not in use yet: save the file, then press Reload';
  }
  // `problem` goes with `css: null`; it is missing only from an older build's payload.
  return NOT_IN_USE[payload.problem ?? 'notFound'];
}
