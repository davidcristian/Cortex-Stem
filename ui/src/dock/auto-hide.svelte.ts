import { untrack } from 'svelte';

import { BOUNCE_MS } from '$lib/components/DockItem.svelte';
import {
  autoHideActive,
  autoHideStep,
  hideDelayMs,
  INITIAL_AUTO_HIDE,
  isAutoHidden,
  onDockAt,
  revealStripOn,
  type AutoHideEvent,
  type AutoHideState,
} from '$lib/dock/autohide';

import type { DockController } from './controller.svelte';
import type { DragSession } from './reorder.svelte';
import { viewportNow } from './view.svelte';

/** The driver of the auto-hide machine. */
export class AutoHideDriver {
  /** Set in the constructor; the `$derived` fields read it only later. */
  private readonly dock!: DockController;

  // --- Auto-hide (ADR-0008) ------------------------------------------------
  // The decisions are the pure machine's (src/lib/dock/autohide.ts); this is
  // its driver: it feeds it pointer, hold, timer and settings events, runs
  // its one timer, and applies a phase change to the hit region and the
  // panel's class. CSS does the slide. Nothing runs while nothing changes.
  state: AutoHideState = INITIAL_AUTO_HIDE;
  autoHideTimer: ReturnType<typeof setTimeout> | null = null;
  /** `state` for the template: set at the end of every `send`. */
  autoHideView = $state.raw<AutoHideState>(INITIAL_AUTO_HIDE);
  /**
   * Auto-hide applies (`autoHideActive`: the mode is `always`, or a
   * fullscreen app is in front, or the mode is `whenOverlapped` and
   * a window covers the dock): `data-autohide` is set.
   */
  autoHideOn = $derived(autoHideActive(this.autoHideView));
  /** The machine's phase, for the template (`data-autohide`). */
  autoHidePhase = $derived(this.autoHideView.phase);
  /** Slid out, or sliding out. */
  autoHidden = $derived(this.autoHidePhase === 'hidden');
  /**
   * The reveal strip is in the DOM (`revealStripOn`): whenever auto-hide
   * applies, except in fullscreen above a visible taskbar.
   */
  stripOn = $derived(revealStripOn(this.autoHideView, this.dock.view.atScreenEdge));
  /**
   * The resting panel's whole-pixel start and length along the dock, which
   * the reveal strip shares, and the margin between it and the window's
   * edge side, which the strip fills while the dock is on screen
   * (`revealStripThickness`).
   */
  stripBox = $state<{ start: number; length: number; gap: number } | null>(null);
  /** Native menus from this dock that have not closed yet (`show_*_menu` pending). */
  menusOpen = 0;
  bounceHoldTimer: ReturnType<typeof setTimeout> | null = null;
  bounceSeen = 0;

  needsAttention = $derived(this.dock.view.visibleItems.some((i) => i.needsAttention));
  fullscreen = $derived(this.dock.view.facts.fullscreen);
  overlapped = $derived(this.dock.view.facts.overlapped);

  constructor(dock: DockController) {
    this.dock = dock;
    $effect(() => {
      const mode = this.dock.view.settings?.autoHide.mode ?? 'off';
      const delayMs = this.dock.view.settings?.autoHide.delayMs ?? 0;
      untrack(() => this.send({ type: 'configure', mode, delayMs }));
    });
    // Only a bounce that plays holds the dock (none under reduced motion or
    // with the bounce turned off).
    $effect(() => {
      const serial = this.dock.store.bounceSerial;
      if (serial === this.bounceSeen) {
        return;
      }
      this.bounceSeen = serial;
      untrack(() => {
        if (this.dock.view.bounceEnabled) {
          this.holdForBounce();
        }
      });
    });
    // An app asking for attention brings a hidden dock back and keeps
    // it up until no item needs attention any more, whatever the motion
    // settings: the wiggle may be off, the item still has to be seen. From the
    // snapshot's items for this monitor, not the rendered list a file drop may
    // be holding.
    $effect(() => {
      const active = this.needsAttention;
      untrack(() => this.send({ type: 'hold', hold: 'attention', active }));
    });
    // A fullscreen app owns this dock's monitor (the platform decides,
    // the snapshot carries it per dock): the dock gets out of the way
    // whatever the auto-hide mode, and comes back when the app leaves the
    // foreground or fullscreen.
    $effect(() => {
      const active = this.fullscreen;
      untrack(() => this.send({ type: 'fullscreen', active }));
    });
    // A window of another process covers this dock's resting panel
    // (the engine decides from the frames the platform reports and the panel
    // rect sent by `reportDockPanel`): with the mode `whenOverlapped` the dock
    // hides after the delay and comes back when the window is gone.
    $effect(() => {
      const active = this.overlapped;
      untrack(() => this.send({ type: 'overlapped', active }));
    });
  }

  send(event: AutoHideEvent): void {
    if (!autoHideActive(this.state) && autoHideActive(autoHideStep(this.state, event).state)) {
      // Auto-hide comes on (the mode, a fullscreen app, an overlap). The
      // machine forgot the pointer when it last went off, and a pointer
      // resting on the panel since then has said nothing: the panel's
      // `:hover` says where it is, so the dock does not hide from under it.
      const inside = this.dock.panelEl?.matches(':hover') ?? false;
      if (inside !== this.state.pointerInside) {
        this.send({ type: 'pointer', inside });
      }
    }
    const before = this.state;
    const { state, timer } = autoHideStep(before, event);
    this.state = state;
    if (timer !== 'none' && this.autoHideTimer !== null) {
      clearTimeout(this.autoHideTimer);
      this.autoHideTimer = null;
    }
    if (timer === 'arm') {
      this.autoHideTimer = setTimeout(() => {
        this.autoHideTimer = null;
        this.send({ type: 'timeout' });
      }, hideDelayMs(state));
    }
    const wasHidden = isAutoHidden(before);
    const hidden = isAutoHidden(state);
    if (hidden && !wasHidden) {
      // Out: nothing magnified or pending stays behind, and from now on the
      // window takes the pointer only in the strip.
      this.dock.hover.leaveHoverState();
      this.dock.reports.reportHitRegion();
      this.dock.measure.scheduleMeasure();
    } else if (wasHidden && !hidden) {
      // Back: the normal region goes out before the slide starts (the class
      // `slid-out` changes the DOM after this returns), so the panel never slides
      // in over a click-through window.
      this.dock.reports.reportHitRegion();
    } else if (
      autoHideActive(state) !== autoHideActive(before) ||
      revealStripOn(state, this.dock.view.atScreenEdge) !==
        revealStripOn(before, this.dock.view.atScreenEdge)
    ) {
      this.dock.reports.reportHitRegion();
    }
    if (hidden !== wasHidden) {
      // The app layer keeps the fact: a tray click or the toggle
      // shortcut then brings a slid-out panel back (`dock://show`) instead
      // of hiding the window. Housekeeping: a failure only logs.
      void this.dock.backend
        .setDockAutoHidden(hidden)
        .catch(this.dock.intents.report('set_dock_auto_hidden', false));
    }
    this.autoHideView = state;
  }

  /** The panel's own slide or fade ended (not a slot's transition bubbling up). */
  readonly onpaneltransitionend = (e: TransitionEvent): void => {
    if (e.target === e.currentTarget) {
      this.dock.measure.scheduleMeasure();
    }
  };

  readonly onstripenter = (): void => {
    this.send({ type: 'strip' });
  };

  readonly onstripleave = (): void => {
    this.send({ type: 'pointer', inside: false });
  };

  /**
   * The drag is over. No `pointerleave` may ever come (the lifted icon was
   * under the pointer, and the window shrinks back), so where the
   * pointer is is worked out from the release point: over the panel, or
   * between it and the strip, counts as inside.
   */
  endDragHold(d: DragSession): void {
    this.send({ type: 'pointer', inside: this.isOnDock(d.lastX, d.lastY) });
    this.send({ type: 'hold', hold: 'drag', active: false });
  }

  /**
   * (`x`, `y`) is over the panel, or between it and the window's edge side
   * while the reveal strip is there to report the pointer leaving
   * (`onDockAt`; not in fullscreen above a visible taskbar): where
   * the pointer counts as on the dock after a drag that produced no pointer
   * event to say so.
   */
  isOnDock(x: number, y: number): boolean {
    const rect = this.dock.panelEl?.getBoundingClientRect();
    return (
      rect !== undefined &&
      onDockAt(
        rect,
        x,
        y,
        viewportNow(),
        revealStripOn(this.state, this.dock.view.atScreenEdge),
        this.dock.view.edge,
      )
    );
  }

  menuOpened(): void {
    this.menusOpen += 1;
    if (this.menusOpen === 1) {
      this.send({ type: 'hold', hold: 'menu', active: true });
    }
  }

  readonly menuClosed = (): void => {
    this.menusOpen = Math.max(0, this.menusOpen - 1);
    if (this.menusOpen === 0) {
      this.send({ type: 'hold', hold: 'menu', active: false });
    }
  };

  /** An item bounces for a launch the dock started: stay up until it lands. */
  holdForBounce(): void {
    if (this.bounceHoldTimer !== null) {
      clearTimeout(this.bounceHoldTimer);
    }
    this.send({ type: 'hold', hold: 'bounce', active: true });
    this.bounceHoldTimer = setTimeout(() => {
      this.bounceHoldTimer = null;
      this.send({ type: 'hold', hold: 'bounce', active: false });
    }, BOUNCE_MS);
  }
}
