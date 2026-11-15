# Thumbnail probe: what it prints

Each registration of the [thumbnail probe](index.md) prints a block like
this (the values are illustrative):

```text
[1] 1. normal: own window, cropped to its frame [A1 A2 A7]
    source 0x1a0b2c: own window 0x1a0b2c
    state now: not minimised, not cloaked, display affinity 0x0
    DwmRegisterThumbnail ok in 85 us
    DwmQueryThumbnailSourceSize ok in 9 us
    DwmUpdateThumbnailProperties ok in 31 us
    source size (DWM): 494x307
    GetWindowRect 494x307 at (1402, 709); extended frame bounds 480x300 at (1409, 709)
    crop: rcSource (7, 0)-(487, 300)
    rcDestination 288x180 at (12, 88) (client coordinates)
```

A failed call reads `DwmRegisterThumbnail FAILED 0x80070005 (Access is
denied.) in 40 us`. A failure is a result, not a probe error: the probe
goes on.

At the end (and when it is stopped early) the probe prints the summary,
between `==== thumbnail probe summary` and `==== end of summary ====`:

- the Windows build, the probe window's DPI and scale, and whether the probe
  is elevated;
- one table row per slot: `origin` (own, found, not found, or slot 1 for
  slot 10), `state` (read when the summary is printed: `iconic`,
  `cloak 0xN`, `wda 0xN` or `-`), the `register` result and its time in µs,
  `size (DWM)`, `window` (`GetWindowRect` size), `frame` (extended frame
  bounds size), whether a `crop` applied, and the `update` result and its
  time;
- one line per slot naming what was found (title, class, pid, exe,
  elevation, minimised, desktop) or how to make it findable;
- `A6` lines: per API the number of calls, failures, slowest and mean time,
  and how many calls took longer than 16 ms;
- one line per phase step (A6 worker thread, A5 steps 1 to 4, A11 steps 1
  and 2);
- the notes typed at each pause.
