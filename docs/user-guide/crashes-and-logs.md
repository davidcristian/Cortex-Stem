# Crashes and the log file

This page describes the dialog Stem shows after a crash and the log file it
writes.

## After a crash

If Stem crashes, it writes a note, `crash.json`, to
`%LOCALAPPDATA%\dev.davidcristian.stem\`, the folder that holds the log
folder. At the next start, about 2 seconds after the dock appears, a Windows
dialog says that Stem crashed the last time it ran. The dialog shows the
time, the place in the code and the error message, and offers to open the
log folder: _Yes_ opens it in File Explorer. The dialog appears once per
crash. The dock keeps working while it is open.

If Stem was ended some other way (from Task Manager, or by a power loss),
there is no dialog. The next start writes one warning to the log.

## The log file

While _Write a log file_ (Settings → [System](settings-window.md#system)) is
on (the default), Stem writes
`%LOCALAPPDATA%\dev.davidcristian.stem\logs\stem.log`. When the file grows
past 2 MiB, Stem deletes it and starts a new one, so there is only ever one
file. Timestamps are in UTC. Settings → [About](settings-window.md#about)
shows the path and opens the folder.

The `STEM_LOG` environment variable sets the amount of detail: `error`,
`warn`, `info` (the default), `debug` or `trace`. An unknown value means
`info`, and the log has a warning. To start Stem with more detail, quit it
from the tray, then run this from a Command Prompt:

```bat
set STEM_LOG=debug
"%LOCALAPPDATA%\Stem\stem.exe"
```

`trace` writes a very large amount of detail, so use it only to investigate
a specific problem. When you report a problem, attach the log of the session
in which the problem happened.
