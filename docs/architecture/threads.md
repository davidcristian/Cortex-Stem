# Threads

Stem runs its work on the threads in the table below. The rules after the table say
which work may run on which thread.

| Thread | Crate | Does |
|--------|-------|------|
| Tauri main (UI) thread | tauri | window creation, commands (which must return fast) |
| `stem-platform-events` | platform | the hidden event window, the WinEvent hooks, the shell hook and timers; posts `PlatformEvent`s (see *The events thread* below) |
| `dock-recycle-bin` | platform | queries the Recycle Bin (`SHQueryRecycleBinW`) every 5 s and emits `RecycleBinChanged` when it changes; the query walks every drive and can block for seconds, so it never runs on the events thread |
| `stem-service` worker | service | engine updates (it drains every queued input in one batch), calls the listeners (`State` coalesced to at most one per 16 ms) |
| `dock-identity` | service | platform identity queries (`identify_window`, `identify_target`), one retry after 2 s, then a fallback identity; `IdentityUnavailable` when there was nothing to query; never blocks the event drain |
| `stem-platform-hit-test` | platform | started by the first `set_dock_hit_region`: a message-only window, raw mouse input (`RIDEV_INPUTSINK`) plus a 4 Hz safety poll (a 30 Hz poll if raw input cannot be registered); `GetCursorPos` → `ScreenToClient` → `rules::hit_region` hysteresis per tracked dock window; posts pass-through transitions to the dock window, whose subclass applies them on the main thread; stopped with the platform |
| `stem-appbar` | platform | started by the first `set_dock_app_bar` that requests a band; sends every `SHAppBarMessage` of the dock windows' AppBars (see *The AppBar thread* below); stopped with the platform |
| `stem-taskbar` | platform | one per `SHAppBarMessage` for `system.hideWindowsTaskbar`; its caller waits at most 500 ms for the result ([Settings](settings.md)) |
| `stem-recycle` | platform | one per `delete_to_recycle_bin`: a COM single-threaded apartment (STA) that runs `IFileOperation` with the shell's own dialogs (their message loops run here); returns the result to the waiting caller and ends |
| `dock-empty-recycle-bin` | platform | one per `ShellAction::EmptyRecycleBin`: an STA that runs `SHEmptyRecycleBinW` with the shell's confirmation |
| `stem-recycle-bin-refresh` | service | one per `delete_to_recycle_bin`, after the platform call: one `recycle_bin()` query, queued for the worker as `RecycleBinChanged`, so the bin's icon updates at once instead of at the next 5 s poll |
| `stem-icon-cache` | service | with a cache directory: writes and evicts the disk icon cache's entries from a bounded queue (see *The icon cache thread* below) |
| WebView2 threads | WebView2 | rendering |

Rules: Win32 work that needs a message loop or a COM STA runs on a thread
that has one: the events thread, or a feature's own thread from the table
above. Commands may call `Platform` methods directly when the call is quick
and bounded (`SetForegroundWindow`, `ShellExecuteEx`), never anything that
pumps messages or waits on another process. The one command that waits on the
user is `delete_to_recycle_bin`, because the shell's dialogs stay up for as
long as the user leaves them. Its shell work runs on its own `stem-recycle`
thread, and the command waits for it on Tauri's blocking pool, so neither
the main thread nor the async runtime's workers are held.

## The events thread

`stem-platform-events` creates a hidden top-level window and runs its
message loop. The window is top-level, not message-only, because broadcasts
such as `WM_SETTINGCHANGE` and `WM_DISPLAYCHANGE` do
not reach message-only windows. The thread installs the `SetWinEventHook`
hooks and the shell hook, keeps its timers, and posts `PlatformEvent`s.

**Late-visible windows.** A create or show event, or `HSHELL_WINDOWCREATED`,
can find a window hidden or cloaked while it passes every other
eligibility rule. Such a window is queried again 250 ms, 1 s and 3 s later
on one thread timer (`rules::recheck`; at most 64 windows pending, the
oldest dropped first). Once it qualifies it is reported with the same
`WindowAdded` that a show produces.

## Event sources

The events thread listens to two sources:

- the WinEvent hooks, which are required: `start` fails without them;
- a shell hook window (`RegisterShellHookWindow`), which is optional: a
  failed registration only logs a warning.

WinEvents drive every `PlatformEvent` about windows. The shell hook adds
`HSHELL_WINDOWCREATED` as one more show-type event, `HSHELL_WINDOWACTIVATED`
and `HSHELL_RUDEAPPACTIVATED` as one more foreground report, and
`HSHELL_FLASH` as the only source of `WindowAttention`.

Both sources go through the same query, and the thread's `known` map
coalesces them. The first source to find a window eligible emits the one
`WindowAdded` (debug line `window 0x… added by <source>`). The other
source's report becomes a `WindowUpdated` with unchanged facts, which
changes nothing in the engine. A foreground switch can arrive twice with
the same id, which also changes nothing.

Without the shell hook, the dock loses only the attention cue. When a
WinEvent for one window is missed, the shell hook, the delayed re-checks or
the foreground path still report the window. `HSHELL_WINDOWDESTROYED` and
`HSHELL_REDRAW` are not translated, on purpose: `EVENT_OBJECT_DESTROY`,
`EVENT_OBJECT_HIDE` and `EVENT_OBJECT_NAMECHANGE` already cover them, and
removals are idempotent. Every received event of either source is logged
once at trace with its name, so a trace log can be counted per source. The
full table is in `windows/events.rs`, section *Sources*.

## The AppBar thread

`stem-appbar` starts with the first `set_dock_app_bar` that requests a band
([ADR-0010](../adr/0010-appbar.md); a band is the strip of screen the shell
reserves for an AppBar). It sends every `SHAppBarMessage` of the dock
windows' AppBars, in order: `ABM_NEW`; `ABM_QUERYPOS` and `ABM_SETPOS` once
a window's requests have not changed for 250 ms; `ABM_REMOVE`;
`ABM_ACTIVATE`; `ABM_WINDOWPOSCHANGED`. It handles the shell notifications
that the dock windows' subclass queues for it. It requests a band it could
not get again after 1 s, 5 s, 30 s, then every minute, and reports each
granted band through a sink. Every decision is made by
`rules::appbar::Bars`, which is pure and tested. Nothing on the main thread
waits for this thread, except the release at quit, which is bounded and
keeps dispatching sent messages while it waits.

## The icon cache thread

With a cache directory, `stem-icon-cache` scans the disk icon cache at
start: it builds the index, deletes stray temp files and evicts entries
over the bounds. It then writes entries (temp file, then rename), evicts
the least recently used ones past 4096 files or 64 MiB, and saves the
recency of hits. Its jobs come from a bounded queue of 256 jobs. When the
queue is full the job is dropped, and the caller never waits. At shutdown
(on the main thread, at `RunEvent::Exit`) it works through its queue for at
most 400 ms and drops the rest. Shutdown waits for it at most 500 ms; a
writer still stuck after that is left to finish on its own. Reads happen on
the thread that called `get_icon`.
