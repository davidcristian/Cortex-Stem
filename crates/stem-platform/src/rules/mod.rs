//! Host-independent rules used by the Windows implementation.
//!
//! Everything here is a pure function over plain data (`stem-core` types,
//! integers, strings) so it compiles and is unit-tested on every host. The
//! `windows/` modules gather OS facts and call into here for decisions.
//!
//! No `windows` crate imports, no `unsafe`, no I/O.

pub mod appbar;
pub mod backdrop;
pub mod backoff;
pub mod eligibility;
pub mod events;
pub mod fullscreen;
pub mod hit_region;
pub mod icons;
pub mod identity;
pub mod locale;
pub mod power;
pub mod process_access;
pub mod recheck;
pub mod recycle;
pub mod shell;
pub mod taskbar;
pub mod theme;
pub mod volume;
pub mod wide;
