# ADR-0008: Auto-hide reveals through the hit region, not a new hook

Date: 2026-11-04 · Status: accepted

## Context

With `AutoHideMode::Always` the dock slides away `autoHide.delayMs` after
the pointer has left it, and comes back when the pointer touches the
bottom edge (the dock's own edge; see "Every edge" below). The question is
how the reveal is detected: a `GetCursorPos` poll on the events thread
while the dock is hidden, or a low-level mouse hook.

The platform already tracks the pointer for every dock window on the
hit-test thread (raw input with `RIDEV_INPUTSINK`, plus a 4 Hz safety
poll). It switches the window between click-through and hit-testable from
two rectangles the UI reports: the `panel` (entering it makes the window
take clicks) and the `envelope` (leaving it makes the window click-through
again). The dock window's bottom edge is the bottom edge of its monitor's
work area (on `top`, `left` and `right` the window is flush with that edge
of the work area instead).

## Decision

- **No new platform mechanism.** While hidden, the UI reports a thin reveal
  strip as both `panel` and `envelope`: the bottom 2 CSS px of the dock
  window, as wide as the resting panel (on every edge, the 2 px along the
  window's edge side, as long as the panel; see "Every edge" below). The
  hit-test thread makes the window hit-testable when the pointer enters the
  strip, as it does for the panel, and the webview's own `pointerenter` on
  the strip element starts the reveal. A low-level mouse hook and a second
  poll on the events thread are rejected
  ([Detecting the reveal](../research/auto-hide.md#detecting-the-reveal)).
- **The state machine is presentation and lives in the UI**, as a pure,
  unit-tested module beside the magnification maths
  (`src/lib/dock/autohide.ts`), like magnification itself: `shown` → (the
  pointer left the panel) → `pending` → (`delayMs` passed) → `hidden` →
  (the pointer in the strip) → `shown`. It never hides while a hold is on:
  an icon lifted for a drag, a native context menu from the dock open, an
  item bouncing for a launch the dock started, an item with keyboard focus,
  or a file dragged over the dock. An item that needs attention (its window
  flashed its taskbar button) is the one hold that also reveals a hidden
  dock, and it keeps the dock up until no item needs attention, as the
  stock taskbar does. A file dragged over
  the reveal strip also reveals the dock, because an OLE drag sends no
  pointer events.
- **Motion:** the panel slides out towards its edge by its own thickness
  plus its margin to the edge and its shadow, with `transform: translateY`
  over 200 ms (`ease-in` out, `ease-out` back in; `translateX` on `left`
  and `right`). With `prefers-reduced-motion` it fades instead. The dock
  window keeps its size and place, so no `SetWindowPos` happens on hide or
  reveal.
- **Settings:** `autoHide.mode` is `off` (the default), `always` or
  `whenOverlapped` (see "When overlapped" below). `autoHide.delayMs` is
  250 ms by default and at most 5000 ms.
- With click-through off, the whole dock window takes clicks even while
  the panel is hidden; the settings page says that auto-hide works best
  with click-through on. Neither setting changes the other.

## Consequences

- The reveal zone is the work area's edge on the dock's side (its bottom
  edge for a dock on `bottom`). That is the screen's edge only when no
  stock taskbar (or other app bar) is visible on that side; with a visible
  stock taskbar there, the strip sits just inside it. Fullscreen alone is
  the exception: then there is no strip (see "Fullscreen").
- While the dock is hidden, a click within 2 px of the work area's edge on
  the dock's side goes to the dock (the strip is hit-testable while the
  pointer is inside it). macOS has the same reveal zone.
- Touch and pen produce no hover, so they cannot reveal a hidden dock; the
  global shortcut and the tray toggle still show it.
- The reveal itself needs no IPC beyond the hit region and the content size
  the UI reports.

## Fullscreen

A fullscreen app (a game, a browser after F11, a video in fullscreen, a
slideshow) must have the screen to itself, whatever the auto-hide mode, the
way the stock taskbar hides for a "rude" window. The platform decides
what is fullscreen. The events thread checks the foreground window
(`rules::fullscreen`) 150 ms after a foreground change, a move of the
foreground window or a display change. The window is fullscreen when the
shell's `SHQueryUserNotificationState` returns `QUNS_BUSY`,
`QUNS_RUNNING_D3D_FULL_SCREEN` or `QUNS_PRESENTATION_MODE`, or when its
frame covers its monitor's full bounds and it is not minimised, not
maximised, not a shell surface and not one of Stem's windows. The thread
emits `PlatformEvent::FullscreenChanged` when the result changes. The
engine stores it per dock, as `DockFacts.fullscreen` in `DockState.docks`
(see "One dock per monitor"); there is no separate `dock://visibility`
event. A maximised window is excluded on purpose: with the stock taskbar
auto-hidden, the work area is the whole monitor, and every maximised
window would otherwise count.

In the UI the same machine takes one more event, `fullscreen`. While it is
set, the dock behaves as if the mode were `always` with no delay: it goes
out at once with the pointer away, stays up while the pointer is over it,
and goes out the moment the pointer leaves, through `pending` and a 0 ms
timer, so a strip-leave and a panel-enter in one dispatch cannot hide it.
On screen the strip fills the whole margin under the panel, so no gap
between the two counts as away on the pointer's way up. The reveal strip,
`show` and the holds work as in the other modes, so the user can still
reach the dock over a fullscreen app, and `attention` still reveals. When
fullscreen ends, a mode other than `always` shows the dock again at once;
`always` resumes its own delay, and a hidden dock stays hidden. `enabled`
means "the mode is `always`". `autoHideActive` (the mode is `always`, or
fullscreen, or the mode is `whenOverlapped` and a window overlaps) is what
the strip and the hit region follow. No setting turns this off: like the
stock taskbar's, it is always on.

Fullscreen above a visible stock taskbar gets no strip. The dock window
sits on the work area's edge, which with the stock taskbar visible (the
default) is about 48 px above the screen's bottom edge. A fullscreen app
covers the taskbar too, so a strip there would be a line across the
fullscreen picture, and every pointer crossing it would flash the dock over
the video. So there is no strip, and the hidden dock reports no clickable
area, while both of these hold:

- only fullscreen makes the machine active: the mode is not `always`
  (under `whenOverlapped` the fullscreen window is itself the overlap, so
  the overlap does not count);
- this dock's monitor (from `get_monitors` / `dock://monitors`, matched by
  the page's own monitor) has a work area that stops short of its bounds on
  the dock's edge (on `bottom`,
  `workArea.y + workArea.height < bounds.y + bounds.height`; the other
  edges are in "Every edge" below).

No clickable area means a 0 × 0 rectangle at the viewport's origin as both
`panel` and `envelope` ([`docs/ipc/hit-region.md`](../ipc/hit-region.md)
rejects only negative sizes, and a rectangle with no area contains no
point), so the window is click-through everywhere. Only `show` (the tray or the toggle shortcut,
`dock://show`) and `attention` bring the dock back; once shown, it reports
its plain region. With the taskbar auto-hidden (Windows' own setting, or
*Hide the Windows taskbar*) the work area reaches the screen's edge and the
strip works as above. The mode `always` and an overlap keep their strip on
the work area's edge, as the Consequences above describe.

The machine also clears its pointer flag (`pointerInside`) whenever
auto-hide stops applying: `strip` and `show` set it without the pointer
being over the panel, and a removed strip sends no `pointerleave`. The page
reads the panel's `:hover` again when auto-hide comes back on, so a dock
shown on request during one fullscreen still hides at the next.

## When overlapped

With `autoHide.mode = whenOverlapped` the dock hides, after
`autoHide.delayMs`, while a window on its monitor overlaps the panel where
it rests, and comes back when none does, the way macOS's dock moves out
of the way of a window dragged over it. Rust computes the fact from the
window frames the platform reports (`RawWindow.rect`) and one more input,
the resting panel.

- **The UI reports the resting panel**, not the hit region. While hidden
  the hit region is the 2 px strip, which no window could ever overlap, so
  the dock would hide and come back over and over. The resting panel (where
  it sits when shown, CSS px of the viewport) is reported through its own
  command, `set_dock_panel_rect`, whenever that rect changes (content,
  settings, the window growing for a drag), whatever the auto-hide phase.
  The app layer converts it once, with the window's position and scale
  factor, to physical pixels of the virtual desktop (the frames' space). It
  converts it again whenever it moves the window (a monitor or work-area
  change moves the window under an unchanged viewport), and hands the
  service `None` while the dock window is hidden by the tray or the toggle
  shortcut.
- **The rule is pure** (`stem_core::overlap`): a window overlaps when it is
  visible, not minimised, not cloaked (whatever `showOnlyCurrentDesktop`
  is set to: a cloaked window is not on screen), has a frame, belongs to
  another process, and its frame intersects the panel with a positive
  area. The monitor is implicit in the geometry. The engine stores the panel,
  recomputes the flag after every input that can change it (a frame,
  visibility, minimised or cloaked state, a window coming or going, a
  resync, the panel) and puts it in the snapshot as `DockFacts.overlapped`.
  A `WindowUpdated` that only moves a window bumps the revision exactly when
  the flag flips.
- **Frames stay current** through an `EVENT_OBJECT_LOCATIONCHANGE` hook for
  every top-level window. The callback keeps only
  `OBJID_WINDOW`/`CHILDID_SELF` reports about top-level windows. The loop
  collects the ones it tracks and, 150 ms after the first report of a burst,
  queries each again and emits `WindowUpdated` only when its facts changed. A
  dragged window costs the engine at most about seven updates a second, and
  the UI one `dock://state` per flip. No poll.
- **In the UI** the same machine takes `overlapped`. With the mode
  `whenOverlapped` it is active while the flag is set (`autoHideActive`,
  above). A window arriving over the dock starts the normal countdown
  (never an instant hide: a window dragged past must not make the dock
  jump); the strip, `show` and the holds work as always; and the flag
  clearing shows the dock again at once. `fullscreen` and `overlapped` are
  independent; whichever still applies keeps the dock hidden. A countdown
  of 0 ms goes through `pending` and a 0 ms timer in every mode, so a
  strip-leave and a panel-enter in one dispatch can never hide the dock.
- The settings page offers *When a window overlaps* next to *Always*; the
  delay applies to both.

## One dock per monitor

With `monitors = all` the two facts are per dock: `DockState.docks` lists,
for each monitor that has a dock window, `{ monitor, fullscreen,
overlapped }` (`DockFacts`). The app layer registers each dock window's
panel with its label and monitor (`set_dock_panel(label, monitor, rect)`),
and the engine runs the overlap rule once per panel. The fullscreen result
includes the fullscreen window's monitor (`MonitorFromWindow`), so a video
in fullscreen on one monitor hides only that monitor's dock. A result with
no monitor (the shell's state alone, a NULL foreground) goes to the primary
monitor's dock. Each page picks its own entry by the monitor its URL names
(the primary's page by the primary of the monitor list) and sends the same
two machine events; the machine has no input about monitors. A dock whose
monitor the app layer has not given the engine yet has no entry and takes
both facts as `false`.

## Toggle while hidden

The tray's left click and the toggle shortcut hide and show the dock
*windows*. With auto-hide a window is usually visible with its panel slid
out. A toggle that only hid and showed the window would then hide an empty
window on the first press, so nothing visible would happen, and only the
second press (the window shown again, the page's `visibilitychange`) would
bring the panel back.

- **One press brings the dock back, whatever hid it.** The decision stays
  in Rust (`windows::toggle_dock`), which needs one fact from the UI: the
  dock reports `set_dock_auto_hidden { hidden }` on every change of "the
  panel is slid out" (the machine's phase `hidden`), from the place that
  applies the phase, without repeats. The app keeps it per dock window
  (`windows::DockAutoHidden`); it is `false` until the UI reports and
  whenever the window is created. The primary dock window's state decides
  the action for every dock window. Primary window hidden: show every dock
  window. Primary window visible with its panel slid out: emit
  `dock://show` (no payload) to every visible dock window and leave those
  windows alone (a hidden one is shown). Primary window visible with its
  panel shown: hide every dock window. The UI handles `dock://show` with
  the same `show` event as the `visibilitychange` path, which stays for the
  case where the window was hidden. The machine has no extra state for
  this.
- **The race is accepted.** The UI reports after the fact, so a press that
  comes while the panel is sliding out can still hide the window; the next
  press shows it, and `visibilitychange` brings the panel back. That costs
  at most one extra press, and no round trip is added to prevent it.
- **Toasts follow the panel.** A failure notice that stayed in place while
  the panel is slid out would float above an empty slot. So while the phase
  is `hidden`, the notices' container moves down by the panel box and its
  bottom margin (`transform: translateY`, the same 200 ms as the slide,
  none under reduced motion). The notices then sit 6 px above the bottom
  edge and move back up with the panel. This describes a dock on `bottom`;
  "Every edge" below has the other edges. Toasts are never suppressed.

## Every edge

[Research: auto-hide](../research/auto-hide.md#every-edge) describes the
slide, the reveal strip and the fullscreen exception on each edge.
