# ADR-0004: Linux-first development with cross-target verification

Date: 2026-11-03 · Status: accepted

## Context

Most development happens in Linux containers, by AI coding agents. The
product runs only on Windows. Development needs the strongest feedback
possible without running the binary, and a clear statement of what that
feedback cannot prove.

## Decision

1. The architecture puts behaviour in host-independent crates
   (`stem-core`, `stem-service`), tested on Linux with the mock platform.
2. The real Windows code, including the Tauri crate, is type-checked and
   linted from Linux with this command:

   ```sh
   cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings
   ```

   `rust-toolchain.toml` installs the `rust-std` for the MSVC target. Check
   and clippy need no linker, but `tauri-build` needs `llvm-rc` on the host
   to compile the Windows resource file. This proves API usage, feature
   flags, types and lints, not runtime behaviour.
3. The frontend runs in a browser against `MockBackend`; Playwright takes
   screenshots that a person or an agent looks at.
4. GitHub Actions builds the real installer on `windows-latest`.
5. Every runtime claim about Windows has a manual test procedure, which a
   person runs on Windows. The exit criterion that ends a milestone in
   `docs/PLAN.md` is something a person observes.

On one Linux host, a Tauri 2.12 app with the `windows` crate type-checks
for the Windows target in about 2 minutes from a clean build and about
20 seconds after a change. Building the Tauri crate natively on Linux would
need the WebKitGTK development packages, which the project does not use.

## Consequences

- The Linux checks are fast and deterministic, and their results are
  accurate for what they cover.
- A passing cross-target clippy run can still hide runtime bugs. The manual
  tests on Windows cover those. The plan puts the first run on Windows
  early: the exit criterion of M1, the first milestone after the
  foundation, is a person installing Stem on Windows 11 and finding the
  dock working.
- `src-tauri` is kept thin because it cannot be unit-tested on Linux
  (ADR-0007).
- Path semantics differ: on Linux, `std::path::Path` does not treat `\` as
  a separator. Code that must behave the same in tests on every host uses
  explicit string handling (see `stem_core::normalize_path`).
