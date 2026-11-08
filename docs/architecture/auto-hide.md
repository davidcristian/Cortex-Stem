# Auto-hide

This file describes how the dock hides its panel and brings it back, and how it behaves
while a fullscreen app covers the screen.

Auto-hide ([ADR-0008](../adr/0008-auto-hide.md)) is presentation, so the UI
implements it: a pure state machine (`src/lib/dock/autohide.ts`, unit-tested)
that `Dock.svelte` drives from pointer events, holds and one timer. A hold
is a condition that keeps the dock on screen.

With `autoHide.mode = always` the panel hides `delayMs` after the pointer
left it. It never hides while an icon is lifted for a drag, a native menu
from the dock is open (the span of the `show_*_menu` promise) or an item
bounces for a launch the dock started. `whenOverlapped` behaves like `off`
until the platform reports window geometry.

The panel slides out downwards in 200 ms, by `translateY`: by its own height,
the margin under it and room for its shadow. Under reduced motion it
fades instead. The window keeps its size and place.

While the panel is hidden, the UI reports a reveal strip as both hit
rectangles: the bottom 2 CSS px of the window, as wide as the resting panel. So the
hit-test thread ([Threads](threads.md)) lets the window take the pointer only
there. The strip element's `pointerenter` reveals the panel. The region for
the shown panel (its [envelope](dock-window.md#click-through-outside-the-panel),
extended to include the strip, so a pointer resting on the edge stays
tracked) is reported before the slide starts.

The tray and the toggle shortcut also reveal the panel: a window that was
hidden reveals its panel when it is shown again (`visibilitychange`).

An item that needs attention also reveals the dock. The `attention` hold is
the one hold that reveals when it begins. It keeps the dock up until no item
needs attention any more, like the stock taskbar on auto-hide. The platform
has no auto-hide mechanism of its own.

## Fullscreen

While `DockState.fullscreen` is
set, the same machine behaves as if the mode were `always` with no delay,
whatever the mode is. `autoHideActive`, which the strip and the hit region
follow, is true when the mode is `always`, during fullscreen, or when the
mode is `whenOverlapped` and a window overlaps the panel.

With the pointer away the dock slides out at once. With the pointer over
it the dock stays, and it hides the moment the pointer leaves, through
`pending` and a 0 ms timer, so a strip-leave and a panel-enter in one
dispatch cannot hide it. The strip, the holds and `attention` work as
above.

When fullscreen ends, a mode other than `always` shows the dock again.
`always` resumes its normal delay, and a hidden dock stays hidden. The
platform makes the fullscreen decision ([Data flow](data-flow.md)). The UI
only passes the fact into the machine, next to `attention`.
