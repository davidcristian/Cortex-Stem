/**
 * The base of `MockBackend`: its state, the order commands run in, the
 * events it sends, the one way settings change and the lazy resolution of
 * identities (stem-service's command semantics).
 */

import type { BackendEvent, BackendEvents, EventHandler, FileDragHandler } from '$lib/ipc/backend';
import type { AppId } from '$lib/ipc/generated/AppId';
import type { AppIdentity } from '$lib/ipc/generated/AppIdentity';
import type { AppInfo } from '$lib/ipc/generated/AppInfo';
import type { MonitorId } from '$lib/ipc/generated/MonitorId';
import type { Settings } from '$lib/ipc/generated/Settings';
import type { ThemeInfo } from '$lib/ipc/generated/ThemeInfo';
import type { UserCss } from '$lib/ipc/generated/UserCss';

import { demoAppInfo, type MockUserCssFile, userCssFrom } from '../app-files';
import { Engine } from '../engine';
import { fallbackDisplayName, identityForTarget, identityForWindow } from '../identity';
import type { MockCall, MockOptions } from '../options';
import { dockLabel } from '../panel';
import { DEMO_POWER, DEMO_VOLUME, NO_CLOCK_FORMAT } from '../readings';
import { DEFAULT_THEME, SCENARIOS } from '../scenarios';
import { validateSettings } from '../settings';
import { mulberry32 } from './random';

type Listeners = { [E in BackendEvent]?: Set<EventHandler<E>> };

export class MockBackendBase {
  protected readonly engine: Engine;
  protected readonly listeners: Listeners = {};
  private readonly random: () => number;
  private readonly latency: readonly [number, number];
  private readonly identityDelayMs: number;
  private readonly logging: boolean;
  protected readonly viewportHeight: number | undefined;
  protected readonly viewportWidth: number | undefined;
  /** The monitor of the dock page this backend serves (`MockOptions.monitor`); `null` for the primary's. */
  readonly pageMonitor: MonitorId | null;
  private readonly timers = new Set<ReturnType<typeof setTimeout>>();
  /** Settles when every command issued so far has run (see `command`). */
  private queue: Promise<void> = Promise.resolve();
  protected identityTimer: ReturnType<typeof setTimeout> | null = null;
  protected failNext: string | null = null;
  private readonly mediaQuery: MediaQueryList | null;
  private readonly onSchemeChange = (): void => {
    this.setTheme({ ...this.engine.theme, dark: this.mediaQuery?.matches ?? true });
  };

  /**
   * Every side effect and pin request so far, oldest first (for tests;
   * mirror of `MockPlatform::calls`).
   */
  readonly calls: MockCall[] = [];
  protected readonly fileDragHandlers = new Set<FileDragHandler>();
  /** What `getAppInfo` answers; `setAppInfo` changes it. */
  protected appInfo: AppInfo = demoAppInfo();
  /**
   * What the page last reported through `setDockAutoHidden`: the
   * app layer's `DockAutoHidden` for this page's window, which a press of
   * the focus shortcut reads (`requestFocus`).
   */
  protected autoHidden = false;
  /**
   * Whether this page's dock window has the keyboard from a press of the
   * focus shortcut: Rust's `DockFocus` session for it while it is
   * the foreground window, so the next press lets go (`requestFocus`). Set
   * by a press that gave it the focus; cleared when the page gives the
   * focus back (`releaseDockFocus`) and when another window comes to the
   * front (`pushForeground`, an activation), which the real dock sees as
   * losing the activation. A click that launches or minimises leaves it
   * set, as it leaves the real dock in front.
   */
  protected shortcutFocus = false;
  /** The user's `dock.css` "on disk" (`null`: no file); `writeUserCss` changes it. */
  protected userCssFile: MockUserCssFile;
  /** What the last read of it gave: at construction, then each `reloadUserCss`. */
  protected userCssRead: UserCss;

  constructor(options: MockOptions = {}) {
    this.userCssFile = options.userCss ?? null;
    this.userCssRead = userCssFrom(this.userCssFile);
    const scenario = SCENARIOS[options.scenario ?? 'default']();
    if (options.position !== undefined) {
      scenario.settings.position = options.position;
    }
    this.latency = options.latencyMs ?? [20, 80];
    this.identityDelayMs = options.identityDelayMs ?? 300;
    this.logging = options.log ?? true;
    this.viewportHeight = options.viewportHeight;
    this.viewportWidth = options.viewportWidth;
    this.pageMonitor = options.monitor ?? null;
    this.random = mulberry32(options.seed ?? 0x5eed);

    // In a browser the OS theme is the browser's colour scheme, so Playwright's
    // `colorScheme` emulation drives light/dark exactly like Windows would.
    this.mediaQuery =
      typeof window !== 'undefined' && typeof window.matchMedia === 'function'
        ? window.matchMedia('(prefers-color-scheme: dark)')
        : null;
    const theme: ThemeInfo = { ...DEFAULT_THEME, dark: this.mediaQuery?.matches ?? true };
    this.mediaQuery?.addEventListener('change', this.onSchemeChange);

    // `MockPlatform::start` delivers these in this order.
    // Validated as `Settings::from_json` validates the file it loads.
    this.engine = new Engine(validateSettings(scenario.settings), DEFAULT_THEME);
    this.engine.setMonitors(scenario.monitors);
    this.engine.setTheme(theme);
    this.engine.setRecycleBin(scenario.recycleBin);
    this.engine.snapshotWindows(scenario.windows);
    this.engine.setForeground(scenario.foreground);
    this.engine.setPower(options.power ?? DEMO_POWER);
    this.engine.setClockFormat(options.clockFormat ?? NO_CLOCK_FORMAT);
    this.engine.setVolume(options.volume === undefined ? DEMO_VOLUME : options.volume);

    this.scheduleIdentityResolution();
  }

  /** Stop timers and OS listeners (tests, hot reload). */
  dispose(): void {
    for (const t of this.timers) {
      clearTimeout(t);
    }
    this.timers.clear();
    if (this.identityTimer !== null) {
      clearTimeout(this.identityTimer);
      this.identityTimer = null;
    }
    this.mediaQuery?.removeEventListener('change', this.onSchemeChange);
  }

  /** The label of the dock window this backend's page is (`dock`, or `dock-<id>`). */
  get pageLabel(): string {
    return dockLabel(this.pageMonitor, this.engine.primaryMonitorId());
  }

  /**
   * The OS theme or accent changed (`ThemeChanged`): `dock://theme` when it
   * differs from the last one; the snapshot does not carry the theme, so
   * no `dock://state` (as Rust).
   */
  setTheme(theme: ThemeInfo): void {
    const changed = JSON.stringify(theme) !== JSON.stringify(this.engine.theme);
    const stateChanged = this.engine.setTheme(theme);
    if (changed) {
      this.emit('dock://theme', { ...theme });
    }
    if (stateChanged) {
      this.emitState();
    }
  }

  // --- internals

  /**
   * A command runs after its own latency and after every command issued
   * before it has run: Tauri delivers a webview's commands in order, so a
   * later report (a hit region, the auto-hide flag) never lands before an
   * earlier one, however the latencies were drawn; `calls` is in issue order.
   */
  protected command<T>(job: () => T): Promise<T> {
    const failure = this.failNext;
    this.failNext = null;
    const run =
      failure === null
        ? job
        : (): T => {
            throw failure;
          };
    const [lo, hi] = this.latency;
    const ms = lo + (hi - lo) * this.random();
    const latency =
      ms <= 0
        ? Promise.resolve()
        : new Promise<void>((resolve) => {
            const timer = setTimeout(() => {
              this.timers.delete(timer);
              resolve();
            }, ms);
            this.timers.add(timer);
          });
    // `run` may throw: commands fail with a `string` across IPC
    // (docs/ipc/) and the mock throws strings for the same reason; the
    // rejection is the caller's, the queue goes on.
    const result = Promise.all([latency, this.queue]).then(run);
    this.queue = result.then(
      () => undefined,
      () => undefined,
    );
    return result;
  }

  protected emit<E extends BackendEvent>(event: E, payload: BackendEvents[E]): void {
    const set = this.listeners[event] as Set<EventHandler<E>> | undefined;
    if (!set) {
      return;
    }
    for (const handler of set) {
      handler(payload);
    }
  }

  protected emitState(): void {
    this.emit('dock://state', this.engine.snapshot());
  }

  protected record(call: MockCall): void {
    this.calls.push(call);
  }

  protected log(message: string): void {
    if (this.logging) {
      console.info(`[mock] ${message}`);
    }
  }

  /**
   * The one way the mock's settings change, as `mutate_settings` is Rust's:
   * `next` is validated (`validateSettings`, `Settings::validate`), handed
   * to the engine, and announced: the settings event always, a state event
   * if items changed. Returns the validated document (the engine's; clone
   * it before handing it out).
   */
  protected applySettings(next: Settings): Settings {
    const settings = validateSettings(next);
    const changed = this.engine.setSettings(settings);
    this.emit('dock://settings', structuredClone(settings));
    if (changed) {
      this.emitState();
    }
    this.scheduleIdentityResolution();
    return settings;
  }

  protected scheduleIdentityResolution(): void {
    if (this.engine.unresolvedApps().length === 0 || this.identityTimer !== null) {
      return;
    }
    if (this.identityDelayMs <= 0) {
      this.resolveIdentities();
      return;
    }
    this.identityTimer = setTimeout(() => {
      this.identityTimer = null;
      this.resolveIdentities();
    }, this.identityDelayMs);
  }

  /** `DockService::resolve_identity` for every unresolved app, then one state event. */
  protected resolveIdentities(): void {
    let changed = false;
    for (const appId of this.engine.unresolvedApps()) {
      changed = this.engine.setIdentity(appId, this.resolveIdentity(appId)) || changed;
    }
    if (changed) {
      this.emitState();
    }
  }

  private resolveIdentity(appId: AppId): AppIdentity {
    const pinned = this.engine.settings.pinned.find((p) => p.id === appId);
    const raw = this.engine.firstWindowOf(appId);
    let identity: AppIdentity;
    if (raw) {
      identity = identityForWindow(raw);
    } else if (pinned) {
      identity = identityForTarget(pinned.launch);
    } else {
      identity = {
        id: appId,
        displayName: fallbackDisplayName(appId),
        iconKey: null,
        launch: null,
      };
    }
    // The engine groups by the id *it* derived; keep that as the key.
    return { ...identity, id: appId, launch: identity.launch ?? pinned?.launch ?? null };
  }
}
