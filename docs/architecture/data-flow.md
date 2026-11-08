# Data flow

This file describes how events from Windows reach the UI, how a click reaches Windows, and
which facts about the screen the engine publishes on the way.

```
OS ──WinEvents/shell hook──▶ events thread ──PlatformEvent──▶ channel
                                                               │
     DockService worker thread ◀───────────────────────────────┘
       engine.apply(input) → EngineOutcome{changed, unresolved_apps}
       ├─ unresolved apps → platform.identify_*() → IdentityResolved → channel
       └─ changed → listeners → Tauri emit("dock://state", snapshot) → UI
UI click ──invoke("primary_click")──▶ service.primary_click
       engine.primary_action(key)
         → Launch | ActivateWindow | MinimizeWindow | Shell | None
       → platform.launch / window_action / shell_action
```

- **The engine is pure.** It never calls the OS. It turns inputs into state
  and returns what a click does. Ordering, grouping, separators, cloaked
  windows, pinned and running items, and cycling through an app's windows
  are all decided in the engine and unit-tested there.
- **Identity resolution is lazy.** The engine reports the app ids it has
  not seen before. The service requests each one's name, icon and launch
  target from the platform off the UI thread, and passes the result back
  to the engine. Until then the item shows the window title.
- **Snapshots, not diffs.** `DockState` is small (tens of items). Sending
  it whole avoids the bugs of keeping a second copy in step through diffs.
  The service coalesces the emits to at most one per 16 ms.
- **Facts about the screen reach the UI the same way.** The events thread
  decides whether a fullscreen app covers the screen, because it has the
  foreground `HWND`, its frame and its monitor. The decision is
  `windows::fullscreen` over the pure `rules::fullscreen`. The shell's
  state (`SHQueryUserNotificationState`) counts first. Otherwise a window
  counts when its frame covers its monitor's full bounds and it is not
  minimised, maximised, a shell surface or a window of Stem's own process.
  The thread evaluates this once at start and again 150 ms after a
  foreground change, a display change or a move of the foreground window
  (an `EVENT_OBJECT_LOCATIONCHANGE` hook filtered to that window). It
  emits `PlatformEvent::FullscreenChanged` once
  at start and then only when the result changes. The engine stores the result and
  publishes it as `DockState.fullscreen`, and the UI applies it. There is no separate
  event for it, and nothing polls.
