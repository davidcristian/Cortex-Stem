/**
 * Auto-hide (ADR-0008): when the dock slides out of view and when it
 * comes back. Pure presentation logic, like the magnification maths:
 * `Dock.svelte` reports what happened (the pointer is over the panel or the
 * reveal strip or not, a hold such as a drag began or ended, the delay timer
 * fired, the settings changed) and applies what comes back: the phase to
 * render, and whether to arm or clear its one timer.
 *
 * ```text
 *   shown ──pointer out, nothing held──▶ pending ──delayMs──▶ hidden
 *     ▲                                    │                    │
 *     └─────pointer back, or a hold────────┘                    │
 *     └──────────strip touched, or an item needs attention──────┘
 * ```
 *
 * `autoHide.mode = always` hides whenever the pointer is away; `off` never
 * does; `whenOverlapped` hides only while `DockState.overlapped`
 * says a window covers the resting panel (see below). Switching to a mode
 * that does not hide right now reveals at once. A hold (an icon lifted for
 * a drag, a native context menu open, an item bouncing for a launch the dock
 * started, a file from the shell being dragged over the dock, an item with
 * keyboard focus) keeps the dock up while it lasts but never reveals a
 * hidden one.
 * Once hidden, the reveal strip brings the dock back, and so does the dock
 * window being shown on request (ADR-0008: the tray and the toggle shortcut
 * still show it).
 * The one hold that also reveals is `attention`: an item whose app
 * flashed its taskbar button brings a hidden dock back and keeps it up until
 * no item needs attention any more (the window was activated), like the
 * stock taskbar on auto-hide.
 *
 * Fullscreen: while `DockState.fullscreen` says a fullscreen app
 * owns the screen (a game, a browser after F11, a video, a slideshow), the
 * machine behaves as if the mode were `always` with no delay, whatever the
 * mode is: the `fullscreen` event hides at once with the pointer away; with
 * the pointer over the dock it stays until the pointer leaves, and then goes
 * through `pending` with a 0 ms timer (so a strip-leave and a panel-enter in
 * one dispatch cannot hide it). The strip, `show` and the holds work as
 * usual. When fullscreen ends, a mode other than `always` shows the dock
 * again at once; `always` takes over through its normal delay, and a dock
 * that is hidden stays hidden. `enabled` keeps meaning "the mode is
 * `always`"; `autoHideActive` is what the UI reads for the strip and the
 * hit region.
 *
 * Fullscreen above a visible stock taskbar: the dock window sits on
 * its edge of the work area (ADR-0008; `Settings.position`), which
 * with the stock taskbar (or another app bar) on that edge is about 48 px
 * inside the screen's, so a strip there would be a line across the
 * fullscreen picture that flashes the dock over it whenever the pointer
 * crosses it. While only fullscreen makes the machine active
 * (`onlyFullscreenHides`) and the work area stops short of the monitor on
 * the dock's edge (`workAreaReachesEdge`), there is no strip
 * (`revealStripOn`) and the hidden dock takes no pointer at all
 * (`autoHideHitRegion`): the tray, the toggle shortcut (`show`) and
 * `attention` still bring it back. With the taskbar auto-hidden (Windows'
 * own setting, or Stem's) or on another edge, the work area reaches
 * the dock's edge and the strip works as above.
 *
 * Every edge: the panel slides out towards the dock's edge, the
 * reveal strip lies along the window's edge side (the bottom 2 px on
 * `bottom`, the leftmost 2 px on `left`, and so on), and the band between
 * the panel and that side is the strip's while the dock is on screen. The
 * machine itself knows nothing about edges; only the rectangles of
 * `autohide/region.ts` do.
 *
 * When overlapped: with the mode `whenOverlapped` (`dodges`), the
 * `overlapped` event carries `DockState.overlapped`, the engine's verdict
 * that a window of another process covers the resting panel. While it is
 * set the machine is active exactly as with `always`: the dock hides after
 * `delayMs` once the pointer is away (never at once, so a window dragged
 * past does not make it jump), the strip, `show` and the holds work as
 * usual. When it clears the dock comes back at once (a countdown is
 * dropped), unless `always` or fullscreen keeps the machine active.
 * `fullscreen` and `overlapped` are independent facts; `autoHideActive` is
 * "the mode is `always`, or fullscreen, or the mode is `whenOverlapped` and
 * a window overlaps".
 *
 * A countdown of 0 ms also goes through `pending` and a 0 ms timer, so two
 * events of one dispatch (a strip-leave followed by a panel-enter) can
 * never hide the dock between them, whatever the mode. For that to hold
 * when the pointer comes in from the strip slowly, the strip on screen fills
 * the whole margin between the panel and the window's edge side
 * (`revealStripThickness`): there is no gap between the two where the
 * pointer would count as away.
 *
 * The machine forgets the pointer (`pointerInside` back to `false`) when
 * auto-hide stops applying (the mode, the end of fullscreen or of an
 * overlap): `strip` and `show` set it without the pointer being over the
 * panel, and the strip can go from under the pointer with no `pointerleave`
 * (none comes from a removed element), so a stale `true` would leave the
 * next fullscreen or overlap unable to hide the dock. `Dock.svelte` tells
 * it where the pointer is again: the next move over the panel does, and so
 * does the panel's `:hover` when auto-hide comes on again.
 */

export * from './autohide/machine';
export * from './autohide/region';
export * from './autohide/state';
