# Toggle while hidden

The tray's left click and the toggle shortcut (`behavior.toggleShortcut`) both run
`windows::toggle_dock`, which hides the dock *window* when it is visible and shows it otherwise.
With auto-hide (ADR-0008), a visible window often has its panel slid out. Hiding that window
would be a press with no visible effect: only the next press, which shows the window again,
would bring the panel back (through the page's `visibilitychange`). One press must bring the dock
back whatever hid it. Rust makes the decision, and it needs one fact from the UI:

- `set_dock_auto_hidden({ hidden })`: auto-hide has slid the calling dock window's panel out
  (`true`: the auto-hide state machine's phase became `hidden`), or back (`false`). The UI sends
  it on every change of that fact, from the code that applies the phase, never twice in a row
  with the same value. Rust stores it per dock window (`windows::DockAutoHidden`; `false` by
  default and whenever the window is created). Only dock windows may call it; a failure is
  logged, never shown.
- `toggle_dock` then does one of three things:
  - window hidden: it shows the window, and the page's `visibilitychange` brings a slid-out panel
    back;
  - window visible and the flag set to hidden: it emits `dock://show` to that window and leaves
    the window alone (`dock reveal requested` in the log, at debug level);
  - window visible and the panel shown: it hides the window.
- `dock://show` (payload `null`, to the one dock window): the UI brings the panel back exactly as
  the `visibilitychange` path does (`show` to the auto-hide state machine). The dock stays up
  until the pointer has been over it and left, and the hit region is reported before the slide
  starts.
- The UI reports after the fact, so a press that arrives while the panel is sliding out may still
  hide the window. The next press shows it again, and the panel comes back with it. That means at
  most one extra press, and no round trip is spent on preventing it.
- Failure notices (toasts) are never suppressed by auto-hide. While the panel is slid out, they
  are shown just inside the window's edge side (just above its bottom edge on `bottom`,
  [Sizing protocol](sizing-protocol.md)) and move back out with the panel.
- `MockBackend` records the call (`calls` entry `setDockAutoHidden`), and `requestShow()` emits
  `dock://show` as `toggle_dock` would.
