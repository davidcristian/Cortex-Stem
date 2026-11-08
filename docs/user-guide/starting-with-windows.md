# Starting with Windows

_Start Stem when you sign in_ (Settings → [System](settings-window.md#system))
adds a `Stem` entry to your user's startup apps, in the registry key
`HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run` (in
`HKEY_LOCAL_MACHINE` instead when Stem runs as administrator). Turning the
setting off removes the entry. Stem checks the entry each time it starts. If
you disable Stem in Task Manager's _Startup apps_ while the setting is on,
Stem turns the entry back on at its next start, so use the setting instead.
