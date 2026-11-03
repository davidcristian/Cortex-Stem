# Threads

Stem runs its work on the threads in the table below. The rules after the table say
which work may run on which thread.

| Thread | Crate | Does |
|--------|-------|------|
| Tauri main (UI) thread | tauri | window creation, commands (which must return fast) |
| `stem-platform-events` | platform | the hidden event window, the WinEvent hooks, the shell hook and timers; posts `PlatformEvent`s |
| `stem-service` worker | service | engine updates (it drains every queued input in one batch), calls the listeners (`State` coalesced to at most one per 16 ms) |
| `dock-identity` | service | platform identity queries (`identify_window`, `identify_target`), one retry after 2 s, then a fallback identity; never blocks the event drain |
| `dock-empty-recycle-bin` | platform | one per `ShellAction::EmptyRecycleBin`: a COM single-threaded apartment (STA) that runs `SHEmptyRecycleBinW` with the shell's confirmation |
| WebView2 threads | WebView2 | rendering |

Rules: Win32 work that needs a message loop or a COM STA runs on a thread
that has one: the events thread, or a feature's own thread from the table
above. Commands may call `Platform` methods directly when the call is quick
and bounded (`SetForegroundWindow`, `ShellExecuteEx`), never anything that
pumps messages or waits on another process.
