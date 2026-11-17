# Thumbnail probe: source files

The [thumbnail probe](index.md) is made of these files, and these checks
lint and test it:

- `crates/stem-platform/src/windows/probe/thumbnails.rs` and `thumbnails/`:
  the probe. All of its `unsafe` code is in these files, each block with a
  `// SAFETY:` comment.
- `crates/stem-platform/examples/thumbnail-probe.rs`: the entry point.
- `crates/stem-platform/Cargo.toml`: the `probes` feature (off by default,
  never enabled by the app) and the example's `required-features`.
- `scripts/check-all.ps1` and `scripts/check-all.sh` lint the probe in
  their Windows-target clippy step (rung 4):

  ```text
  cargo clippy -p stem-platform --all-targets --features probes --target x86_64-pc-windows-msvc -- -D warnings
  ```

  On a Windows host the same step also runs the unit tests of the pure
  parts (fit, crop, layout, table, window picking, the worker result line,
  the early-input drain):

  ```text
  cargo test -p stem-platform --features probes --lib windows::probe::
  ```

  On other hosts the module is not compiled, and the scripts print that the
  tests were skipped.
