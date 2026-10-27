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
//! # Example
//!
//! ```
//! use std::path::Path;
//! use tpt_profiler::{config::Config, pipeline};
//!
//! let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/single-crate");
//! let run = pipeline::analyse(&root, &Config::default())?;
//! for opportunity in &run.opportunities {
//!     println!("{} {}", opportunity.id, opportunity.description);
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Runnable version: `cargo run -p tpt-profiler --example analyse -- <path>`.

pub mod baseline;
pub mod build_time;
pub mod ci;
pub mod config;
pub mod deps;
pub mod discovery;
pub mod ecosystem;
pub mod environment;
pub mod github;
pub mod measure;
pub mod pipeline;
pub mod runs;
pub mod tracking;

pub use config::{
    BenchmarksConfig, CiConfig, Config, Overrides, ProfilingConfig, ProjectConfig, ScoringConfig,
    WorkloadConfig,
};

/// Version of the workspace, as reported by the CLI.
#[must_use]
pub const fn version() -> &'static str {
    tpt_profiler_model::version()
}

pub use tpt_profiler_model as model;

/// Function sites for mapping hotspots back to source (spec §8.5).
pub use tpt_profiler_static::functions;
