//! Core library for `tpt-profiler`.
//!
//! This crate owns the orchestrator pipeline (spec §5):
//!
//! ```text
//! discovery → analysis → evidence → opportunities → priority → report
//! ```
//!
//! and the analyzer registry (spec §67). It is the only crate allowed to
//! depend on all analysis crates; embedders should depend on this crate
//! rather than the internals.
//!
//! **Status:** the data model and configuration are in place; the
//! pipeline lands with the MVP (spec §62).

pub mod config;
pub mod discovery;
pub mod environment;

pub use config::{
    BenchmarksConfig, CiConfig, Config, Overrides, ProfilingConfig, ProjectConfig, ScoringConfig,
};

/// Version of the workspace, as reported by the CLI.
#[must_use]
pub const fn version() -> &'static str {
    tpt_profiler_model::version()
}

pub use tpt_profiler_model as model;
