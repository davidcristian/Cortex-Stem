# Threads

Stem runs its work on the threads in the table below. The rules after the table say
which work may run on which thread.

| Thread | Crate | Does |
|--------|-------|------|
| Tauri main (UI) thread | tauri | window creation, commands (which must return fast) |
| `stem-platform-events` | platform | the hidden event window, the WinEvent hooks, the shell hook and timers; posts `PlatformEvent`s |
| `stem-service` worker | service | engine updates, identity resolution (an icon extraction can take milliseconds), calls the listeners |
| WebView2 threads | WebView2 | rendering |

Rules: Win32 work that needs a message loop or a COM single-threaded
apartment (STA) runs on the events thread. Commands may call `Platform`
methods directly when the call is quick and bounded (`SetForegroundWindow`,
`ShellExecuteEx`), never anything that pumps messages or waits on another
process.
