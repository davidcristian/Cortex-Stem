/**
 * The app's own files in a demo run: what `get_app_info` returns, and the
 * user's `dock.css` read the way src-tauri's `user_css` reads it.
 */

import type { AppInfo } from '$lib/ipc/generated/AppInfo';
import type { UserCss } from '$lib/ipc/generated/UserCss';
import type { UserCssProblem } from '$lib/ipc/generated/UserCssProblem';

/**
 * What `get_app_info` answers in a demo run: a mock version, the
 * paths a real installation has for the user `me`, and the platform name.
 * The paths are shown, never opened, in a browser.
 */
export function demoAppInfo(): AppInfo {
  return {
    version: '0.1.0-mock',
    settingsPath: 'C:\\Users\\me\\AppData\\Roaming\\dev.davidcristian.stem\\settings.json',
    logPath: 'C:\\Users\\me\\AppData\\Local\\dev.davidcristian.stem\\logs\\stem.log',
    platform: 'mock',
  };
}

/** Where a real installation of the user `me` keeps `dock.css`: next to `demoAppInfo().settingsPath`. */
export const DEMO_USER_CSS_PATH =
  'C:\\Users\\me\\AppData\\Roaming\\dev.davidcristian.stem\\dock.css';

/** The largest `dock.css` Rust reads (`user_css::MAX_BYTES`, 64 KiB). */
export const USER_CSS_MAX_BYTES = 64 * 1024;

/**
 * What `open_user_css` writes when there is no file (a stand-in: the real
 * template is `user_css::TEMPLATE` in src-tauri). Comments only, like the
 * real one, so creating it changes nothing.
 */
export const MOCK_USER_CSS_TEMPLATE =
  "/* dock.css: your own styles for Stem's dock (mock template). */\n.dock-stage {\n  /* --stem-dot: #0078d4; */\n}\n";

/**
 * The mock's `dock.css` "on disk": its text (saved as
 * UTF-8), its bytes as an editor saved them (`Uint8Array`, which need not
 * be UTF-8), a file that is there but cannot be read (`{ unreadable }`,
 * with the error the log would name), or `null` for no file.
 */
export type MockUserCssFile = string | Uint8Array | { readonly unreadable: string } | null;

/**
 * `user_css::read_file` and `report` for the mock's file: what the page
 * gets (`UserCss` without its path). No file is `notFound`; over 64 KiB
 * is `tooLarge` (checked first, as Rust checks the size before it
 * decodes); bytes that are not UTF-8 are `notUtf8`; `{ unreadable }` is
 * `unreadable`. A leading byte order mark is dropped.
 */
export function readUserCss(file: MockUserCssFile): Pick<UserCss, 'css' | 'problem'> {
  const refused = (problem: UserCssProblem): Pick<UserCss, 'css' | 'problem'> => ({
    css: null,
    problem,
  });
  if (file === null) {
    return refused('notFound');
  }
  if (!(file instanceof Uint8Array) && typeof file !== 'string') {
    return refused('unreadable');
  }
  const bytes = typeof file === 'string' ? new TextEncoder().encode(file) : file;
  if (bytes.length > USER_CSS_MAX_BYTES) {
    return refused('tooLarge');
  }
  let text: string;
  try {
    // `ignoreBOM` keeps a byte order mark in the text, so that only a
    // leading one is dropped below, as `user_css::decode` drops it.
    text = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes);
  } catch {
    return refused('notUtf8');
  }
  return { css: text.startsWith('\uFEFF') ? text.slice(1) : text, problem: null };
}

/** What a read of `file` gives the page (`readUserCss` with the demo's path). */
export function userCssFrom(file: MockUserCssFile): UserCss {
  return { path: DEMO_USER_CSS_PATH, ...readUserCss(file) };
}
