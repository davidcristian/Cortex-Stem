/** Pure helpers mirrored from stem-core (model/ids.rs): app ids and file names. */

import type { AppId } from '$lib/ipc/generated/AppId';
import type { RawWindow } from '$lib/ipc/generated/RawWindow';

/** `stem_core::normalize_path`: lower-case, forward slashes. */
export function normalizePath(path: string): string {
  return path.replace(/\\/g, '/').toLowerCase();
}

export function appIdForExe(path: string): AppId {
  return `exe:${normalizePath(path)}`;
}

export function appIdForAppUserModelId(aumid: string): AppId {
  return `aumid:${aumid.trim()}`;
}

export function appIdForWindowClass(className: string): AppId {
  return `class:${className.trim().toLowerCase()}`;
}

/** `stem_core::app_id_for`: AUMID, else exe path, else window class. */
export function appIdFor(raw: RawWindow): AppId {
  const aumid = raw.appUserModelId?.trim();
  if (aumid) {
    return appIdForAppUserModelId(aumid);
  }
  if (raw.exePath !== null) {
    return appIdForExe(raw.exePath);
  }
  return appIdForWindowClass(raw.className);
}

/** File name without extension; `/` and `\` both separate (mock/identity.rs `file_stem`). */
export function fileStem(path: string): string {
  const name = path.split(/[/\\]/).pop() ?? path;
  const dot = name.lastIndexOf('.');
  return dot <= 0 ? name : name.slice(0, dot);
}
