# Hiding the Windows taskbar

_Hide the Windows taskbar_ (Settings → [System](settings-window.md#system))
does not remove the taskbar. It turns on Windows' own _Automatically hide the
taskbar_ option, the same one as in Settings → Personalization → Taskbar →
Taskbar behaviors. The taskbar still slides up when the pointer touches the
bottom edge of the screen beside the dock, so the tray, the clock and quick
settings stay reachable. A dock at the bottom moves down to the screen edge,
because the work area grows.

Before the first change, Stem saves whether auto-hide was already on, in
`%LOCALAPPDATA%\dev.davidcristian.stem\taskbar.json`. Turning the setting
off, or quitting Stem, puts that state back and deletes the file. If you had
auto-hide on before, it stays on.

If Stem crashes or is ended from Task Manager, the taskbar stays hidden,
because Windows keeps the option. The next time Stem starts, it reads the
file. With the setting off, Stem puts your state back at once. With the
setting on, Stem leaves the taskbar hidden and puts your state back when it
quits. If you uninstall Stem after a crash, turn the Windows option off
yourself.

Stem does not track changes you make to the Windows option while Stem runs.
When it quits, Stem puts back the state it saved.
