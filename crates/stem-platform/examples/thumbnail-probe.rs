//! DWM thumbnail probe (ADR-0009). Run it once on a Windows host,
//! from a terminal that is not elevated:
//!
//! ```text
//! cargo run -p stem-platform --example thumbnail-probe --features probes
//! ```
//!
//! `docs/research/thumbnail-probe/` says what to open first and how to
//! read the output.
//! The code lives in `stem_platform::windows::probe::thumbnails`.

use std::process::ExitCode;

#[cfg(windows)]
fn main() -> ExitCode {
    stem_platform::windows::probe::thumbnails::run()
}

#[cfg(not(windows))]
fn main() -> ExitCode {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "thumbnail-probe runs on Windows only");
    ExitCode::FAILURE
}
