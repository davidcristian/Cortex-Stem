//! Host-independent rules used by the Windows implementation.
//!
//! Everything here is a pure function over plain data (`stem-core` types,
//! integers, strings) so it compiles and is unit-tested on every host. The
//! `windows/` modules gather OS facts and call into here for decisions.
//!
//! No `windows` crate imports, no `unsafe`, no I/O.

pub mod eligibility;
pub mod events;
pub mod hit_region;
pub mod icons;
pub mod identity;
pub mod recheck;
pub mod theme;
