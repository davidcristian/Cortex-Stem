//! `stem-core` — the platform-independent heart of Stem.
//!
//! This crate owns:
//! - the **domain model** ([`model`]): windows, apps, dock items, monitors, theme;
//! - the **settings** model with defaults, validation, and migration ([`settings`]);
//! - the **events** that flow from a platform into the engine ([`events`]);
//! - the **engine** ([`engine`]): a pure state machine that turns platform
//!   events + settings into a [`model::DockState`] snapshot and decides what a
//!   click on an item should do;
//! - the **overlap rule** ([`overlap`]): whether a window covers the dock's
//!   resting panel, which the engine reports as
//!   [`model::DockState::overlapped`].
//!
//! Rules (see `AGENTS.md`): no `unsafe`, no platform APIs, no `tauri`. Every
//! behaviour here is unit-testable on any host, which is where most of the
//! project's confidence comes from.

#![forbid(unsafe_code)]

pub mod engine;
pub mod events;
pub mod model;
pub mod overlap;
pub mod settings;
/// TypeScript export pipeline: `cargo test -p stem-core --features ts`.
#[cfg(all(feature = "ts", test))]
mod ts;

pub use engine::{DockEngine, EngineOutcome, ItemAction, app_id_for};
pub use events::{EngineInput, PlatformEvent};
pub use model::*;
pub use settings::{Settings, SettingsError};
