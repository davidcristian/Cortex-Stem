# How to run the thumbnail probe

This page says how to run the [thumbnail probe](index.md) on Windows: what
to open first, the command, and how to step through and stop it.

## Before you run it

- Use a normal terminal (PowerShell or Windows Terminal), **not** an
  elevated one. An elevated probe cannot test A8, and it prints a warning.
- Quit Stem from its tray icon. Stem registers no thumbnails, but its dock
  would list the probe's six source windows.
- Open, in this order:
  1. Task Manager (Ctrl+Shift+Esc). For an administrator account it runs
     elevated without a prompt. Leave it open and not minimised.
  2. Calculator, then minimise it.
  3. A second virtual desktop (Win+Ctrl+D) with one Notepad window on it,
     then go back to the first desktop (Win+Ctrl+Left).
- Do not hover over the stock taskbar while the probe runs. Its own previews
  are thumbnails of the same windows.
- Note the display scale (Settings → System → Display). The probe also
  prints the DPI of its window.

## Running it

```text
cargo run -p stem-platform --example thumbnail-probe --features probes
```

The first build compiles the `windows` crate with three extra features
(`Win32_Security`, `Win32_System_Console`, `Win32_System_Performance`),
which can take a few minutes. To keep a copy of everything the probe
prints, append `2>&1 | Tee-Object -FilePath thumbnail-probe.txt`
(PowerShell). The probe still reads Enter from the console.

At every pause the console lists what to look at. Type a short note if you
like (the summary repeats it), then press Enter. `q` and Enter stops the
probe at any pause.

Only a line typed after the list appears ends a pause. When the probe
starts, it throws away the keys pressed during the build. A line typed
earlier than the list (a second Enter, or one pressed while a phase runs)
is printed as ignored. A `q` among those lines still stops the probe.

Ctrl+C, Ctrl+Break, closing the console and the probe window's close button
also stop the probe. Each of them runs the same clean-up: the probe
unregisters every thumbnail, then destroys every window of its own. If the
probe ever hangs, a second Ctrl+C ends the process at once; Windows then
frees its thumbnails and windows.
