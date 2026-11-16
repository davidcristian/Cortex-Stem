//! `stem-platform`: everything that talks to the operating system.
//!
//! The [`Platform`] trait is the boundary between the pure engine (`stem-core`)
//! and the OS. Two implementations exist:
//! - [`mock::MockPlatform`]: scripted, deterministic, runs on every host. Used
//!   by tests and by the app when `STEM_PLATFORM=mock`.
//! - [`windows::WindowsPlatform`] (`cfg(windows)` only): the real thing.
//!
//! Boundary rules (`AGENTS.md` §4, `docs/development/windows-rules.md`):
//! - no OS types in this file or in the trait;
//! - `unsafe` only under `src/windows/`;
//! - blocking or message-loop work happens on threads owned by this crate.

#![cfg_attr(not(windows), forbid(unsafe_code))]

pub mod mock;
mod platform;
pub mod rules;
#[cfg(windows)]
pub mod windows;

pub use platform::{
    AppBarGrant, AppBarRequest, AppBarSink, BackdropMaterial, DockBackdrop, DockWindowOptions,
    EventSink, HitRegion, NativeWindowHandle, Platform, PlatformError, PlatformResult,
    create_platform, is_mock,
};
