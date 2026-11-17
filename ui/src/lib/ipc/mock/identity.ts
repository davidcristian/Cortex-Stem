/**
 * App identities as the mock platform resolves them (mock/identity.rs), and the
 * display names `rules::identity` and stem-service fall back to.
 */

import type { AppId } from '$lib/ipc/generated/AppId';
import type { AppIdentity } from '$lib/ipc/generated/AppIdentity';
import type { LaunchTarget } from '$lib/ipc/generated/LaunchTarget';
import type { RawWindow } from '$lib/ipc/generated/RawWindow';

import { appIdFor, appIdForAppUserModelId, appIdForExe, fileStem } from './model';

/**
 * `identity_for` in mock/identity.rs: a window whose AppUserModelID the
 * shell knows (`appsFolderKnows`) is that app (`identityForAumid`), so
 * Calculator in an `ApplicationFrameHost` frame launches the Store app and
 * names no file. Any other window, a desktop program with an
 * AppUserModelID of its own included (its id stays `aumid:`), is named
 * after its exe's stem, else its title, and launches that exe.
 */
export function identityForWindow(raw: RawWindow): AppIdentity {
  const aumid = raw.appUserModelId?.trim();
  if (aumid !== undefined && appsFolderKnows(aumid)) {
    return identityForAumid(aumid);
  }
  const id = appIdFor(raw);
  return {
    id,
    displayName: raw.exePath !== null ? fileStem(raw.exePath) : raw.title,
    iconKey: `mock:${id}`,
    launch:
      raw.exePath !== null ? { kind: 'exe', path: raw.exePath, args: [], workingDir: null } : null,
  };
}

/**
 * `identity_for_aumid` in mock/identity.rs: the `aumid:` id, a `storeApp`
 * launch by the AppUserModelID and no file. With no AppsFolder to name the
 * app, its name is the real platform's fallback (`aumidFallbackName`), its
 * icon key the mock's (`mock:<id>`).
 */
function identityForAumid(appUserModelId: string): AppIdentity {
  const aumid = appUserModelId.trim();
  const id = appIdForAppUserModelId(aumid);
  return {
    id,
    displayName: aumidFallbackName(aumid),
    iconKey: `mock:${id}`,
    launch: { kind: 'storeApp', appUserModelId: aumid },
  };
}

/**
 * `apps_folder_knows` in mock/identity.rs: the mock's shell knows exactly
 * the packaged form of an AppUserModelID, `<package family name>!<application
 * id>` with the family `<name>_<publisher id>`
 * (`Microsoft.WindowsCalculator_8wekyb3d8bbwe!App`), and no desktop
 * program's own (`308046B0AF4A39CB`, `Chrome.Profile1`).
 */
export function appsFolderKnows(aumid: string): boolean {
  const trimmed = aumid.trim();
  const bang = trimmed.indexOf('!');
  if (bang < 0) {
    return false;
  }
  const family = trimmed.slice(0, bang);
  const app = trimmed.slice(bang + 1);
  const underscore = family.indexOf('_');
  if (underscore < 0) {
    return false;
  }
  const name = family.slice(0, underscore);
  const publisher = family.slice(underscore + 1);
  return name.trim() !== '' && /^[A-Za-z0-9]+$/.test(publisher) && app.trim() !== '';
}

/** `rules::identity::PLACEHOLDER_DISPLAY_NAME`. */
const PLACEHOLDER_DISPLAY_NAME = 'Application';

/** `rules::identity::clean_display_name`: trimmed, every trailing `.exe` removed, non-empty. */
function cleanDisplayName(name: string): string | null {
  let cleaned = name.trim();
  while (cleaned.length >= 4 && cleaned.slice(-4).toLowerCase() === '.exe') {
    cleaned = cleaned.slice(0, -4).trimEnd();
  }
  return cleaned === '' ? null : cleaned;
}

/**
 * `rules::identity::aumid_fallback_name`: the name the real platform gives
 * an AppUserModelID the shell cannot name
 * (`Microsoft.WindowsCalculator_8wekyb3d8bbwe!App` → `WindowsCalculator`).
 */
export function aumidFallbackName(aumid: string): string {
  const cleaned = cleanDisplayName(aumid);
  if (cleaned === null) {
    return PLACEHOLDER_DISPLAY_NAME;
  }
  const family = cleaned.split('!')[0] ?? cleaned;
  const name = family.split('_')[0] ?? family;
  const last = name.split('.').pop() ?? name;
  const chosen = last === '' ? cleaned : last;
  return cleanDisplayName(chosen) ?? cleanDisplayName(cleaned) ?? PLACEHOLDER_DISPLAY_NAME;
}

/**
 * `identify_target` in mock/platform.rs: a `storeApp` target is the same
 * identity as the app's windows.
 */
export function identityForTarget(target: LaunchTarget): AppIdentity {
  switch (target.kind) {
    case 'exe':
    case 'shortcut': {
      const id = appIdForExe(target.path);
      return { id, displayName: fileStem(target.path), iconKey: `mock:${id}`, launch: target };
    }
    case 'storeApp':
      return identityForAumid(target.appUserModelId);
    case 'uri': {
      const id = `uri:${target.uri}`;
      return { id, displayName: target.uri, iconKey: `mock:${id}`, launch: target };
    }
  }
}

/** `fallback_display_name` in stem-service. */
export function fallbackDisplayName(appId: AppId): string {
  const parts = appId.split(/[/\\:]/);
  let tail = parts[parts.length - 1] ?? appId;
  if (tail.endsWith('.exe')) {
    tail = tail.slice(0, -'.exe'.length);
  }
  return tail === '' ? appId : tail;
}
