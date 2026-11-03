# Performance budgets

This file lists Stem's performance targets.

| Metric                                  | Target |
|-----------------------------------------|--------|
| Idle CPU (no window changes)            | ≈ 0 % (nothing polls except a slow Recycle Bin timer) |
| Event → UI update                       | < 50 ms |
| Startup to dock visible                 | < 1 s |
| Memory (Rust side)                      | < 30 MB; WebView2 uses far more |
| Icon extraction per app                 | off the UI thread, bounded (timeouts of at most 200 ms) |
