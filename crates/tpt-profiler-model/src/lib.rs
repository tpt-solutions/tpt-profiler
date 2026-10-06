//! Shared data model for `tpt-profiler`.
//!
//! This crate is the dependency-free leaf of the workspace (see
//! `DESIGN.md` §2): every other crate may depend on it, it depends on
//! nothing else in the workspace.
//!
//! It is `no_std` + [`alloc`] by default (spec §73); the optional `std`
//! feature enables OS-specific helpers, and the optional `serde` feature
//! (with `default-features = false`) adds serialisation to the persisted
//! and emitted types.
//!
//! Contents:
//!
//! * [`error`] — the structured error taxonomy (spec §74),
//!   which distinguishes *"no opportunity detected"* (not an error) from
//!   actual failures via [`outcome::AnalysisOutcome`].
//! * [`schema`] — versioned envelope for all persisted/emitted JSON
//!   (spec §69).
//! * [`hash`] — stable FNV-1a hashing used for deterministic IDs.
//! * [`id`] — deterministic, reproducible opportunity IDs (spec §25).

#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

pub mod error;
pub mod hash;
pub mod id;
pub mod outcome;
pub mod schema;

/// Version of this crate, as reported in run records and schema
/// envelopes.
#[must_use]
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
