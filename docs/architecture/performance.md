# Performance budgets

This file lists Stem's performance targets and times measured on one test PC.

| Metric                                  | Target |
|-----------------------------------------|--------|
| Idle CPU (no window changes)            | ≈ 0 % (nothing polls except the Recycle Bin query every 5 s on `dock-recycle-bin` and the hit-test thread's 4 Hz `GetCursorPos` safety poll) |
| Event → UI update                       | < 50 ms |
| Startup to dock visible                 | < 1 s |
| Memory (Rust side)                      | < 30 MB; WebView2 uses far more |
| Icon extraction per app                 | off the UI thread, bounded (timeouts of at most 200 ms); from the disk cache after the first start |

## Measured

These figures cover what can be measured without real windows on screen.
They come from the tests in `crates/stem-service/src/perf_tests.rs`, run
once in release on one test PC (Windows 11, x86-64) while other builds
were also loading it:

```
cargo test -p stem-service --release perf_ -- --nocapture --test-threads 1
```

The verification ladder runs the same tests in the debug profile, which is
2 to 5 times slower (in parentheses below). The tests assert
behaviour, never these times. The figures are orders of magnitude, not
benchmarks.

| Measurement | Result (debug) | Test |
|-------------|----------------|------|
| Event storm, open: 200 windows, each `WindowAdded` + `ForegroundChanged` (400 events, pushed in 0.2 ms) through the mock, the real worker and engine | final state 24 ms (68 to 84 ms) after the first event, in 2 `State` notifications; one identity lookup | `perf_a_storm_of_200_windows_…` |
| Event storm, close: the 200 closed, the foreground handed on each time (400 events) | 24 ms (67 to 72 ms), 1 notification; the items and the foreground equal the ones before the storm | same |
| Real extraction, 11 system exes, `WindowsPlatform::icon_png` with an empty memory cache | 96 px: median 3.4 ms, p95 7.7 ms (median 16.5 ms, p95 39 ms); 256 px: median 8.2 ms, p95 12.6 ms (median 21 ms, p95 33 ms) | `perf_real_extraction_against_a_disk_hit_on_windows` (Windows hosts only; file reads and GDI only) |
| Disk hit, the same icons (8 KB average at 96 px, 39 KB at 256 px) | median 0.10 ms (0.2 to 0.25 ms), of which 0.03 ms is the exe's `metadata`; the slowest of 11 took 3 to 10 ms, a first read of a file written moments before (probably on-access scanning; a start with a filled cache reads files written in an earlier session) | same |
| Disk hit, 100 entries of 64 KiB | median 0.18 ms (0.23 to 0.37 ms) | `perf_a_hit_on_a_large_icon_…` |
| Miss: what the cache adds to the caller | about 0.15 ms: two `metadata` calls, a failed open, a copy of the PNG and a non-blocking send (mock, 100 keys: 0.28 → 0.45 ms at 96 px, 0.67 → 0.82 ms at 256 px; real exes: within the noise of the extraction) | `perf_icon_requests_hit_the_disk_cache_after_a_restart` |
| Write, on the `stem-icon-cache` thread | 0.8 to 1.2 ms per entry (0.9 to 2.3 ms), never on the caller's thread | `CacheStats::write_nanos` |

How to read the storm numbers: the events are queued in 0.2 ms. The worker
drains everything queued in one batch and emits at most one `State` per
16 ms, so 400 events become 1 or 2 notifications. Nearly all of the 24 ms is
the worker applying the inputs one by one. `DockEngine::apply` compares
snapshots for each input, so an input costs time in proportion to the
number of windows listed: about 60 µs on average here, with 0 to 200
windows. That keeps a storm of this size inside the 50 ms event-to-UI
budget. A real storm also passes through the platform's WinEvent hook and
its per-window queries, which the mock skips.
