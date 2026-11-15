# Performance budgets

This file lists Stem's performance targets, how the dock page's idle work is checked, and
times measured on one test PC.

| Metric                                  | Target |
|-----------------------------------------|--------|
| Idle CPU (no window changes)            | ≈ 0 % (nothing polls except the Recycle Bin query every 5 s on `dock-recycle-bin` and the hit-test thread's 4 Hz `GetCursorPos` safety poll; the dock page, in the dev build, does no work once settled, which `e2e/idle.spec.ts` checks, see "The dock page at idle") |
| Event → UI update                       | < 50 ms |
| Startup to dock visible                 | < 1 s |
| Memory (Rust side)                      | < 30 MB; WebView2 uses far more |
| Icon extraction per app                 | off the UI thread, bounded (timeouts of at most 200 ms); from the disk cache after the first start |

## The dock page at idle

`e2e/idle.spec.ts`, part of `pnpm e2e:screenshots`, opens the dock page in
mock mode in headless Chromium, one browser context per scenario. It runs
against the dev build (`pnpm dev`). It checks that once the page has
settled it does no work over a 3 s window:

- CDP `Performance.getMetrics` shows no layout and no style recalculation,
  at most 3 ms of script and at most 25 ms of main-thread tasks. On one PC
  a settled page measured 0 ms of script and 0.7 to 2.2 ms of tasks, and up
  to 3.8 ms of tasks while that PC was busy with other work;
- no frame, timer or idle callback runs and no timer is set, and at the
  end nothing repeats faster than once a minute;
- `document.getAnimations()` has nothing running.

Settled means: fonts and images are loaded, and for 250 ms in a row there
is no running animation, no pending frame or idle callback, no timer due
before the window would end (plus a 1 s margin), and nothing that repeats
faster than once a minute. How often a timer repeats:

- an interval: at its delay;
- a timeout due on a minute of the wall clock: at most once a minute (the
  clock item's tick);
- any other timeout: the time from the due time of the last timeout from
  the same call site that has run to this one's due time; while none from
  that call site has run, its delay.

Work once a minute is allowed, and the settle places the window between two
runs.

Scenarios:

- at rest on every edge, with each theme preset, in light mode, with a
  contrast theme and with transparency off;
- after a hover ends (bottom and left edges);
- after a launch bounce, a toast, a drag-reorder and an attention wiggle
  end;
- the clock and battery items on (with the page coming back into view: one
  clock timer is left), the clock switched on and off again (none of its
  timers is left), and a timeout set again each minute;
- auto-hide shown under a resting pointer, slid out, and brought back from
  the strip.

Control scenarios show that the checks detect busy work: a frame loop with
an interval (it exceeds both time budgets), a 10 s polling chain and an
idle-callback loop. The attention wiggle is busy by design until the
window is activated, so it is checked against its own limits: no layout,
no frame, timer or idle callback, script and tasks within the budgets, at
most 2 style recalculations per window, and nothing running but the
wiggle. Vite's dev client (a 30 s WebSocket ping) is not counted. The
bundle (`dist/`) can be measured with the same spec through `pnpm preview`
(see the spec's header comment); this is not part of the verification
ladder in `AGENTS.md`. Headless Chromium is not WebView2. The idle CPU of
the real processes can only be measured on Windows, in the same states:
after a hover, a drag and an auto-hide slide, and during a wiggle.

## Measured

These figures cover what can be measured without real windows on screen.
They come from the tests in `crates/stem-service/src/perf_tests.rs`, run
twice in release on one test PC (Windows 11, x86-64) while other builds
were also loading it:

```
cargo test -p stem-service --release perf_ -- --nocapture --test-threads 1
```

The verification ladder runs the same tests in the debug profile, which is
1 to 4 times slower (in parentheses below, also two runs). The tests assert
behaviour, never these times. The figures are orders of magnitude, not
benchmarks.

| Measurement | Result (debug) | Test |
|-------------|----------------|------|
| Event storm, open: 200 windows, each `WindowAdded` + `ForegroundChanged` (400 events, pushed in 0.2 ms) through the mock, the real worker and engine | final state 24 ms (62 to 64 ms) after the first event, in 2 `State` notifications; one identity lookup | `perf_a_storm_of_200_windows_…` |
| Event storm, close: the 200 closed, the foreground handed on each time (400 events) | 24 to 25 ms (64 to 65 ms), 1 notification; the items and the foreground equal the ones before the storm | same |
| Real extraction, 11 system exes, `WindowsPlatform::icon_png` with an empty memory cache | 96 px: median 3.2 to 3.3 ms, p95 7.9 to 8.2 ms (median 8.8 to 9.2 ms, p95 13 to 14 ms); 256 px: median 8.1 to 8.4 ms, p95 11.9 to 12.6 ms (median 13 to 14 ms, p95 18 ms) | `perf_real_extraction_against_a_disk_hit_on_windows` (Windows hosts only; file reads and GDI only) |
| Disk hit, the same icons (8 KB average at 96 px, 39 KB at 256 px) | median 0.09 to 0.13 ms (0.11 to 0.24 ms), of which 0.03 ms is the exe's `metadata`; the slowest of 11 took 2 to 4 ms, a first read of a file written moments before (probably on-access scanning; a start with a filled cache reads files written in an earlier session) | same |
| Disk hit, 100 entries of 64 KiB | median 0.13 to 0.14 ms (0.28 to 0.30 ms) | `perf_a_hit_on_a_large_icon_…` |
| Later request for a (key, size) the session already served (a second monitor's dock, a reload) | under 1 µs (1 µs): a map lookup and a copy of the PNG, no file touched | `perf_icon_requests_hit_the_disk_cache_after_a_restart` |
| Miss: what the cache adds to the caller | about 0.2 ms: two `metadata` calls, a failed open, the session map, a copy of the PNG and a non-blocking send (mock, 100 keys: 0.25 → 0.42 ms at 96 px, 0.67 → 0.84 to 0.88 ms at 256 px; real exes: within the noise of the extraction) | same |
| Write, on the `stem-icon-cache` thread | 0.8 to 1.2 ms per entry (0.8 to 1.3 ms), never on the caller's thread | `CacheStats::write_nanos` |
| Shutdown with the writer stuck for 3 s and writes queued behind it | returns after the 500 ms bound; the writer, once free, drops the queued writes (`abandoned`) and ends | `shutdown_waits_for_a_stuck_writer_…` (in `icon_cache`) |

How to read the storm numbers: the events are queued in 0.2 ms. The worker
drains everything queued in one batch and emits at most one `State` per
16 ms, so 400 events become 1 or 2 notifications. Nearly all of the 24 ms is
the worker applying the inputs one by one. `DockEngine::apply` compares
snapshots for each input, so an input costs time in proportion to the
number of windows listed: about 60 µs on average here, with 0 to 200
windows. That keeps a storm of this size inside the 50 ms event-to-UI
budget. A real storm also passes through the platform's WinEvent hook and
its per-window queries, which the mock skips.
