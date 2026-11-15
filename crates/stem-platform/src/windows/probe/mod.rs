//! Diagnostic probes (feature `probes`, off by default).
//!
//! Throwaway programs that answer open questions about Windows behaviour on
//! a real Windows host, before any code builds on the answer. Nothing in the
//! app calls them; each one is run by hand through an example in
//! `crates/stem-platform/examples/`.
//!
//! - `thumbnails` — DWM thumbnails (ADR-0009 assumptions A1 to A11),
//!   run by `examples/thumbnail-probe.rs`; see
//!   `docs/research/thumbnail-probe/`.

pub mod thumbnails;
