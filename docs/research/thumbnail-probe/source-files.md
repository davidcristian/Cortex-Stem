# Thumbnail probe: source files

The [thumbnail probe](index.md) is made of these files, and these checks
lint and test it:

- `crates/stem-platform/src/windows/probe/thumbnails.rs`: the probe. All
  of its `unsafe` code is in this file, each block with a `// SAFETY:`
  comment.
- `crates/stem-platform/examples/thumbnail-probe.rs`: the entry point.
- `crates/stem-platform/Cargo.toml`: the `probes` feature (off by default,
  never enabled by the app) and the example's `required-features`.
- `scripts/check-all.ps1` and `scripts/check-all.sh` lint the probe in
  their Windows-target clippy step (rung 4):

  ```text
  cargo clippy -p stem-platform --all-targets --features probes --target x86_64-pc-windows-msvc -- -D warnings
  ```

  The pure parts (fit, crop, layout, table, window picking) have unit tests
  that run on Windows:

  ```text
  cargo test -p stem-platform --features probes
  ```
