# Development hosts

The product is Windows-only. Development is Linux-first
([ADR-0004](../adr/0004-linux-first-verification.md)):

- `cargo test` covers the core, the platform mock and the service on
  Linux.
- `cargo clippy --workspace --target x86_64-pc-windows-msvc` type-checks
  and lints the real Windows code (including the Tauri crate) without
  linking.
- The frontend runs in a browser against `MockBackend`; Playwright takes
  screenshots to look at.
- GitHub Actions builds the real binary on `windows-latest`.
- Behaviour that exists only at runtime on Windows is tested by hand on a
  Windows PC.
