# Threads

Stem runs its work on the threads in the table below. The rules after the table say
which work may run on which thread.

| Thread | Crate | Does |
|--------|-------|------|
| Tauri main (UI) thread | tauri | window creation, commands (which must return fast) |
| `stem-platform-events` | platform | the hidden event window, the WinEvent hooks, the shell hook and timers; posts `PlatformEvent`s |
| `dock-recycle-bin` | platform | queries the Recycle Bin (`SHQueryRecycleBinW`) every 5 s and emits `RecycleBinChanged` when it changes; the query walks every drive and can block for seconds, so it never runs on the events thread |
| `stem-service` worker | service | engine updates (it drains every queued input in one batch), calls the listeners (`State` coalesced to at most one per 16 ms) |
| `dock-identity` | service | platform identity queries (`identify_window`, `identify_target`), one retry after 2 s, then a fallback identity; `IdentityUnavailable` when there was nothing to query; never blocks the event drain |
| `stem-platform-hit-test` | platform | started by the first `set_dock_hit_region`: a message-only window, raw mouse input (`RIDEV_INPUTSINK`) plus a 4 Hz safety poll (a 30 Hz poll if raw input cannot be registered); `GetCursorPos` → `ScreenToClient` → `rules::hit_region` hysteresis per tracked dock window; posts pass-through transitions to the dock window, whose subclass applies them on the main thread; stopped with the platform |
| `dock-empty-recycle-bin` | platform | one per `ShellAction::EmptyRecycleBin`: a COM single-threaded apartment (STA) that runs `SHEmptyRecycleBinW` with the shell's confirmation |
| WebView2 threads | WebView2 | rendering |

Rules: Win32 work that needs a message loop or a COM STA runs on a thread
that has one: the events thread, or a feature's own thread from the table
above. Commands may call `Platform` methods directly when the call is quick
and bounded (`SetForegroundWindow`, `ShellExecuteEx`), never anything that
pumps messages or waits on another process.
