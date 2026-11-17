/** Dropped paths for the Recycle Bin (mirror of crates/stem-platform/src/rules/recycle.rs). */

/** Why `rules::recycle::plan_recycle` leaves a dropped path out (`SkipReason`). */
export type RecycleSkip = 'notAFilePath' | 'root' | 'missing' | 'duplicate' | 'insideDropped';

const RECYCLE_SKIP_TEXT: Record<RecycleSkip, string> = {
  notAFilePath: 'is not an absolute file path',
  root: 'is a drive or share root',
  missing: 'does not exist',
  duplicate: 'is listed twice',
  insideDropped: 'is inside another dropped folder',
};

/** `RecyclePlan`: what a drop on the Recycle Bin asks the shell to do. */
export interface RecyclePlan {
  /** In drop order, in the shell's form (`\` separators, none doubled or trailing). */
  recycle: string[];
  /** In drop order, as dropped. */
  skipped: Array<{ path: string; reason: RecycleSkip }>;
}

/** `shell_path`: a plain absolute file path in the shell's form, or why it is not one. */
function shellPath(raw: string): { path: string } | { skip: RecycleSkip } {
  if (raw === '' || /[\0*?]/.test(raw)) {
    return { skip: 'notAFilePath' };
  }
  const path = raw.replace(/\//g, '\\');
  const plain = (c: string): boolean => c !== '.' && c !== '..' && !c.includes(':');
  let head: string;
  let tail: string[];
  if (path.startsWith('\\\\')) {
    const parts = path.slice(2).split('\\');
    const server = parts[0] ?? '';
    const share = parts[1] ?? '';
    if (server === '' || !plain(server)) {
      return { skip: 'notAFilePath' };
    }
    if (share === '') {
      return { skip: 'root' };
    }
    if (!plain(share)) {
      return { skip: 'notAFilePath' };
    }
    head = `\\\\${server}\\${share}`;
    tail = parts.slice(2);
  } else if (/^[A-Za-z]:\\/.test(path)) {
    head = path.slice(0, 2);
    tail = path.slice(3).split('\\');
  } else {
    return { skip: 'notAFilePath' };
  }
  const components = tail.filter((c) => c !== '');
  if (components.length === 0) {
    return { skip: 'root' };
  }
  if (!components.every(plain)) {
    return { skip: 'notAFilePath' };
  }
  return { path: `${head}\\${components.join('\\')}` };
}

/**
 * `rules::recycle::plan_recycle`: plain absolute paths only, never a root,
 * only what `exists`, each once, nothing inside another dropped folder.
 */
export function recyclePlan(
  paths: readonly string[],
  exists: (path: string) => boolean = () => true,
): RecyclePlan {
  const kept: Array<{ index: number; path: string; key: string }> = [];
  const keys = new Set<string>();
  const skipped: Array<{ index: number; reason: RecycleSkip }> = [];
  paths.forEach((raw, index) => {
    const shell = shellPath(raw);
    if ('skip' in shell) {
      skipped.push({ index, reason: shell.skip });
      return;
    }
    if (!exists(shell.path)) {
      skipped.push({ index, reason: 'missing' });
      return;
    }
    const key = shell.path.toLowerCase();
    if (keys.has(key)) {
      skipped.push({ index, reason: 'duplicate' });
      return;
    }
    keys.add(key);
    kept.push({ index, path: shell.path, key });
  });
  const recycle: string[] = [];
  for (const { index, path, key } of kept) {
    let inside = false;
    for (let at = key.indexOf('\\', 1); at !== -1 && !inside; at = key.indexOf('\\', at + 1)) {
      inside = keys.has(key.slice(0, at));
    }
    if (inside) {
      skipped.push({ index, reason: 'insideDropped' });
    } else {
      recycle.push(path);
    }
  }
  skipped.sort((a, b) => a.index - b.index);
  return {
    recycle,
    skipped: skipped.map(({ index, reason }) => ({ path: paths[index]!, reason })),
  };
}

/** `nothing_to_recycle`: the error of a plan that recycles nothing. */
export function nothingToRecycle(plan: RecyclePlan): string {
  const { skipped } = plan;
  if (skipped.length === 0) {
    return 'no paths to move to the Recycle Bin';
  }
  if (skipped.length === 1) {
    return `${skipped[0]!.path} ${RECYCLE_SKIP_TEXT[skipped[0]!.reason]}`;
  }
  return skipped.every((s) => s.reason === 'missing')
    ? `none of the ${skipped.length} paths exists`
    : `none of the ${skipped.length} paths can be moved to the Recycle Bin`;
}
